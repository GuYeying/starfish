



// 放在文件顶部或单独mod
use std::fmt;

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

// 支持 &str 直接转
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}
