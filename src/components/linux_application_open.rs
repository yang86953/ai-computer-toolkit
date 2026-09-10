//! Linux application.open@2 的 provider-neutral 输入 Component。

use serde_json::Value;

use crate::domain::{AppControlError, AppResult};

/// 冻结首批无公共参数输入，拒绝 path、argv、URI、环境或 shell 字段。
pub(crate) fn validate(input: Option<&Value>) -> AppResult<()> {
    if input
        .and_then(Value::as_object)
        .is_some_and(serde_json::Map::is_empty)
    {
        return Ok(());
    }
    Err(AppControlError::new(
        "INVALID_ARGUMENT",
        "application.open@2 requires an empty input object.",
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate;

    #[test]
    fn input_is_exactly_an_empty_object() {
        assert!(validate(Some(&json!({}))).is_ok());
        for rejected in [
            json!(null),
            json!([]),
            json!({"argv": []}),
            json!({"path": "/bin/true"}),
        ] {
            assert_eq!(
                validate(Some(&rejected))
                    .err()
                    .unwrap_or_else(|| panic!("非空或非对象输入必须失败"))
                    .code,
                "INVALID_ARGUMENT"
            );
        }
        assert_eq!(
            validate(None)
                .err()
                .unwrap_or_else(|| panic!("缺失输入必须失败"))
                .code,
            "INVALID_ARGUMENT"
        );
    }
}
