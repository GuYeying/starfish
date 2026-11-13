//! 统一日志模块：三 sink（文件 / wasm 远程 / 控制台直出）+ 六级过滤 +
//! 第三方 `log` crate 桥接（wgpu/egui/rapier/rustpython 等外部输出
//! 一键收编）+ **退化定约**：未初始化或文件写入失败 = 自动退化控制台
//! 输出，消息永不丢失。
//!
//! 编译期剔除：`trace!`/`debug!` 宏在 release 构建（`!debug_assertions`）
//! 展开为空——库内调试细语不进生产包；Error/Warn/Info 恒编译（运行时
//! 级别过滤），崩溃后日志文件仍可取证。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
#[cfg(target_arch = "wasm32")]
mod web;

/// 日志级别（对齐 `log` crate——第三方桥接零转换成本；词汇与 Unreal/
/// Unity 级别族同构精简）
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Off => "OFF",
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

impl From<log::Level> for Level {
    fn from(l: log::Level) -> Self {
        match l {
            log::Level::Error => Level::Error,
            log::Level::Warn => Level::Warn,
            log::Level::Info => Level::Info,
            log::Level::Debug => Level::Debug,
            log::Level::Trace => Level::Trace,
        }
    }
}

impl From<Level> for log::LevelFilter {
    fn from(l: Level) -> Self {
        match l {
            Level::Off => log::LevelFilter::Off,
            Level::Error => log::LevelFilter::Error,
            Level::Warn => log::LevelFilter::Warn,
            Level::Info => log::LevelFilter::Info,
            Level::Debug => log::LevelFilter::Debug,
            Level::Trace => log::LevelFilter::Trace,
        }
    }
}

/// 日志配置（创建时一次性——[`init`] 消费；后续 init 忽略并告警）
pub struct LogConfig {
    /// 全局级别阈值（默认 Info；trace/debug 另受 release 编译剔除约束）
    pub level: Level,
    /// 桌面/Android：日志文件全路径（追加写；宿主用 io 基准目录组装）
    pub file: Option<std::path::PathBuf>,
    /// wasm：远程收集端点（POST text/plain 批量上行——wasm 无文件系统，
    /// 日志唯一持久通道；缺省 = 退化控制台）
    pub remote: Option<String>,
    /// 控制台直出阈值（None = 不直出，纯文件/远程；Some(Warn) = 级别
    /// ≥ Warn 的消息同时上控制台）
    pub console: Option<Level>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: Level::Info, file: None, remote: None, console: None }
    }
}

