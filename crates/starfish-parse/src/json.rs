//! json 解析（serde_json 直通——结构化数据的存档/配置面）。
//!
//! 值模型 = serde_json::Value（py 侧映射 dict/list/str/num/bool/None）。

use serde_json::Value;

/// 解析 json 文本 → Value（解析失败 = Err 带行列定位）
pub fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("json 解析失败: {e}"))
}

/// Value → json 文本（紧凑形态——存档写盘默认）
pub fn dump(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

/// Value → json 文本（美化缩进——人读配置）
pub fn dump_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into())
}

/// py 面辅助：Value → py 值树的桥接由 pygamest 绑定层完成（Value 的
/// 对象/数组/标量分派映射 dict/list/str/f64/bool/None）
pub const VALUE_MODEL_DOC: &str = "serde_json::Value = dict/list/str/f64/bool/None";
