//! UIX 协作式语义元素定位的 provider-neutral 输入契约。

use serde::Deserialize;
use serde_json::Value;

const MAXIMUM_AUTOMATION_ID_BYTES: usize = 512;
const MAXIMUM_NAME_BYTES: usize = 256;
const MAXIMUM_ROLE_BYTES: usize = 64;
const MAXIMUM_ACTION_BYTES: usize = 64;

/// UIX 公开语义字段的严格 AND selector。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixElementSelector {
    automation_id: Option<String>,
    role: Option<String>,
    name: Option<String>,
    focused: Option<bool>,
    enabled: Option<bool>,
    action: Option<String>,
}

impl UixElementSelector {
    /// 供定位与等待 Component 共用的严格 selector 校验。
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.automation_id.is_none()
            && self.role.is_none()
            && self.name.is_none()
            && self.focused.is_none()
            && self.enabled.is_none()
            && self.action.is_none()
        {
            return Err("UIX element selector requires at least one semantic field.");
        }
        for (value, maximum) in [
            (self.automation_id.as_deref(), MAXIMUM_AUTOMATION_ID_BYTES),
            (self.role.as_deref(), MAXIMUM_ROLE_BYTES),
            (self.name.as_deref(), MAXIMUM_NAME_BYTES),
            (self.action.as_deref(), MAXIMUM_ACTION_BYTES),
        ] {
            if value.is_some_and(|value| value.is_empty() || value.len() > maximum) {
                return Err("UIX element selector strings are empty or exceed their bounds.");
            }
        }
        Ok(())
    }

    /// 对 Adapter 已脱敏的中立字段执行大小写敏感的精确 AND 匹配。
    pub(crate) fn matches(
        &self,
        automation_id: Option<&str>,
        role: &str,
        name: &str,
        focused: bool,
        enabled: bool,
        actions: &[String],
    ) -> bool {
        self.automation_id
            .as_deref()
            .is_none_or(|value| automation_id == Some(value))
            && self.role.as_deref().is_none_or(|value| role == value)
            && self.name.as_deref().is_none_or(|value| name == value)
            && self.focused.is_none_or(|value| focused == value)
            && self.enabled.is_none_or(|value| enabled == value)
            && self
                .action
                .as_deref()
                .is_none_or(|value| actions.iter().any(|action| action == value))
    }
}

/// `ui.element.locate@2` 的严格输入。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixElementLocateInput {
    selector: UixElementSelector,
}

impl UixElementLocateInput {
    /// 从完整 JSON 值解析并验证封闭 selector。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let selector = value
            .get("selector")
            .and_then(Value::as_object)
            .ok_or("UIX element location input violates its bounded contract.")?;
        if selector.values().any(Value::is_null) {
            return Err("UIX element selector fields cannot be null.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX element location input violates its bounded contract.")?;
        input.selector.validate()?;
        Ok(input)
    }

    pub(crate) fn selector(&self) -> &UixElementSelector {
        &self.selector
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn selector_is_closed_bounded_and_exact_and() {
        // 测试输入解析失败时显式中止，避免 expect 掩盖错误路径。
        let Ok(input) = UixElementLocateInput::parse(&json!({
            "selector": {
                "automationId": "save",
                "role": "button",
                "enabled": true,
                "action": "invoke"
            }
        })) else {
            panic!("valid selector must parse");
        };
        assert!(input.selector().matches(
            Some("save"),
            "button",
            "保存",
            false,
            true,
            &["focus".to_owned(), "invoke".to_owned()],
        ));
        assert!(!input.selector().matches(
            Some("save"),
            "button",
            "保存",
            false,
            false,
            &["invoke".to_owned()],
        ));
        assert!(UixElementLocateInput::parse(&json!({ "selector": {} })).is_err());
        assert!(
            UixElementLocateInput::parse(&json!({
                "selector": { "automationId": null, "enabled": true }
            }))
            .is_err()
        );
        assert!(
            UixElementLocateInput::parse(&json!({
                "selector": { "automationId": "save", "xpath": "//*" }
            }))
            .is_err()
        );
    }

    #[test]
    fn selector_string_bounds_are_utf8_byte_bounds() {
        assert!(
            UixElementLocateInput::parse(&json!({
                "selector": { "name": "界".repeat(100) }
            }))
            .is_err()
        );
    }
}
