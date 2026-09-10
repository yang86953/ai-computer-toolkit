//! UIX Agent 修订等待的私有 wire Component。

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::components::window_revision_wait_contract::WindowRevisionWaitCondition;

use super::{
    AgentClient, Failure, RevisionWaitOutcome, RevisionWaitOutcomeKind, WireWindow,
    read_request_failure, remaining, resolve_until,
};

const WAIT_RESPONSE_GRACE: Duration = Duration::from_millis(50);

#[derive(Debug, Deserialize)]
struct WireWait {
    outcome: String,
    window: WireWindow,
}

/// 在一个总 deadline 和同一认证连接内等待精确窗口代际关闭。
pub(crate) fn wait_closed(target: &str, timeout_ms: u32) -> Result<RevisionWaitOutcome, Failure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    let window = resolve_until(target, deadline)?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)?;
    client.wait_closed(
        window.window_id,
        window.generation,
        window.revision,
        window.presented_revision,
    )
}

impl AgentClient {
    /// 跨过普通修订变化，直到 Agent 证明同一 generation 已关闭。
    pub(super) fn wait_closed(
        &mut self,
        window_id: u64,
        generation: u64,
        mut baseline_revision: u64,
        mut baseline_presented_revision: u64,
    ) -> Result<RevisionWaitOutcome, Failure> {
        loop {
            let outcome = self.wait_revision(
                window_id,
                generation,
                baseline_revision,
                baseline_presented_revision,
                WindowRevisionWaitCondition::RevisionAfter {
                    revision: baseline_revision,
                },
            )?;
            match outcome.kind {
                RevisionWaitOutcomeKind::Closed => return Ok(outcome),
                RevisionWaitOutcomeKind::Changed => {
                    baseline_revision = outcome.revision;
                    baseline_presented_revision = outcome.presented_revision;
                }
                RevisionWaitOutcomeKind::Presented => return Err(Failure::Protocol),
            }
        }
    }

    /// 在同一认证连接上执行有界 revision wait。
    pub(super) fn wait_revision(
        &mut self,
        window_id: u64,
        generation: u64,
        baseline_revision: u64,
        baseline_presented_revision: u64,
        condition: WindowRevisionWaitCondition,
    ) -> Result<RevisionWaitOutcome, Failure> {
        if !self.request_types.contains("wait") {
            return Err(Failure::Unavailable);
        }
        let provider_timeout = remaining(self.deadline)?
            .checked_sub(WAIT_RESPONSE_GRACE)
            .ok_or(Failure::Timeout)?;
        let timeout_ms =
            u64::try_from(provider_timeout.as_millis()).map_err(|_| Failure::Protocol)?;
        if timeout_ms == 0 {
            return Err(Failure::Timeout);
        }
        let mut payload = json!({
            "window_id": window_id,
            "generation": generation,
            "timeout_ms": timeout_ms,
        });
        let (field, threshold) = match condition {
            WindowRevisionWaitCondition::RevisionAfter { revision } => ("after_revision", revision),
            WindowRevisionWaitCondition::PresentedAtLeast { revision } => {
                ("presented_revision", revision)
            }
        };
        payload
            .as_object_mut()
            .unwrap_or_else(|| unreachable!("wait payload is always an object"))
            .insert(field.to_owned(), json!(threshold));
        let reply = self
            .request("act-wait", "wait", payload)
            .map_err(read_request_failure)?;
        wait_outcome(
            reply,
            window_id,
            generation,
            baseline_revision,
            baseline_presented_revision,
            condition,
        )
    }
}

/// 验证 Agent 等待终态与请求条件一致。
pub(super) fn wait_outcome(
    reply: Value,
    window_id: u64,
    generation: u64,
    baseline_revision: u64,
    baseline_presented_revision: u64,
    condition: WindowRevisionWaitCondition,
) -> Result<RevisionWaitOutcome, Failure> {
    let waited = serde_json::from_value::<WireWait>(reply).map_err(|_| Failure::Protocol)?;
    let window = waited.window;
    if window.window_id != window_id
        || window.generation != generation
        || window.revision < baseline_revision
        || window.presented_revision < baseline_presented_revision
        || window.presented_revision > window.revision
    {
        return Err(Failure::Protocol);
    }
    let kind = match (waited.outcome.as_str(), condition) {
        ("changed", WindowRevisionWaitCondition::RevisionAfter { revision })
            if !window.closed && window.revision > revision =>
        {
            RevisionWaitOutcomeKind::Changed
        }
        ("presented", WindowRevisionWaitCondition::PresentedAtLeast { revision })
            if !window.closed && window.presented_revision >= revision =>
        {
            RevisionWaitOutcomeKind::Presented
        }
        ("closed", _) if window.closed => RevisionWaitOutcomeKind::Closed,
        _ => return Err(Failure::Protocol),
    };
    Ok(RevisionWaitOutcome {
        kind,
        revision: window.revision,
        presented_revision: window.presented_revision,
        closed: window.closed,
    })
}
