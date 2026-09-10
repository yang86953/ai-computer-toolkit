//! Linux `process` surface Adapter。

use serde_json::{Value, json};

use crate::{
    adapters::{
        AppAdapter,
        linux::{process_termination_pidfd, procfs},
    },
    capabilities,
    components::opaque_id::{OpaqueTargetMatch, match_opaque_target},
    domain::{AppControlError, AppResult, CommandRequest},
    modules::process_termination,
};

/// `/proc` 观察与受保护 pidfd mutation 的无状态 Adapter。
pub struct ProcessAdapter;

pub(crate) fn public_process_observation(process: &procfs::ProcessRecord) -> Value {
    json!({
        "sessionId": process.session_id,
        "targetKind": "running-process",
        "processName": process.process_name,
        "state": "running",
        "identityFreshness": if process.identity_reliable {
            "process-lifetime"
        } else {
            "best-effort-current-snapshot"
        },
        "metadataAccess": process.metadata_access,
        "integrityRelation": "unknown",
        "hasVisibleWindow": false,
        "windowVisibility": "no-visible-titled-window",
        "foregroundRequiredForObservation": false,
        "windowSessionIds": [],
    })
}

fn io_error(_: std::io::Error) -> AppControlError {
    AppControlError::with_details(
        "PROCESS_SNAPSHOT_FAILED",
        "The Linux process snapshot could not be read.",
        json!({
            "platform": "linux",
            "provider": "procfs",
            "providerState": "unavailable",
            "executionRealm": "none",
            "fallback": "none",
        }),
    )
}

impl AppAdapter for ProcessAdapter {
    fn app_id(&self) -> &'static str {
        "process"
    }

    fn status(&self) -> AppResult<Value> {
        let probe = procfs::snapshot(1).map_err(io_error)?;
        let termination_supported = process_termination_pidfd::runtime_supported();
        let mut available_capabilities = vec![
            capabilities::PROCESS_DISCOVER,
            capabilities::PROCESS_METADATA_READ,
        ];
        if termination_supported {
            available_capabilities.push(capabilities::PROCESS_TERMINATE_GRACEFUL_V2);
            available_capabilities.push(capabilities::PROCESS_TERMINATE_FORCE_V2);
        }
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "backend": "Linux procfs process snapshot",
            "platform": "linux",
            "backgroundPolicy": "guaranteed",
            "readOnly": !termination_supported,
            "probeComplete": probe.complete,
            "terminationProviderState": if termination_supported {
                "available-procfs-owner-generation-bound-same-non-root-uid-pidfd"
            } else {
                "unavailable"
            },
            "capabilities": available_capabilities,
        }))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        if request.target.contains_key("processId") {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "processId is private; filter by name or inspect an opaque sessionId.",
            ));
        }
        let mut inventory = procfs::snapshot(usize::MAX).map_err(io_error)?;
        if let Some(name) = request.target.get("name").and_then(Value::as_str) {
            inventory
                .records
                .retain(|process| process.process_name == name);
        }
        let total = inventory.records.len();
        inventory.records.truncate(request.max_items);
        let sessions = inventory
            .records
            .iter()
            .map(public_process_observation)
            .collect::<Vec<_>>();
        Ok(json!({
            "capability": capabilities::PROCESS_DISCOVER,
            "readOnly": true,
            "executionDomain": "host-headless",
            "count": sessions.len(),
            "total": total,
            "truncated": total > sessions.len(),
            // 当前没有 Linux desktop 关系来源，完整性必须保持 false。
            "complete": false,
            "foregroundUnchanged": true,
            "sessions": sessions,
        }))
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        let session_id = request
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required.")
            })?;
        let inventory = procfs::snapshot(usize::MAX).map_err(io_error)?;
        let process = match match_opaque_target(session_id, &inventory.records, |process| {
            Some(procfs::opaque_process_session_id(process))
        }) {
            OpaqueTargetMatch::Unique(process) => process,
            OpaqueTargetMatch::Missing => {
                return Err(AppControlError::new(
                    "STALE_SESSION",
                    "The running process target no longer exists.",
                ));
            }
            OpaqueTargetMatch::Ambiguous => {
                return Err(AppControlError::new(
                    "AMBIGUOUS_TARGET",
                    "The opaque process session matched more than one process.",
                ));
            }
        };
        Ok(json!({
            "capability": capabilities::PROCESS_METADATA_READ,
            "readOnly": true,
            "executionDomain": "host-headless",
            "foregroundUnchanged": true,
            "process": public_process_observation(process),
        }))
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        match request.operation.as_deref() {
            Some("terminate-graceful") => process_termination::perform(
                request.target.get("sessionId").and_then(Value::as_str),
                request.confirmed,
                request.args.get("input"),
            ),
            Some("terminate-force") => process_termination::perform_force(
                request.target.get("sessionId").and_then(Value::as_str),
                request.confirmed,
                request.args.get("input"),
            ),
            _ => Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The Linux process provider only supports explicit versioned graceful or force termination operations.",
                json!({
                    "platform": "linux",
                    "surface": "process",
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::io_error;
    use crate::domain::error_json;

    #[test]
    fn adapter_snapshot_failure_matches_public_error_contract() {
        let instance = error_json(&io_error(std::io::Error::other("fixture")));
        let schema = serde_json::from_str(include_str!(
            "../../../contracts/v1/error-envelope.schema.json"
        ))
        .expect("schema must parse");
        jsonschema::draft202012::validate(&schema, &instance)
            .expect("adapter snapshot error must validate");
        assert_eq!(instance["error"]["code"], "PROCESS_SNAPSHOT_FAILED");
    }
}