struct State {
    level: Level,
    console: Option<Level>,
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    // 启动期诊断计数（stats_string 消费——定位日志自身开销用）
    emit_count: u64,        // 自家宏输出条数
    emit_ms: u128,          // 自家输出累计耗时（含文件写入）
    bridge_count: u64,      // 第三方（log crate 桥）接入条数
    bridge_ms: u128,        // 第三方输出累计耗时
    /// 桌面/Android 文件句柄（None = 未配置或已退化）
    file: Option<std::fs::File>,
    /// 文件路径（Android 重开追加模式用；桌面句柄模式忽略）
    file_path: Option<std::path::PathBuf>,
    /// 文件写入失败退化标记（置位后不再尝试文件，全部退化控制台）
    file_failed: bool,
    /// wasm 远程端点
    remote: Option<String>,
    /// wasm 批量缓冲（≥16 行或 Error 触发上行）
    remote_buf: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static INITED: AtomicBool = AtomicBool::new(false);

/// 初始化（创建时一次性；重复调用忽略——首个配置生效）。
/// 同时安装 `log` crate 桥（wgpu/egui/rapier/rustpython 等外部输出
/// 收编进同一管道与级别过滤）。
pub fn init(config: LogConfig) {
    if INITED.load(Ordering::SeqCst) {
        // 重复 init = 首个配置生效——但必须**留痕**（否则宿主以为主通道
        // 已换址，实际还在旧槽：Android MainActivity 先行 init 的场景
        // 实测 2026-10-31）
        __emit(Level::Warn, format_args!(
            "[starfish-debug] 重复 init 被忽略（首个配置生效）——新 file={:?}",
            config.file,
        ));
        return;
    }
    let file = config.file.as_ref().and_then(|path| {
        match std::fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => Some(f),
            Err(e) => {
                // 打不开 = 退化控制台 + **旁路文件自曝**（嵌入形态控制台
                // 不可见——裸写旁路把错误落到磁盘可取证）
                let alt = format!("{}.openfail", path.display());
                let _ = std::fs::write(
                    &alt,
                    format!("日志文件打开失败: {e}
（本文件 = 失败自曝旁路）"),
                );
                console_raw(&format!(
                    "[starfish-debug] 日志文件打开失败 {path:?}: {e}——退化控制台输出"
                ));
                None
            }
        }
    });
    let file_failed = config.file.is_some() && file.is_none();
    *STATE.lock().unwrap() = Some(State {
        level: config.level,
        console: config.console,
        #[cfg(not(target_arch = "wasm32"))]
        start: std::time::Instant::now(),
        emit_count: 0,
        emit_ms: 0,
        bridge_count: 0,
        bridge_ms: 0,
        file,
        file_path: config.file.clone(),
        file_failed,
        remote: config.remote,
        remote_buf: Vec::new(),
    });
    INITED.store(true, Ordering::SeqCst);
    // 第三方桥（log crate 全局单 logger；已被占 = 忽略，自家管道照常）
    let _ = log::set_boxed_logger(Box::new(FacadeBridge));
    log::set_max_level(config.level.into());
}

/// 引擎内统一输出口（宏的落地函数；msg = 预格式化消息）
pub fn __emit(level: Level, msg: impl std::fmt::Display) {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let mut guard = STATE.lock().unwrap();
    let Some(st) = guard.as_mut() else {
        // 退化定约：未初始化 = 控制台直出（消息永不丢）
        console_raw(&format!("[{}][未初始化] {msg}", level.as_str()));
        return;
    };
    if level > st.level {
        return;
    }
    let line = if cfg!(target_arch = "wasm32") {
        st_count_inc();
        format!("[{}#{:>04}][{}] {msg}", "log", count_load(), level.as_str())
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            format!("[+{:>6}ms][{}] {msg}", st.start.elapsed().as_millis(), level.as_str())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = st;
            format!("[{}][{}] {msg}", count_load(), level.as_str())
        }
    };
    // ① 文件（桌面/Android）；失败 = 永久退化控制台
    //    Android：**按路径重开追加**（每条一次 open）——长句柄在卓易通
    //    FUSE 上写入对外不可见（句柄缓存实证 2026-10-31），重开写 =
    //    write_diag 同款模式（该模式全站可见性实证）。桌面保留长句柄。
    let mut degrade = st.file_failed;
    if st.file_failed {
        // 已退化：不再尝试文件
    } else if cfg!(target_os = "android") {
        #[cfg(target_os = "android")]
        if let Some(path) = st.file_path.as_ref() {
            use std::io::Write;
            match std::fs::OpenOptions::new().append(true).create(true).open(path) {
                Ok(mut f) => {
                    if let Err(e) = writeln!(f, "{line}") {
                        degrade = true;
                        st.file_failed = true;
                        console_raw(&format!("{line}"));
                        console_raw(&format!(
                            "[starfish-debug] 日志文件写入失败 {e}——退化控制台输出"
                        ));
                    }
                }
                Err(e) => {
                    degrade = true;
                    st.file_failed = true;
                    console_raw(&format!("{line}"));
                    console_raw(&format!(
                        "[starfish-debug] 日志文件重开失败 {e}——退化控制台输出"
                    ));
                }
            }
        }
    } else if let Some(f) = st.file.as_mut() {
        use std::io::Write;
        if let Err(e) = writeln!(f, "{line}") {
            degrade = true;
            st.file = None;
            st.file_failed = true;
            console_raw(&format!("{line}"));
            console_raw(&format!(
                "[starfish-debug] 日志文件写入失败 {e}——本次会话退化控制台输出"
            ));
        }
    } else if st.file.is_none() && degrade_not_configured(st) {
        degrade = true;
    }
    // ② wasm 远程（批量；Error 即时上行）
    #[cfg(target_arch = "wasm32")]
    if let Some(ep) = st.remote.clone() {
        st.remote_buf.push(line.clone());
        if st.remote_buf.len() >= 16 || level == Level::Error {
            let body = st.remote_buf.join("\n");
            st.remote_buf.clear();
            web::post(&ep, &body);
        }
    }
    // ③ 控制台直出（策略阈值；或文件/远程不可用退化）
    let console_hit = st.console.is_some_and(|th| level <= th);
    if degrade || console_hit {
        console_raw(&line);
    }
    // 自耗统计（级别过滤的早退不计——只算真实产出；wasm 无计时只计数）
    st.emit_count += 1;
    #[cfg(not(target_arch = "wasm32"))]
    st_add_ms(st, t0);
}

// 自耗毫秒累计（native 专用——wasm 无 Instant，仅计数）
#[cfg(not(target_arch = "wasm32"))]
fn st_add_ms(st: &mut State, t0: std::time::Instant) {
    st.emit_ms += t0.elapsed().as_micros();
}

// 桌面/Android：文件未配置（file=None 且未失败）时视为无持久通道 → 退化
// 控制台（用户定约：没开 log 或写入失败 = 后台输出兜底）。wasm 同理由
// remote 缺省触发。
fn degrade_not_configured(st: &State) -> bool {
    if cfg!(target_arch = "wasm32") {
        st.remote.is_none()
    } else {
        st.file.is_none()
    }
}

