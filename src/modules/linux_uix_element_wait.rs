//! 协调 UIX 语义元素 revision wait，并投影只读的脱敏元素事实。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, ElementWaitOutcome, NodeRecord, RectRecord,
    },
    capabilities,
    components::uix_element_wait_contract::UixElementWaitInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixElementWaitPort {
    fn wait(
        &self,
        session_id: &str,
        input: &UixElementWaitInput,
    ) -> Result<ElementWaitOutcome, ElementWaitFailure>;
}

struct SystemUixElementWait;

impl UixElementWaitPort for SystemUixElementWait {
    fn wait(
        &self,
        session_id: &str,
        input: &UixElementWaitInput,
    ) -> Result<ElementWaitOutcome, ElementWaitFailure> {
        uix_agent::perform_element_wait(session_id, input)
    }
}

/// 在同一认证连接使用 Agent revision-after wait，且不执行任何输入或前景操作。
pub(crate) fn wait(session_id: &str, value: &Value) -> AppResult<Value> {
    wait_with(&SystemUixElementWait, session_id, value)
}

fn wait_with(port: &impl UixElementWaitPort, session_id: &str, value: &Value) -> AppResult<Value> {
    let input = UixElementWaitInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port.wait(session_id, &input).map_err(wait_error)?;
    let snapshot_id = uix_window::snapshot_id(
        &outcome.snapshot.session_id,
        outcome.snapshot.revision,
        outcome.snapshot.presented_revision,
    );
    let element = outcome
        .element
        .as_ref()
        .map(|node| public_element(node, &snapshot_id));
    let match_state = if element.is_some() {
        "unique"
    } else {
        "missing"
    };
    let revision_wait_used = outcome.wait_count > 0;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/ui-element-wait/v2",
        "capability": capabilities::UI_ELEMENT_WAIT_V2,
        "targetId": outcome.snapshot.session_id,
        "snapshotId": snapshot_id,
        "condition": input.condition().as_str(),
        "selectorSemantics": "exact-and",
        "matchState": match_state,
        "matchCount": outcome.match_count,
        "sampleCount": outcome.sample_count,
        "waitCount": outcome.wait_count,
        "revision": outcome.snapshot.revision,
        "presentedRevision": outcome.snapshot.presented_revision,
        "revisionWaitUsed": revision_wait_used,
        "targetReResolved": true,
        "sameAuthenticatedConnectionUsed": true,
        "element": element,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "retrySafe": true,
        "safety": {
            "provider": "uix-agent-v1",
            "providerCache": "disabled",
            "sameAuthenticatedConnectionUsed": true,
            "revisionWaitUsed": revision_wait_used,
            "providerPolling": false,
            "stabilitySemantics": false,
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "sensitiveSnapshotPublished": false,
            "hostCoordinateMappingPublished": false,
            "clickPointInferred": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "desktopPointerMoved": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

pub(crate) fn public_element(node: &NodeRecord, snapshot_id: &str) -> Value {
    json!({
        "elementId": uix_window::public_node_id(snapshot_id, &node.native_id),
        "identityFreshness": "snapshot-revision",
        "automationId": node.automation_id,
        "role": node.role,
        "name": node.name,
        "focused": node.focused,
        "enabled": node.enabled,
        "actions": node.actions,
        "geometry": {
            "coordinateSpace": "application-client",
            "unit": "logical-px",
            "frame": public_rect(node.frame),
            "visibleBounds": node.visible_bounds.map(public_rect),
        },
    })
}

fn public_rect(rect: RectRecord) -> Value {
    json!({
        "x": rect.x,
        "y": rect.y,
        "width": rect.width,
        "height": rect.height,
    })
}

fn wait_error(failure: ElementWaitFailure) -> AppControlError {
    match failure {
        ElementWaitFailure::Ambiguous { match_count } => AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX semantic selector matched more than one current element.",
            uix_agent::ambiguous_details(match_count),
        ),
        ElementWaitFailure::Transport(source) => uix_window::public_error(source),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::adapters::linux::uix_agent::{Failure, Snapshot};

    use super::*;

    #[derive(Clone, Copy)]
    enum FixtureKind {
        Unique,
        Missing,
        Ambiguous,
    }

    struct FixturePort {
        kind: FixtureKind,
    }

    impl UixElementWaitPort for FixturePort {
        fn wait(
            &self,
            _: &str,
            _: &UixElementWaitInput,
        ) -> Result<ElementWaitOutcome, ElementWaitFailure> {
            if matches!(self.kind, FixtureKind::Ambiguous) {
                return Err(ElementWaitFailure::Ambiguous { match_count: 2 });
            }
            let (element, match_count) = match self.kind {
                FixtureKind::Unique => (Some(node("7:1", Some("save"))), 1),
                FixtureKind::Missing => (None, 0),
                FixtureKind::Ambiguous => (None, 2),
            };
            Ok(ElementWaitOutcome {
                snapshot: Snapshot {
                    session_id: "s2:w:0123456789abcdef".to_owned(),
                    revision: 8,
                    presented_revision: 7,
                    nodes: Vec::new(),
                },
                element,
                match_count,
                sample_count: 2,
                wait_count: 1,
            })
        }
    }

    fn input(condition: &str) -> Value {
        json!({
            "selector": { "automationId": "save" },
            "condition": condition,
            "timeoutMs": 500
        })
    }

    fn node(id: &str, automation_id: Option<&str>) -> NodeRecord {
        NodeRecord {
            native_id: id.to_owned(),
            parent_native_id: None,
            automation_id: automation_id.map(str::to_owned),
            focused: false,
            role: "button".to_owned(),
            name: "保存".to_owned(),
            enabled: true,
            actions: vec!["invoke".to_owned()],
            frame: RectRecord {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 30.0,
            },
            visible_bounds: None,
        }
    }

    #[test]
    fn unique_projects_snapshot_scoped_element_without_native_identity() {
        let Ok(result) = wait_with(
            &FixturePort {
                kind: FixtureKind::Unique,
            },
            "s2:w:0123456789abcdef",
            &input("unique"),
        ) else {
            panic!("唯一元素等待必须投影成功");
        };
        assert_eq!(result["capability"], capabilities::UI_ELEMENT_WAIT_V2);
        assert_eq!(result["condition"], "unique");
        assert_eq!(result["selectorSemantics"], "exact-and");
        assert_eq!(result["matchState"], "unique");
        assert_eq!(result["matchCount"], 1);
        assert_eq!(result["sampleCount"], 2);
        assert_eq!(result["waitCount"], 1);
        assert_eq!(result["revisionWaitUsed"], true);
        assert_eq!(result["sameAuthenticatedConnectionUsed"], true);
        assert_eq!(result["element"]["geometry"]["unit"], "logical-px");
        assert_eq!(result["safety"]["nativeIdentityExposed"], false);
        assert_eq!(result["safety"]["fallback"], "none");
        assert!(
            result["element"]["elementId"]
                .as_str()
                .is_some_and(|id| id.starts_with("s2:e:"))
        );

        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "read",
            "capability": capabilities::UI_ELEMENT_WAIT_V2,
            "targetId": "s2:w:0123456789abcdef",
            "executionRealm": "same-session-no-focus",
            "requiredExecutionRealm": "same-session-no-focus",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": { "activationRequested": false },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v2/ui-element-wait.schema.json"
        )) else {
            panic!("元素等待结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "Module 实际结果必须匹配公开 schema"
        );
    }

    #[test]
    fn missing_is_success_with_zero_match_and_null_element() {
        let Ok(result) = wait_with(
            &FixturePort {
                kind: FixtureKind::Missing,
            },
            "s2:w:0123456789abcdef",
            &input("missing"),
        ) else {
            panic!("缺失元素等待必须投影成功");
        };
        assert_eq!(result["matchState"], "missing");
        assert_eq!(result["matchCount"], 0);
        assert_eq!(result["element"], Value::Null);
        assert_eq!(result["readOnly"], true);
        assert_eq!(result["retrySafe"], true);
    }

    #[test]
    fn initial_snapshot_success_does_not_claim_revision_wait_use() {
        struct ImmediatePort;
        impl UixElementWaitPort for ImmediatePort {
            fn wait(
                &self,
                _: &str,
                _: &UixElementWaitInput,
            ) -> Result<ElementWaitOutcome, ElementWaitFailure> {
                Ok(ElementWaitOutcome {
                    snapshot: Snapshot {
                        session_id: "s2:w:0123456789abcdef".to_owned(),
                        revision: 8,
                        presented_revision: 7,
                        nodes: Vec::new(),
                    },
                    element: Some(node("7:1", Some("save"))),
                    match_count: 1,
                    sample_count: 1,
                    wait_count: 0,
                })
            }
        }

        let Ok(result) = wait_with(&ImmediatePort, "s2:w:0123456789abcdef", &input("unique"))
        else {
            panic!("初始 snapshot 满足条件时必须直接成功");
        };
        assert_eq!(result["revisionWaitUsed"], false);
        assert_eq!(result["safety"]["revisionWaitUsed"], false);
    }

    #[test]
    fn ambiguous_maps_to_element_semantics_not_window_target_text() {
        let Err(error) = wait_with(
            &FixturePort {
                kind: FixtureKind::Ambiguous,
            },
            "s2:w:0123456789abcdef",
            &input("unique"),
        ) else {
            panic!("多匹配必须拒绝");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(
            error.message,
            "The UIX semantic selector matched more than one current element."
        );
        assert_eq!(error.details["targetKind"], "element");
        assert_eq!(error.details["matchCount"], 2);
    }

    #[test]
    fn transport_stale_error_does_not_invent_element_observation() {
        let error = wait_error(ElementWaitFailure::Transport(Failure::Stale));
        assert_eq!(error.code, "STALE_SESSION");
        assert_eq!(error.details["fallback"], "none");
    }
}
