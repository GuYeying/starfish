//! Starfish 构建工具（xtask 模式：构建工具即 Rust 代码，跨平台无脚本依赖）
//!
//! 用法（经 cargo 别名，见 .cargo/config.toml）：
//! ```text
//! cargo xtask list                                   # 列出全部示例及安卓可用性
//! cargo xtask android 03_texture                     # 构建+打包（有设备则部署）
//! cargo xtask android 03_texture --build             # 仅构建出 APK，不装机
//! cargo xtask android 16_dialog --abi x86_64         # 指定 ABI（模拟器用 x86_64）
//! cargo xtask android 15_empty_window --no-default-features --features dialog,gfx
//! ```
//!
//! 设计约定：
//! - **特性勾选**：`--features a,b` 追加特性、`--no-default-features` 剔除默认。
//!   Android 支持状态登记在 [`FEATURE_ANDROID_SUPPORT`]——**未来新增 Rust 模块，
//!   在此表加一行即可**；未登记的特性照常传给 cargo（附提醒）。
//! - **示例双注册**：桌面 bin + `*_android` cdylib 两条 `[[example]]` 同源文件。
//!   传 `03_texture` 自动解析到 `03_texture_android`（存在同名精确匹配则优先）。
//! - **管线**：cargo ndk 编译 → aapt2 打包 → 内嵌 .so/classes.dex（dialog 模块
//!   的 FilePickerFragment，见 reference/android构建与运行指南.md 坑位 12）→
//!   apksigner 签名 → adb 部署。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Write;

// ════════════════════ 模块支持注册表（未来模块在此登记） ════════════════════

#[derive(Clone, Copy, PartialEq)]
enum Support {
    /// Android 全功能可用
    Ok,
    /// Android 空实现占位（编译通过，功能不可用）
    Stub,
}

const FEATURE_ANDROID_SUPPORT: &[(&str, Support)] = &[
    ("gfx", Support::Ok),
    ("font", Support::Ok),
    ("video", Support::Ok),
    ("dialog", Support::Ok),
    ("net", Support::Ok),
    ("gamepad", Support::Stub),
];

/// 所有 APK 共用的权限清单要素（robust 起见全部声明，运行时未用则无副作用）
const PKG: &str = "com.starfish.test";
const ABI_DEFAULT: &str = "arm64-v8a";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => cmd_list(),
        Some("android") => cmd_android(&args[1..]),
        Some(h @ ("help" | "--help" | "-h")) => {
            println!("{HELP}");
            let _ = h;
        }
        _ => {
            eprintln!("{HELP}");
            std::process::exit(2);
        }
    }
}

const HELP: &str = "\
Starfish 构建工具
  cargo xtask list                列出全部示例及安卓可用性
  cargo xtask android <示例> [选项]
    <示例>                        示例名（03_texture 自动解析 03_texture_android）
    --abi <arm64-v8a|x86_64>      目标 ABI（默认 arm64-v8a）
    --orientation <方向>          横竖屏（默认 landscape；可选 portrait / fullSensor /
                                  sensorLandscape / sensorPortrait / unspecified 等）
    --features <a,b,...>          追加特性（勾选组件库）
    --no-default-features         剔除默认特性集
    --build                       仅构建出 APK，不 adb 部署
    --debug                       debug 构建（默认 release）";

// ════════════════════ 参数解析 ════════════════════

struct AndroidArgs {
    example: String,
    abi: String,
    orientation: String,
    features: Vec<String>,
    no_default_features: bool,
    deploy: bool,
    release: bool,
}

/// 合法 screenOrientation 值（游戏库默认横屏）
const ORIENTATIONS: &[&str] = &[
    "landscape", "portrait", "fullSensor", "sensorLandscape", "sensorPortrait",
    "unspecified", "fullUser", "userLandscape", "userPortrait", "locked",
];

fn parse_android_args(raw: &[String]) -> AndroidArgs {
    let mut it = raw.iter();
    let example = it
        .next()
        .filter(|s| !s.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("✗ 缺少示例名。用法: cargo xtask android <示例> [--abi ...] [--build]");
            std::process::exit(2);
        });
    let mut out = AndroidArgs {
        example,
        abi: ABI_DEFAULT.to_string(),
        orientation: "landscape".to_string(),
        features: vec![],
        no_default_features: false,
        deploy: true,
        release: true,
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--abi" => out.abi = it.next().cloned().unwrap_or_else(|| die("--abi 需要值")),
            "--orientation" => {
                let o = it.next().cloned().unwrap_or_else(|| die("--orientation 需要值"));
                if !ORIENTATIONS.contains(&o.as_str()) {
                    die(&format!("--orientation {o} 不合法。可选: {}", ORIENTATIONS.join(" / ")));
                }
                out.orientation = o;
            }
            "--features" => {
                let f = it.next().cloned().unwrap_or_else(|| die("--features 需要值"));
                out.features.extend(f.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from));
            }
            "--no-default-features" => out.no_default_features = true,
            "--build" => out.deploy = false,
            "--debug" => out.release = false,
            other => die(&format!("未知参数 {other}（--help 看用法）")),
        }
    }
    out
}