fn st_count_inc() {
    COUNT.fetch_add(1, Ordering::SeqCst);
}
fn count_load() -> u64 {
    COUNT.load(Ordering::SeqCst)
}
static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 启动期日志自耗诊断（宿主关键节点打一行——定位日志自身开销）：
/// 自家条数/耗时 + 第三方桥接条数/耗时 + 文件状态
pub fn stats_string() -> String {
    match STATE.lock().unwrap().as_ref() {
        Some(st) => format!(
            "[starfish-debug] 日志自耗统计: 自家{}条/{:.1}ms 第三方桥{}条/{:.1}ms 级别={:?} 文件={}",
            st.emit_count, st.emit_ms as f64 / 1000.0,
            st.bridge_count, st.bridge_ms as f64 / 1000.0,
            st.level,
            if st.file_failed { "退化控制台" } else if st.file.is_some() || st.file_path.is_some() { "正常" } else { "未配置" },
        ),
        None => "[starfish-debug] 日志未初始化".into(),
    }
}

/// 内部状态快照（诊断用——宿主经已验证通道打出，定位 emit 断层）
pub fn state_string() -> String {
    let inited = INITED.load(Ordering::SeqCst);
    match STATE.lock().unwrap().as_ref() {
        Some(st) => format!(
            "[log-state] INITED={} level={:?} file={} file_failed={} 自家{}条 桥{}条",
            inited, st.level,
            if st.file.is_some() { "已开" } else { "无" },
            st.file_failed, st.emit_count, st.bridge_count,
        ),
        None => format!("[log-state] INITED={inited} STATE=None"),
    }
}

/// 控制台裸输出（平台分支：wasm = console.log；其余 = println!）
pub(crate) fn console_raw(line: &str) {
    #[cfg(target_arch = "wasm32")]
    web::console(line);
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
}

// ── 第三方 log crate 桥（wgpu/egui/rapier/rustpython…收编）────────
struct FacadeBridge;

impl log::Log for FacadeBridge {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        INITED.load(Ordering::SeqCst)
            && Level::from(meta.level()) <= STATE.lock().unwrap().as_ref().map(|s| s.level).unwrap_or(Level::Off)
    }
    fn log(&self, record: &log::Record) {
        // wasm 门控（2026-11-13 egui_app 实测定案）：此处曾是唯一漏网的
        // 裸 Instant——wgpu/naga/egui 第三方首次管线编译发 log::info! →
        // 全局桥 → std Instant wasm32 未实现 → 启动即 panic（egui_app
        // wasm 崩、纯脚本案例无第三方日志故不触发——七轮 raise 二分 +
        // wasm-tools 反汇编调用方全景锁定）
        #[cfg(not(target_arch = "wasm32"))]
        let t0 = std::time::Instant::now();
        // 第三方 Info 降位为自家 Debug（wgpu/egui 等内部进展属诊断——
        // 发行壳 Info 级别下不落盘；dev Debug 级别照常全见），
        // Warn/Error 原样透传（崩溃取证通道不动）
        let lvl = match record.level() {
            log::Level::Info => Level::Debug,
            l => Level::from(l),
        };
        __emit(
            lvl,
            format_args!("[{}] {}", record.target(), record.args()),
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut g) = STATE.try_lock() {
            if let Some(st) = g.as_mut() {
                st.bridge_count += 1;
                st.bridge_ms += t0.elapsed().as_micros();
            }
        }
    }
    fn flush(&self) {}
}

// ── 输出宏（单表达式形态——调用点保留 format! 组装；机械替换友好）──

/// 错误（恒编译；崩溃取证主通道）
#[macro_export]
macro_rules! log_error {
    ($msg:expr $(,)?) => { $crate::log::__emit($crate::log::Level::Error, $msg) };
}

/// 警告（恒编译）
#[macro_export]
macro_rules! log_warn {
    ($msg:expr $(,)?) => { $crate::log::__emit($crate::log::Level::Warn, $msg) };
}

/// 信息（恒编译——生命周期/启动报告主通道）
#[macro_export]
macro_rules! log_info {
    ($msg:expr $(,)?) => { $crate::log::__emit($crate::log::Level::Info, $msg) };
}

/// 调试（**release 构建编译剔除**——库内调试细语不进生产包）
#[macro_export]
macro_rules! log_debug {
    ($msg:expr $(,)?) => {
        if cfg!(debug_assertions) {
            $crate::log::__emit($crate::log::Level::Debug, $msg)
        }
    };
}

/// 追踪（**release 构建编译剔除**）
#[macro_export]
macro_rules! log_trace {
    ($msg:expr $(,)?) => {
        if cfg!(debug_assertions) {
            $crate::log::__emit($crate::log::Level::Trace, $msg)
        }
    };
}
