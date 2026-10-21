//! Error types (platform-independent).
//! 错误类型（平台无关）。

use std::fmt;

/// A window operation failure.
/// 窗口操作失败。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowError {
    /// A platform API call failed; carries the underlying error description.
    /// 平台 API 调用失败，附带底层错误描述。
    OsError(String),
    /// A [`Window`](crate::Window) is already alive in this process
    /// (single-window invariant).
    /// 进程内已存在一个存活的 [`Window`](crate::Window)（单窗口约束）。
    AlreadyExists,
    /// The backend for this platform is not implemented yet (stub).
    /// 该平台的后端尚未实现（占位）。
    UnsupportedPlatform,
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowError::OsError(msg) => write!(f, "platform window call failed: {msg}"),
            WindowError::AlreadyExists => {
                write!(f, "a window already exists (single-window invariant)")
            }
            WindowError::UnsupportedPlatform => {
                write!(f, "backend not implemented for this platform")
            }
        }
    }
}

impl std::error::Error for WindowError {}
