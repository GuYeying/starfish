# Android 闪退与 Invalid surface 问题定位与修复（hello 案例）

> 2026-09-27。starfish-rewrite hello 测试案例 Android 线的两个独立问题：
> ① 点击图标**立即闪退**（已修复，根因 = cdylib 未导出 `android_main`）；
> ② 修复后 GPU 初始化正常但 `Surface::configure` 报 **`Invalid surface`**
> （对照实验定性为**模拟器环境层问题**，真机待验收）。
> 关联：`starfish-window/android绑定运行问题与解决方案.md`（旧架构同主题）、
> `doc/log/starfish_changelog_2026-09-27.md`。

---

## 问题一：启动即闪退（已修复）

### 1.1 症状

点击应用图标**瞬间闪退**——无窗口、无 ANR、Rust 侧无任何日志输出
（说明 `android_main` 压根没执行到）。

### 1.2 定位：符号表直接给出铁证

不猜，先验产物。解包 APK 取出 `.so`，查**动态符号表**：

```bash
# NDK llvm-nm；-D 读 .dynamic（dlopen/dlsym 真正使用的表）
llvm-nm -D lib/arm64-v8a/libhello_android.so | grep -E "android_main|ANativeActivity"

# 旧包（闪退版）：
#   U android_main              ← 未定义！胶水 import 无人实现
#   T ANativeActivity_onCreate
```

`U`（Undefined）= 本 `.so` **引用**了 `android_main` 但**没人定义**。
而 `hello.rs` 源码里确实没有这个函数——桌面入口是 `fn main()`，
被注册成 cdylib（`hello_android`）后 `main` 只是普通函数，不是入口。

### 1.3 机理

打包用的是系统 `NativeActivity`（AndroidManifest 的 `meta-data
android.app.lib_name="hello_android"`）：

```
Activity 启动
  → 系统 dlopen("libhello_android.so")
  → 调 ANativeActivity_onCreate（android-activity 胶水提供）
  → 胶水 dlsym(handle, "android_main")       ← 找不到
  → abort → 瞬间闪退
```

`android_main` 是 NativeActivity 模型下**宿主必须导出的 Rust 符号**
（`#[no_mangle]`），等于桌面世界的 `main`。cdylib 不会自动导出任何
非 `#[no_mangle]` 符号，缺了就是启动即死，且**无任何日志**——
死在 dlsym，Rust 代码一行都没跑。

### 1.4 修复

`examples/hello.rs` 增加 Android 入口（三平台仍同一份 `app_body`）：

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: starfish::base::window::AndroidApp) {
    starfish::base::window::android_init(app);   // 装配 starfish-window 回调
    starfish::base::debug::console_log("[hello] android_main 启动");
    if let Err(e) = pollster::block_on(app_body()) {
        starfish::base::debug::console_log(&format!("[hello] 退出: {e}"));
    }
}
```

- `android_init`（starfish-window）：覆盖式装配 `AndroidApp`、重置
  单窗口占位与事件缓冲（进程复用安全）。
- 日志经 android-activity 胶水的 stdout/stderr→logcat 重定向：
  `adb logcat -s RustStdoutStderr` 可见，不需要 android_logger。

### 1.5 修复验证（符号级 + 实跑级）

```bash
llvm-nm -D libhello_android.so | grep android_main
#   0000000000199608 T android_main    ← T = 已导出 ✅
```

模拟器实跑：应用正常启动、日志出现 `[hello] android_main 启动`、
wgpu 适配器/设备初始化成功——闪退消失。（GPU 表面见问题二。）

---

## 问题二：`Invalid surface`——模拟器环境层死穴（定性非回归）

### 2.1 症状

闪退修复后，GPU 初始化全绿（适配器枚举、request_device、feature
report 都成功），随后：

```
thread '<unnamed>' panicked at wgpu-30.0.1/src/backend/wgpu_core.rs:3981:
wgpu error: Validation Error
Caused by:
  In Surface::configure
    Invalid surface