fn die(msg: &str) -> ! {
    eprintln!("✗ {msg}");
    std::process::exit(1);
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap_or_else(|e| die(&format!("启动 {:?} 失败: {e}", cmd.get_program())));
    if !status.success() {
        die(&format!("命令失败: {:?}", cmd));
    }
}

// ════════════════════ Cargo.toml 解析（示例清单） ════════════════════

struct ExampleEntry {
    name: String,
    path: String,
    is_cdylib: bool,
    required_features: Vec<String>,
}

fn parse_examples() -> Vec<ExampleEntry> {
    let toml = std::fs::read_to_string("Cargo.toml").expect("读取 Cargo.toml 失败");
    let mut out = vec![];
    let mut current: Option<ExampleEntry> = None;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with("[[example]]") {
            if let Some(e) = current.take() {
                out.push(e);
            }
            current = Some(ExampleEntry { name: String::new(), path: String::new(), is_cdylib: false, required_features: vec![] });
        } else if let Some(e) = current.as_mut() {
            if let Some(v) = t.strip_prefix("name = ") {
                e.name = unquote(v);
            } else if let Some(v) = t.strip_prefix("path = ") {
                e.path = unquote(v);
            } else if t.starts_with("crate-type") {
                e.is_cdylib = t.contains("cdylib");
            } else if let Some(v) = t.strip_prefix("required-features = ") {
                e.required_features = v.trim_start_matches('[').trim_end_matches(']').split(',').map(unquote).filter(|s| !s.is_empty()).collect();
            }
        }
    }
    if let Some(e) = current.take() {
        out.push(e);
    }
    out
}

fn unquote(v: &str) -> String {
    v.trim().trim_matches('"').to_string()
}

/// 解析示例名：精确匹配优先；否则尝试 `<输入>_android`（双注册约定）
fn resolve_example<'a>(input: &str, examples: &'a [ExampleEntry]) -> &'a ExampleEntry {
    examples
        .iter()
        .find(|e| e.name == input)
        .or_else(|| examples.iter().find(|e| e.name == format!("{input}_android")))
        .unwrap_or_else(|| {
            let hits: Vec<&str> = examples.iter().map(|e| e.name.as_str()).filter(|n| n.contains(input)).take(6).collect();
            die(&format!(
                "未找到示例 `{input}`。相近项: {}（cargo xtask list 看全部）",
                if hits.is_empty() { "无".into() } else { hits.join(", ") }
            ));
        })
}

/// Android 构建的解析：必须落在本注册为 cdylib 的条目上——
/// 1) 输入已是 *_android 2) 存在 <输入>_android 双注册兄弟 3) 输入本身是 cdylib
fn resolve_android_example<'a>(input: &str, examples: &'a [ExampleEntry]) -> &'a ExampleEntry {
    let by_name = |n: String| examples.iter().find(|e| e.name == n);
    let pick = |e: &'a ExampleEntry| -> &'a ExampleEntry {
        if e.is_cdylib {
            e
        } else {
            die(&format!(
                "`{}` 是桌面 bin 条目，不能产出 Android 可加载的共享库（需 crate-type = [\"cdylib\"] 注册）",
                e.name
            ))
        }
    };
    if let Some(e) = input.strip_suffix("_android").and_then(|_| by_name(input.to_string())) {
        return pick(e);
    }
    if let Some(e) = by_name(format!("{input}_android")) {
        return pick(e);
    }
    if let Some(e) = by_name(input.to_string()) {
        return pick(e);
    }
    die(&format!("未找到示例 `{input}`（cargo xtask list 看全部）"))
}

// ════════════════════ SDK 工具定位 ════════════════════

fn android_home() -> PathBuf {
    std::env::var("ANDROID_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| die("未设置 ANDROID_HOME"))
}

/// build-tools 取最高版本目录（不写死版本号）
fn build_tools() -> PathBuf {
    let root = android_home().join("build-tools");
    let best = std::fs::read_dir(&root)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.split('.').map(|s| s.parse::<u32>().ok()).collect::<Option<Vec<u32>>>().map(|v| (v, e.path()))
                })
                .max_by_key(|(v, _)| v.clone())
                .map(|(_, p)| p)
        })
        .flatten()
        .unwrap_or_else(|| die(&format!("build-tools 未找到（{}）", root.display())));
    best
}

