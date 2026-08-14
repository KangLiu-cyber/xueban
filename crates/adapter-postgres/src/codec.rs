//! 事件载荷编解码：领域层 payload 为 JSON 文本（Option<String>），
//! 存储层为 jsonb（Option<Value>）。纯文本往返，不解释内容。

use domain::error::{Error, Result};

/// 领域 JSON 文本 → jsonb 值（入库方向）。
pub fn payload_to_value(payload: &Option<String>) -> Result<Option<serde_json::Value>> {
    match payload {
        None => Ok(None),
        Some(s) => serde_json::from_str(s)
            .map(Some)
            .map_err(|e| Error::Storage(format!("事件载荷不是合法 JSON: {e}"))),
    }
}

/// jsonb 值 → 领域 JSON 文本（出库方向）。
pub fn value_to_payload(value: Option<serde_json::Value>) -> Result<Option<String>> {
    value
        .map(|v| serde_json::to_string(&v))
        .transpose()
        .map_err(|e| Error::Storage(format!("事件载荷序列化失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_round_trips() {
        let payload = Some(r#"{"annotation_id":1,"author":"user"}"#.to_owned());
        let value = payload_to_value(&payload).unwrap();
        assert_eq!(value, Some(json!({"annotation_id": 1, "author": "user"})));
        let back = value_to_payload(value).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn none_stays_none() {
        assert_eq!(payload_to_value(&None).unwrap(), None);
        assert_eq!(value_to_payload(None).unwrap(), None);
    }

    #[test]
    fn invalid_json_is_storage_error() {
        assert!(payload_to_value(&Some("not json".to_owned())).is_err());
    }
}