```

崩点在 `RenderSurface::new` 的首次 `surface.configure()`。

### 2.2 对照实验（三步定性）

| 实验 | 结果 | 结论 |
|---|---|---|
| ① 换 GPU 模式：`-gpu swiftshader_indirect` → `-gpu host`（Radeon 780M 直通） | **同样失败** | 排除 SwiftShader 特有缺陷 |
| ② 强制 Vulkan 后端（`with_backends(VULKAN)`） | `RequestAdapter` 失败 | 模拟器无 Vulkan 适配器，此路不通（实验开关已回退） |
| ③ **装旧 winit 架构的 probe_gfx APK 跑同一模拟器** | **一模一样的 `Invalid surface`，同一 wgpu 30.0.1 位置** | 与 rewrite 代码无关，环境层问题 |

实验③是关键一步：**用已知基线做对照**，一次把"rewrite 回归"和
"wgpu GLES × 模拟器 GL 转译层"划清。结论与旧架构文档
`starfish-window/android绑定运行问题与解决方案.md` §3.4 一致：
"真机 GPU/EGL 环境不同，惰性装配后实测通过"——模拟器 GL 层当时
就没跑通，属于遗留环境问题。

注：hello 的 `WindowInner::new`（Android 后端）已经**阻塞等待
`native_window()` 就绪（上限 10s）**才返回，`begin_frame` 也有
catch_unwind + 3 次重试跳帧自愈——就绪时序层面 rewrite 已比旧架构
更稳，残余失败确与就绪时序无关。

### 2.3 处置

- **模拟器不作为 Android 渲染验收平台**（GLES configure 死穴 +
  无 Vulkan，双后端全灭）；逻辑层（入口/事件/循环/日志）可用模拟器。
- **渲染验收走 arm64 真机**：`target/android-apk/hello_android.apk`
  （2.2 MB，已签名，`android_main` 已导出），安装后看窗口是否纯绿。

---

## 附 A：Android 打包链（starfish-rewrite 现行手动流程）

```bash
# 0) 构建（注意 -P 大写！小写 -p 是 --package 会 panic）
export ANDROID_NDK_ROOT=<ndk路径>
cargo ndk -t arm64-v8a -P 26 -o jniLibs -- build --release --example hello_android
# 模拟器逻辑层验证用：-t x86_64（arm64 包模拟器跑不了）

# 1) strip（8.6MB → 5.5MB）
llvm-strip jniLibs/arm64-v8a/libhello_android.so
# ⚠️ strip 后查符号必须加 -D（动态表），静态表已被删，裸 nm 会误报"丢失"

# 2) 替换 APK 内 .so（python zipfile，保持其余条目原样）
# 3) 对齐 + 签名（debug keystore，口令 android，alias androiddebugkey）
zipalign -f 4 repack.apk aligned.apk
apksigner sign --ks ~/.android/debug.keystore --ks-pass pass:android \
  --key-pass pass:android --out hello_android.apk aligned.apk
```

APK 模板：`target/android-apk/AndroidManifest.xml`（NativeActivity +
`lib_name="hello_android"`，minSdk 26）。示例双注册形态：

```toml
[[example]]
name = "hello"                          # 桌面 bin
[[example]]
name = "hello_android"                  # Android cdylib（同一路径源码）
path = "examples/hello.rs"
crate-type = ["cdylib"]
```

## 附 B：诊断命令速查

```bash
adb logcat -d -s RustStdoutStderr        # Rust stdout/stderr/panic（胶水自动重定向）
adb shell pidof com.starfish.test        # 进程存活判定
llvm-nm -D libXXX.so | grep android_main # T=导出 / U=缺失（缺失=启动即闪退）
adb logcat -d | grep -aE "DEBUG|libc"    # tombstone 帧（自带符号化）
```

## 附 C：状态

| 项 | 结果 |
|---|---|
| 闪退（问题一） | ✅ 已修复（符号级 + 模拟器实跑双验证） |
| Invalid surface（问题二） | ✅ 定性成立：arm64 真机渲染**验收通过（2026-09-27，纯绿）**——真机 GLES/EGL 路径正常，确证该问题为模拟器独有环境层死穴 |
| arm64 真机渲染验收 | ✅ 通过 |