/// android.jar：ANDROID_JAR 环境变量优先，否则取最高 API level
fn android_jar() -> PathBuf {
    if let Ok(p) = std::env::var("ANDROID_JAR") {
        return PathBuf::from(p);
    }
    let root = android_home().join("platforms");
    let best = std::fs::read_dir(&root)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.strip_prefix("android-").map(|s| s.to_string()).map(|n| (n.parse::<u32>().unwrap_or(0), e.path()))
                })
                .max_by_key(|(v, _)| *v)
                .map(|(_, p)| p.join("android.jar"))
        })
        .flatten()
        .unwrap_or_else(|| die("platforms/android-*.jar 未找到"));
    best
}

fn apksigner(bt: &Path) -> Command {
    // Windows build-tools 里 apksigner 是 .bat——CreateProcess 不能直接执行批处理，
    // 须经 cmd /c 包装（Rust 侧参数原样传递，bat 内部 %* 展开正确）
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/c").arg(bt.join("apksigner.bat"));
        c
    }
    #[cfg(not(windows))]
    Command::new(bt.join("apksigner"))
}

fn abi_to_triple(abi: &str) -> &'static str {
    match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "x86_64" => "x86_64-linux-android",
        "armeabi-v7a" => "armv7-linux-androideabi",
        other => die(&format!("未知 ABI: {other}（支持 arm64-v8a / x86_64 / armeabi-v7a）")),
    }
}

fn keystore() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .expect("无法定位用户目录");
    PathBuf::from(home).join(".android").join("debug.keystore")
}

