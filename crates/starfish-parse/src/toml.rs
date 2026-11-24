//! toml 解析（toml crate 直通——人写配置面：与 json 的分工 = 可读性
//! 优先的手写配置文件）。

use serde_json::Value;

/// 解析 toml 文本 → Value（统一值模型 = serde_json::Value——双格式
/// 同构，py 绑定层零分派成本）
pub fn parse(text: &str) -> Result<Value, String> {
    toml::from_str::<toml::Value>(text)
        .map(toml_to_json_value)
        .map_err(|e| format!("toml 解析失败: {e}"))
}

/// Value → toml 文本
pub fn dump(value: &Value) -> String {
    match toml::to_string_pretty(&json_value_to_toml(value)) {
        Ok(s) => s,
        Err(e) => format!("(toml 序列化失败: {e})"),
    }
}

// ── 统一值模型桥（toml::Value ↔ serde_json::Value——双层不外泄）──

fn toml_to_json_value(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json_value).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json_value(v))).collect())
        }
    }
}

fn json_value_to_toml(v: &Value) -> toml::Value {
    match v {
        Value::Null => toml::Value::String(String::new()),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(a) => toml::Value::Array(a.iter().map(json_value_to_toml).collect()),
        Value::Object(o) => {
            toml::Value::Table(o.iter().map(|(k, v)| (k.clone(), json_value_to_toml(v))).collect())
        }
    }
}
