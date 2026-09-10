//! 任务控制 v2 的封闭输入契约；授权来源不属于普通请求帧。

use crate::service::SequencePostcondition;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub(super) const CONTRACT: &str = "act/control/v2";
pub(super) const GRANT_CONTRACT: &str = "act/task-grant/v1";
pub(super) const MAX_FRAME_BYTES: usize = 64 * 1024;
pub(super) const MAX_REQUESTS: usize = 256;
pub(super) const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub(super) const MAX_PENDING: usize = 16;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TaskGrant {
    pub contract_version: String,
    pub task_id: String,
    pub authorization_source: AuthorizationSource,
    pub total_timeout_ms: u64,
    pub max_executions: usize,
    #[serde(default)]
    pub allow_discovery: bool,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum AuthorizationSource {
    UserTask,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Permission {
    pub capability: String,
    pub target_id: String,
    /// 未指定的输入仍受 capability 的封闭 schema 约束；此处仅收紧任务范围。
    #[serde(default)]
    pub required_input: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    pub contract_version: String,
    pub request_id: String,
    pub task_id: String,
    pub operation: Operation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum Operation {
    #[serde(rename = "task.open")]
    Open,
    #[serde(rename = "task.status")]
    Status,
    #[serde(rename = "task.cancel")]
    Cancel,
    #[serde(rename = "task.close")]
    Close,
    Catalog,
    Discover {
        #[serde(default)]
        scope: DiscoveryScope,
    },
    Assess {
        capability: String,
        #[serde(rename = "targetId")]
        target_id: String,
    },
    Execute {
        capability: String,
        #[serde(rename = "targetId")]
        target_id: String,
        input: Map<String, Value>,
        #[serde(default)]
        postconditions: Vec<SequencePostcondition>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DiscoveryScope {
    #[default]
    Applications,
    Media,
}

pub(super) fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

pub(super) fn valid_task_id(value: &str) -> bool {
    value.strip_prefix("t2:").is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