/// robius-file-picker 的 classes.dex（dialog 模块运行时依赖；Cargo 增量保留最新）
fn robius_dex(triple: &str) -> Option<PathBuf> {
    let root = PathBuf::from(format!("target/{triple}/release/build"));
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("robius-file-picker-") {
            let dex = entry.path().join("out").join("classes.dex");
            if let Ok(meta) = std::fs::metadata(&dex) {
                let mtime = meta.modified().ok()?;
                if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                    best = Some((mtime, dex));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

// ════════════════════ 命令：list ════════════════════

fn cmd_list() {
    let examples = parse_examples();
    println!("Starfish 示例（{} 条注册）", examples.len());
    println!("{:-<86}", "");
    let mut shown: Vec<&str> = vec![];
    for e in &examples {
        if e.is_cdylib || shown.contains(&e.name.as_str()) {
            continue;
        }
        let android = examples.iter().find(|x| x.name == format!("{}_android", e.name));
        let mark = if android.is_some() { "✅ 双平台" } else { "── 桌面专属" };
        let feats = if e.required_features.is_empty() {
            String::new()
        } else {
            format!("  [特性: {}]", e.required_features.join(","))
        };
        println!("{:<28} {}{}", e.name, mark, feats);
        shown.push(&e.name);
    }
    println!("{:-<86}", "");
    println!("Android 支持注册表：");
    for (name, s) in FEATURE_ANDROID_SUPPORT {
        let note = match s {
            Support::Ok => "✅ 可用",
            Support::Stub => "⚠ 空实现占位（编译过、功能无）",
        };
        println!("  {name:<10} {note}");
    }
}

// ════════════════════ 命令：android ════════════════════

fn cmd_android(raw: &[String]) {
    let a = parse_android_args(raw);
    let examples = parse_examples();
    let entry = resolve_android_example(&a.example, &examples);
    let triple = abi_to_triple(&a.abi);

    // ── 特性勾选检查：登记表外的提醒、空实现占位警示 ──
    let all_features = entry.required_features.iter().chain(a.features.iter());
    for f in all_features {
        match FEATURE_ANDROID_SUPPORT.iter().find(|(n, _)| n == f) {
            Some((_, Support::Ok)) => {}
            Some((n, Support::Stub)) => println!("⚠ 特性 {n} 为 Android 空实现占位——编译通过但功能不可用"),
            None => println!("⚠ 特性 {f} 未登记 Android 支持状态（xtask src/main.rs FEATURE_ANDROID_SUPPORT 加一行），照常传给 cargo"),
        }
    }

    println!("── [1/4] cargo ndk 编译（{}，API 26，示例 {}）", a.abi, entry.name);
    let mut ndk = Command::new("cargo");
    ndk.args(["ndk", "--platform", "26", "-t", triple, "-o", "./jniLibs", "build"]);
    if a.release {
        ndk.arg("--release");
    }
    ndk.args(["--example", &entry.name]);
    if a.no_default_features {
        ndk.arg("--no-default-features");
    }
    if !a.features.is_empty() {
        ndk.arg("--features").arg(a.features.join(","));
    }
    run(&mut ndk);

    let so = PathBuf::from(format!("jniLibs/{}/lib{}.so", a.abi, entry.name));
    if !so.is_file() {
        die(&format!("未找到 {}（示例需 [[example]] crate-type = [\"cdylib\"]）", so.display()));
    }

    println!("── [2/4] aapt2 打包（NativeActivity 模板）");
    let bt = build_tools();
    let jar = android_jar();
    let out_dir = PathBuf::from("target/android-apk");
    std::fs::create_dir_all(&out_dir).unwrap();
    let manifest_out = out_dir.join("AndroidManifest.xml");
    let manifest_tpl = std::fs::read_to_string("android/AndroidManifest.xml").expect("读取清单模板失败");
    let manifest = manifest_tpl
        .replace("__LIB_NAME__", &entry.name)
        .replace("__ORIENTATION__", &a.orientation);
    std::fs::write(&manifest_out, manifest).unwrap();
    println!("  方向 = {}", a.orientation);
    let unsigned = out_dir.join("app.unsigned.apk");
    run(&mut Command::new(bt.join("aapt2")).args([
        "link", "-I", &jar.to_string_lossy(), "--manifest", &manifest_out.to_string_lossy(), "-o", &unsigned.to_string_lossy(),
    ]));
    zip_append(&unsigned, &so, &format!("lib/{}/{}", a.abi, so.file_name().unwrap().to_string_lossy()));
    // dialog 模块的 FilePickerFragment dex（未启用 dialog 时找不到 → 仅告警）
    match robius_dex(triple) {
        Some(dex) => {
            zip_append(&unsigned, &dex, "classes.dex");
            println!("  已并入 classes.dex（dialog 模块选择器）");
        }
        None => println!("  ⚠ 未找到 robius classes.dex——dialog 在此包不可用（其余不受影响）"),
    }

    println!("── [3/4] apksigner 签名");
    let signed = out_dir.join(format!("{}.apk", entry.name));
    let ks = keystore();
    run(&mut apksigner(&bt).args([
        "sign", "--ks", &ks.to_string_lossy(), "--ks-pass", "pass:android", "--key-pass", "pass:android",
        "--out", &signed.to_string_lossy(), &unsigned.to_string_lossy(),
    ]));
    run(&mut apksigner(&bt).arg("verify").arg(signed.to_string_lossy().as_ref()));
    println!("  签名校验通过");

    println!("── [4/4] 产物：{}", signed.display());
    if !a.deploy {
        println!("（--build 模式：跳过部署）");
        return;
    }
    println!("── 部署（adb）");
    let devices = Command::new("adb").arg("devices").output().ok();
    let attached = devices
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.ends_with("\tdevice")).count())
        .unwrap_or(0);
    if attached == 0 {
        println!("  ⚠ 无已连接设备——仅出包。连接后: adb install -r {} 手动装", signed.display());
        return;
    }
    run(&mut Command::new("adb").args(["install", "-r", &signed.to_string_lossy()]));
    run(&mut Command::new("adb").args([
        "shell", "am", "start", "-n", &format!("{PKG}/android.app.NativeActivity"),
    ]));
    println!("── 看日志：adb logcat -s RustStdoutStderr RustLog RustPanic AndroidRuntime:E");
}

/// 文件以 deflate 压缩并入既有 zip（APK）
///
/// 必须用 `new_append`（先解析既有中央目录再续写）——`new` 从 0 计偏移，
/// 追加进非空包会把目录写坏（apksigner 报 Malformed ZIP Central Directory）
fn zip_append(apk: &Path, file: &Path, entry_name: &str) {
    let data = std::fs::read(file).unwrap_or_else(|e| die(&format!("读取 {} 失败: {e}", file.display())));
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(apk)
        .unwrap_or_else(|e| die(&format!("打开 {} 失败: {e}", apk.display())));
    let mut w = zip::ZipWriter::new_append(f)
        .unwrap_or_else(|e| die(&format!("{} 不是有效 zip: {e}", apk.display())));
    w.start_file(entry_name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))
        .unwrap_or_else(|e| die(&format!("写入 {entry_name} 失败: {e}")));
    w.write_all(&data).unwrap();
    w.finish().unwrap();
}
