//! 限定 Linux Portal 桌面会话可安全执行的 provider-neutral 键盘输入。

use serde_json::Value;

use crate::{
    components::keyboard_input_contract::{KeyboardInput, parse_keyboard_input},
    domain::{AppControlError, AppResult},
};

/// 解析会话级按键输入，并拒绝依赖键盘布局或输入法的文本步骤。
pub(crate) fn parse(value: &Value) -> AppResult<KeyboardInput> {
    let input = parse_keyboard_input(value)?;
    if input.steps.iter().any(|step| step.kind() == "text") {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Portal desktop-session keyboard input does not support text steps.",
        ));
    }
    Ok(input)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn balanced_named_keys_and_chords_are_accepted() {
        let input = parse(&json!({
            "timeoutMs": 5_000,
            "steps": [
                { "type": "key", "key": "left-control", "phase": "down" },
                { "type": "key", "key": "a", "phase": "press" },
                { "type": "key", "key": "left-control", "phase": "up" },
                { "type": "chord", "keys": ["left-alt", "f4"] }
            ]
        }))
        .unwrap_or_else(|error| panic!("balanced Portal input must parse: {error}"));
        assert_eq!(input.steps.len(), 4);
    }

    #[test]
    fn text_and_unbalanced_state_fail_before_adapter_access() {
        let text = parse(&json!({"steps": [{"type": "text", "text": "不可发送"}]}))
            .expect_err("text must be rejected");
        assert_eq!(text.code, "INVALID_ARGUMENT");
        let unbalanced = parse(&json!({
            "steps": [{"type": "key", "key": "left-shift", "phase": "down"}]
        }))
        .expect_err("unbalanced key ownership must be rejected");
        assert_eq!(unbalanced.code, "INVALID_ARGUMENT");
    }
}
