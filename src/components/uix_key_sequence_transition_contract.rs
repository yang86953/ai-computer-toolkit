//! UIX 应用内按键序列及提交后语义条件的版本一输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_key_input_contract::UixKeyPress, uix_key_sequence_contract::UixKeySequenceInput,
};

/// 保存严格验证后的应用内按键序列 transition 请求。
#[derive(Clone, Debug)]
pub(crate) struct UixKeySequenceTransitionInput {
    sequence: UixKeySequenceInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixKeySequenceTransitionInput {
    /// 严格解析封闭输入，并把按键及其时序校验委托给既有 Component。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX key sequence transition input must be an object.");
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "presses" | "intervalMs" | "timeoutMs" | "postcondition"
            )
        }) || contains_null(value)
        {
            return Err("UIX key sequence transition input violates its closed contract.");
        }

        let Some(postcondition_value) = object.get("postcondition") else {
            return Err("UIX key sequence transition requires a postcondition.");
        };
        let Some(postcondition_object) = postcondition_value.as_object() else {
            return Err("UIX key sequence transition postcondition must be an object.");
        };
        let Some(selector) = postcondition_object
            .get("selector")
            .and_then(Value::as_object)
        else {
            return Err("UIX key sequence transition requires a selector object.");
        };
        if selector.is_empty() {
            return Err("UIX key sequence transition selector cannot be empty.");
        }
        let postcondition = serde_json::from_value::<UixElementTransitionPostcondition>(
            postcondition_value.clone(),
        )
        .map_err(|_| "UIX key sequence transition postcondition violates its contract.")?;
        postcondition.selector().validate()?;

        let mut sequence_object = object.clone();
        sequence_object.remove("postcondition");
        let sequence = UixKeySequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            postcondition,
        })
    }

    /// 返回严格验证后的基础 key sequence 输入。
    pub(crate) fn sequence(&self) -> &UixKeySequenceInput {
        &self.sequence
    }

    /// 返回提交后必须满足的 exact-AND 语义条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回 provider-neutral 的完整按键顺序。
    pub(crate) fn presses(&self) -> &[UixKeyPress] {
        self.sequence.presses()
    }

    /// 返回请求中的按键数量。
    pub(crate) fn presses_requested(&self) -> usize {
        self.sequence.presses_requested()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "key-sequence"
    }

    /// 返回相邻按键之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.sequence.interval_ms()
    }

    /// 返回序列计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        self.sequence.planned_duration_ms()
    }

    /// 返回覆盖序列与后置观测的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.sequence.timeout_ms()
    }
}

/// 递归拒绝输入任何位置的显式 null，避免可选字段绕过闭合契约。
fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.iter().any(contains_null),
        Value::Object(object) => object.values().any(contains_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input(condition: &str) -> Value {
        json!({
            "presses": [
                { "key": "control" },
                { "key": "s", "modifiers": ["control"] }
            ],
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": condition
            }
        })
    }

    #[test]
    fn transition_reuses_sequence_and_exact_postcondition() {
        let Ok(input) = UixKeySequenceTransitionInput::parse(&valid_input("unique")) else {
            panic!("有效 key sequence transition 必须解析");
        };
        assert_eq!(input.action(), "key-sequence");
        assert_eq!(input.presses_requested(), 2);
        assert_eq!(input.presses()[1].key(), "s");
        assert_eq!(input.presses()[1].provider_key(), "s");
        assert_eq!(input.interval_ms(), 0);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), 30_000);
        assert_eq!(input.postcondition().condition().as_str(), "unique");
    }

    #[test]
    fn transition_accepts_missing_and_bounded_timing() {
        let mut value = valid_input("missing");
        value["intervalMs"] = json!(100);
        value["timeoutMs"] = json!(300);
        let Ok(input) = UixKeySequenceTransitionInput::parse(&value) else {
            panic!("有界 missing transition 必须解析");
        };
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.interval_ms(), 100);
        assert_eq!(input.planned_duration_ms(), 100);
        assert_eq!(input.timeout_ms(), 300);
    }

    #[test]
    fn transition_rejects_unknown_null_invalid_press_and_postcondition() {
        let mut unknown = valid_input("unique");
        unknown["unexpected"] = json!(true);
        assert!(UixKeySequenceTransitionInput::parse(&unknown).is_err());

        let mut nested_null = valid_input("unique");
        nested_null["presses"][0]["key"] = Value::Null;
        assert!(UixKeySequenceTransitionInput::parse(&nested_null).is_err());

        let mut invalid_key = valid_input("unique");
        invalid_key["presses"][0]["key"] = json!("f13");
        assert!(UixKeySequenceTransitionInput::parse(&invalid_key).is_err());

        let mut invalid_condition = valid_input("unique");
        invalid_condition["postcondition"]["condition"] = json!("stable");
        assert!(UixKeySequenceTransitionInput::parse(&invalid_condition).is_err());

        let mut empty_selector = valid_input("unique");
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixKeySequenceTransitionInput::parse(&empty_selector).is_err());

        let mut too_short = valid_input("unique");
        too_short["intervalMs"] = json!(100);
        too_short["timeoutMs"] = json!(199);
        assert!(UixKeySequenceTransitionInput::parse(&too_short).is_err());
    }

    #[test]
    fn transition_rejects_empty_sequence_and_selector_null() {
        let mut empty = valid_input("unique");
        empty["presses"] = json!([]);
        assert!(UixKeySequenceTransitionInput::parse(&empty).is_err());

        let mut null_selector = valid_input("unique");
        null_selector["postcondition"]["selector"]["role"] = Value::Null;
        assert!(UixKeySequenceTransitionInput::parse(&null_selector).is_err());
    }
}
