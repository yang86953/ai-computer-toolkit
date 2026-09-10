//! 冻结 UIX 精确语义元素动作与提交后条件同步输入。

use serde::Deserialize;
use serde_json::Value;

use super::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_element_location_contract::UixElementSelector,
    uix_element_wait_contract::UixElementWaitCondition,
    uix_semantic_action_contract::{UixSemanticAction, canonical_snapshot_id},
};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存动作提交后必须匹配的 exact-AND 语义元素条件。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixElementTransitionPostcondition {
    selector: UixElementSelector,
    condition: UixElementWaitCondition,
}

impl UixElementTransitionPostcondition {
    /// 返回严格 exact-AND selector。
    pub(crate) fn selector(&self) -> &UixElementSelector {
        &self.selector
    }

    /// 返回 unique 或 missing 封闭条件。
    pub(crate) const fn condition(&self) -> UixElementWaitCondition {
        self.condition
    }
}

/// 保存 snapshot-scoped 动作与提交后语义条件的单请求契约。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixElementTransitionInput {
    snapshot_id: String,
    element_id: String,
    action: UixSemanticAction,
    postcondition: UixElementTransitionPostcondition,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixElementTransitionInput {
    /// 严格解析目标、动作、postcondition 与总 deadline，不回显敏感输入。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(selector) = value
            .get("postcondition")
            .and_then(|condition| condition.get("selector"))
            .and_then(Value::as_object)
        else {
            return Err("UIX element transition postcondition violates its bounded contract.");
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX element transition selector fields cannot be null.");
        }
        let input = serde_json::from_value::<Self>(value.clone()).map_err(
            |_| "UIX element transition input violates schema://ui/element-transition/v1.",
        )?;
        if !canonical_snapshot_id(&input.snapshot_id)
            || OpaqueTargetId::parse(&input.element_id)
                .is_none_or(|target| target.kind() != OpaqueTargetKind::Element)
            || !input.action.validate()
            || input.postcondition.selector.validate().is_err()
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
        {
            return Err("UIX element transition input is outside its bounded contract.");
        }
        Ok(input)
    }

    pub(crate) fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub(crate) fn element_id(&self) -> &str {
        &self.element_id
    }

    pub(crate) fn action(&self) -> &UixSemanticAction {
        &self.action
    }

    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid_input() -> Value {
        json!({
            "snapshotId": "as3:0123456789abcdef",
            "elementId": "s2:e:0123456789abcdef",
            "action": { "type": "invoke" },
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            }
        })
    }

    #[test]
    fn transition_reuses_exact_action_and_postcondition_defaults() {
        let Ok(input) = UixElementTransitionInput::parse(&valid_input()) else {
            panic!("有效元素 transition 必须解析");
        };
        assert_eq!(input.action().provider_action(), "invoke");
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn transition_is_closed_and_rejects_null_or_invalid_targets() {
        let mut unexpected = valid_input();
        unexpected["unexpected"] = json!(true);
        assert!(UixElementTransitionInput::parse(&unexpected).is_err());

        let mut null_selector = valid_input();
        null_selector["postcondition"]["selector"]["role"] = Value::Null;
        assert!(UixElementTransitionInput::parse(&null_selector).is_err());

        let mut stale_snapshot = valid_input();
        stale_snapshot["snapshotId"] = json!("as2:0123456789abcdef");
        assert!(UixElementTransitionInput::parse(&stale_snapshot).is_err());

        let mut wrong_element = valid_input();
        wrong_element["elementId"] = json!("s2:w:0123456789abcdef");
        assert!(UixElementTransitionInput::parse(&wrong_element).is_err());
    }

    #[test]
    fn transition_enforces_total_timeout_and_bounded_action() {
        let mut too_short = valid_input();
        too_short["timeoutMs"] = json!(99);
        assert!(UixElementTransitionInput::parse(&too_short).is_err());

        let mut too_long = valid_input();
        too_long["timeoutMs"] = json!(30_001);
        assert!(UixElementTransitionInput::parse(&too_long).is_err());

        let mut empty_select = valid_input();
        empty_select["action"] = json!({ "type": "select", "value": "" });
        assert!(UixElementTransitionInput::parse(&empty_select).is_err());
    }
}
