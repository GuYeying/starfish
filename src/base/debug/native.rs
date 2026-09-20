//! debug 的原生实现：stdout 输出（panic 天然走 stderr，无需转发）

/// stdout 直写（桌面终端 / Android logcat 的 `RustStdoutStderr` tag）
pub fn log(msg: &str) {
    println!("{msg}");
}
