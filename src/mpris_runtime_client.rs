//! Linux MPRIS runtime client 的窄组合边界。
//!
//! 生产路径只解析当前用户总线的内部 runtime identity，然后把严格请求交给既有
//! self-worker；本模块不读取播放器、不访问环境，也不参与公开 capability 注册。

#[cfg(feature = "linux-mpris-candidate")]
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    adapters::linux::{
        mpris::Failure,
        mpris_runtime::{self, MprisRuntimeIdentity},
    },
    components::{
        linux_media_control_worker_contract as control_contract,
        linux_media_observation_worker_contract as observation_contract,
        linux_media_worker_contract_common as common_contract,
    },
    domain::{AppControlError, AppResult},
    modules::media_session,
};

/// 单次 runtime client 调用的最大原始 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = common_contract::MAXIMUM_TIMEOUT_MS;
/// 单次 worker 请求的最小数量上限。
const MINIMUM_MAXIMUM_ITEMS: u32 = common_contract::MINIMUM_MAXIMUM_ITEMS;
/// 单次 worker 请求的最大数量上限。
const MAXIMUM_MAXIMUM_ITEMS: u32 = common_contract::MAXIMUM_MAXIMUM_ITEMS;

/// 只记录调用开始时的总 deadline，resolver 与 worker 都只能消费这份预算。
struct Deadline(Instant);

impl Deadline {
    fn new(timeout_ms: u32) -> Result<Self, Failure> {
        if !(1..=MAXIMUM_TIMEOUT_MS).contains(&timeout_ms) {
            return Err(Failure::Protocol);
        }
        Ok(Self(
            Instant::now() + Duration::from_millis(u64::from(timeout_ms)),
        ))
    }

    fn remaining_ms(&self) -> Result<u32, Failure> {
        let remaining = self
            .0
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(Failure::Timeout)?;
        u32::try_from(remaining.as_millis())
            .ok()
            .filter(|milliseconds| *milliseconds > 0)
            .ok_or(Failure::Timeout)
    }
}

/// 在解析当前用户 runtime 后运行只读 discovery self-worker。
pub fn discover_current(maximum_items: u32, timeout_ms: u32) -> AppResult<Value> {
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity =
        mpris_runtime::resolve_current(deadline.remaining_ms().map_err(map_runtime_failure)?)
            .map_err(map_runtime_failure)?;
    run_observation(
        &identity,
        "discover",
        None,
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_candidate_client::run_current_image_process(
                request,
                timeout,
                crate::components::cancellation::is_cancelled,
            )
        },
    )
}

/// 在解析当前用户 runtime 后运行单目标只读 state self-worker。
pub fn state_current(target_id: &str, maximum_items: u32, timeout_ms: u32) -> AppResult<Value> {
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity =
        mpris_runtime::resolve_current(deadline.remaining_ms().map_err(map_runtime_failure)?)
            .map_err(map_runtime_failure)?;
    run_observation(
        &identity,
        "state",
        Some(target_id),
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_candidate_client::run_current_image_process(
                request,
                timeout,
                crate::components::cancellation::is_cancelled,
            )
        },
    )
}

/// 未确认的 control 在所有 deadline、endpoint、GUID 与 worker 操作之前结束。
pub fn control_current(
    target_id: &str,
    operation: &str,
    confirmed: bool,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    if !confirmed {
        return Ok(media_session::pre_dispatch_rejected(target_id, operation));
    }
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity =
        mpris_runtime::resolve_current(deadline.remaining_ms().map_err(map_runtime_failure)?)
            .map_err(map_runtime_failure)?;
    run_control(
        &identity,
        target_id,
        operation,
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_control_candidate_client::run_current_image_process(
                request,
                timeout,
                crate::components::cancellation::is_cancelled,
            )
        },
    )
}

/// 仅供私有 D-Bus fixture：使用显式绝对映像与私有地址运行 discovery worker。
#[doc(hidden)]
#[cfg(feature = "linux-mpris-candidate")]
pub fn discover_private(
    executable: &Path,
    address: &str,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    validate_fixture_executable(executable)?;
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity = mpris_runtime::resolve_private(
        address,
        deadline.remaining_ms().map_err(map_runtime_failure)?,
    )
    .map_err(map_runtime_failure)?;
    run_observation(
        &identity,
        "discover",
        None,
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_candidate_client::run_fixed_image_process(
                executable,
                request,
                timeout,
                || false,
            )
        },
    )
}

/// 仅供私有 D-Bus fixture：使用显式绝对映像与私有地址运行 state worker。
#[doc(hidden)]
#[cfg(feature = "linux-mpris-candidate")]
pub fn state_private(
    executable: &Path,
    address: &str,
    target_id: &str,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    validate_fixture_executable(executable)?;
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity = mpris_runtime::resolve_private(
        address,
        deadline.remaining_ms().map_err(map_runtime_failure)?,
    )
    .map_err(map_runtime_failure)?;
    run_observation(
        &identity,
        "state",
        Some(target_id),
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_candidate_client::run_fixed_image_process(
                executable,
                request,
                timeout,
                || false,
            )
        },
    )
}

/// 仅供私有 D-Bus fixture：确认后使用显式绝对映像与私有地址运行 control worker。
#[doc(hidden)]
#[cfg(feature = "linux-mpris-candidate")]
pub fn control_private(
    executable: &Path,
    address: &str,
    target_id: &str,
    operation: &str,
    confirmed: bool,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    if !confirmed {
        return Ok(media_session::pre_dispatch_rejected(target_id, operation));
    }
    validate_fixture_executable(executable)?;
    validate_maximum_items(maximum_items)?;
    let deadline = Deadline::new(timeout_ms).map_err(map_runtime_failure)?;
    let identity = mpris_runtime::resolve_private(
        address,
        deadline.remaining_ms().map_err(map_runtime_failure)?,
    )
    .map_err(map_runtime_failure)?;
    run_control(
        &identity,
        target_id,
        operation,
        maximum_items,
        &deadline,
        |request, timeout| {
            crate::mpris_control_candidate_client::run_fixed_image_process(
                executable,
                request,
                timeout,
                || false,
            )
        },
    )
}

fn run_observation<F>(
    identity: &MprisRuntimeIdentity,
    operation: &str,
    target_id: Option<&str>,
    maximum_items: u32,
    deadline: &Deadline,
    launch: F,
) -> AppResult<Value>
where
    F: FnOnce(&Value, Duration) -> AppResult<Value>,
{
    let timeout_ms = deadline.remaining_ms().map_err(map_runtime_failure)?;
    let request = observation_request(identity, operation, target_id, maximum_items, timeout_ms);
    launch(&request, Duration::from_millis(u64::from(timeout_ms)))
}

fn run_control<F>(
    identity: &MprisRuntimeIdentity,
    target_id: &str,
    operation: &str,
    maximum_items: u32,
    deadline: &Deadline,
    launch: F,
) -> AppResult<Value>
where
    F: FnOnce(&Value, Duration) -> AppResult<Value>,
{
    let timeout_ms = deadline.remaining_ms().map_err(map_runtime_failure)?;
    let request = control_request(identity, target_id, operation, maximum_items, timeout_ms);
    launch(&request, Duration::from_millis(u64::from(timeout_ms)))
}

fn observation_request(
    identity: &MprisRuntimeIdentity,
    operation: &str,
    target_id: Option<&str>,
    maximum_items: u32,
    timeout_ms: u32,
) -> Value {
    let mut request = observation_request_fields(
        identity.address(),
        identity.broker_epoch(),
        operation,
        target_id,
        maximum_items,
        timeout_ms,
    );
    request["expectedBusGuid"] = json!(identity.bus_guid());
    request
}

fn observation_request_fields(
    address: &str,
    broker_epoch: &str,
    operation: &str,
    target_id: Option<&str>,
    maximum_items: u32,
    timeout_ms: u32,
) -> Value {
    let mut request = json!({
        "protocolVersion": observation_contract::CONTRACT_VERSION,
        "operation": operation,
        "sessionBusAddress": address,
        "brokerEpoch": broker_epoch,
        "maximumItems": maximum_items,
        "timeoutMs": timeout_ms,
    });
    if let Some(target_id) = target_id {
        request["targetId"] = json!(target_id);
    }
    request
}

fn control_request(
    identity: &MprisRuntimeIdentity,
    target_id: &str,
    operation: &str,
    maximum_items: u32,
    timeout_ms: u32,
) -> Value {
    let mut request = control_request_fields(
        identity.address(),
        identity.broker_epoch(),
        target_id,
        operation,
        maximum_items,
        timeout_ms,
    );
    request["expectedBusGuid"] = json!(identity.bus_guid());
    request
}

fn control_request_fields(
    address: &str,
    broker_epoch: &str,
    target_id: &str,
    operation: &str,
    maximum_items: u32,
    timeout_ms: u32,
) -> Value {
    json!({
        "protocolVersion": control_contract::CONTRACT_VERSION,
        "sessionBusAddress": address,
        "brokerEpoch": broker_epoch,
        "targetId": target_id,
        "operation": operation,
        "confirmed": true,
        "maximumItems": maximum_items,
        "timeoutMs": timeout_ms,
    })
}

fn validate_maximum_items(maximum_items: u32) -> AppResult<()> {
    if (MINIMUM_MAXIMUM_ITEMS..=MAXIMUM_MAXIMUM_ITEMS).contains(&maximum_items) {
        Ok(())
    } else {
        Err(map_runtime_failure(Failure::Protocol))
    }
}

#[cfg(feature = "linux-mpris-candidate")]
fn validate_fixture_executable(executable: &Path) -> AppResult<()> {
    if executable.is_absolute() {
        Ok(())
    } else {
        Err(protocol_error(
            "The fixture worker executable must be an absolute path.",
        ))
    }
}

fn map_runtime_failure(failure: Failure) -> AppControlError {
    let (code, message, provider_state) = match failure {
        Failure::Unavailable => (
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The Linux MPRIS runtime is unavailable.",
            "unavailable",
        ),
        Failure::Timeout => (
            "TIMEOUT",
            "The Linux MPRIS runtime operation exceeded its deadline.",
            "timeout",
        ),
        Failure::Stale | Failure::BusEpochStale => (
            "STALE_SESSION",
            "The Linux MPRIS runtime generation is stale.",
            "stale",
        ),
        Failure::Ambiguous => (
            "AMBIGUOUS_TARGET",
            "The Linux MPRIS target is ambiguous.",
            "ambiguous",
        ),
        Failure::Protocol => (
            "WORKER_PROTOCOL_ERROR",
            "The Linux MPRIS runtime request violates its protocol.",
            "protocol-error",
        ),
        Failure::ProviderError => (
            "PROCESS_SNAPSHOT_FAILED",
            "The Linux MPRIS runtime provider failed to produce a snapshot.",
            "provider-error",
        ),
        Failure::OutcomeUnknown => (
            "OUTCOME_UNKNOWN",
            "The Linux MPRIS runtime outcome is unknown and is not retryable.",
            "outcome-unknown",
        ),
    };
    let accepted = matches!(failure, Failure::OutcomeUnknown);
    AppControlError::with_details(code, message, runtime_details(provider_state, accepted))
}

#[cfg(feature = "linux-mpris-candidate")]
fn protocol_error(message: &'static str) -> AppControlError {
    let error = map_runtime_failure(Failure::Protocol);
    AppControlError::with_details(error.code, message, error.details)
}

fn runtime_details(provider_state: &'static str, accepted: bool) -> Value {
    json!({
        "platform": "linux",
        "provider": "mpris-v2",
        "providerState": provider_state,
        "executionRealm": "isolated-worker",
        "fallback": "none",
        "partialResultPublished": false,
        "accepted": accepted,
        "targetMayHaveMutated": accepted,
        "retrySafe": !accepted,
        "automaticRetryProhibited": accepted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_builders_match_worker_contracts_without_extra_fields() {
        let observation = observation_request_fields(
            "unix:path=/tmp/mpris-bus",
            "fixture-user-bus-v1:0123456789abcdef0123456789abcdef",
            "state",
            Some("s2:m:0123456789abcdef"),
            4,
            17,
        );
        assert_eq!(observation.as_object().map(serde_json::Map::len), Some(7));
        assert_eq!(
            observation["protocolVersion"],
            observation_contract::CONTRACT_VERSION
        );
        assert_eq!(observation["timeoutMs"], 17);

        let control = control_request_fields(
            "unix:path=/tmp/mpris-bus",
            "fixture-user-bus-v1:0123456789abcdef0123456789abcdef",
            "s2:m:0123456789abcdef",
            "play",
            4,
            17,
        );
        assert_eq!(control.as_object().map(serde_json::Map::len), Some(8));
        assert_eq!(
            control["protocolVersion"],
            control_contract::CONTRACT_VERSION
        );
        assert_eq!(control["confirmed"], true);
    }

    #[test]
    fn unconfirmed_control_returns_before_deadline_or_runtime_resolution() {
        let result = control_current("invalid", "not-an-operation", false, 0, 0);
        assert!(result.is_ok());
        let Ok(value) = result else {
            return;
        };
        assert_eq!(value["data"]["dispatchOutcome"], "pre-dispatch-rejected");
        assert_eq!(value["data"]["accepted"], false);
    }

    #[test]
    fn invalid_maximum_items_is_rejected_before_runtime_resolution() {
        let result = discover_current(0, 1);
        assert_eq!(
            result.err().map(|error| error.code),
            Some("WORKER_PROTOCOL_ERROR")
        );
    }

    #[test]
    fn runtime_failure_details_do_not_contain_bus_identity() {
        let error = map_runtime_failure(Failure::Unavailable);
        let serialized = error.details.to_string();
        assert!(!serialized.contains("unix:path="));
        assert!(!serialized.contains("brokerEpoch"));
        assert!(!serialized.contains("guid"));
        assert!(!serialized.contains("inode"));
    }

    #[test]
    fn runtime_failure_details_preserve_dispatch_safety_for_every_failure() {
        for failure in [
            Failure::Unavailable,
            Failure::Timeout,
            Failure::Stale,
            Failure::BusEpochStale,
            Failure::Ambiguous,
            Failure::Protocol,
            Failure::ProviderError,
            Failure::OutcomeUnknown,
        ] {
            let error = map_runtime_failure(failure);
            let accepted = matches!(failure, Failure::OutcomeUnknown);
            assert_eq!(error.details["accepted"], accepted);
            assert_eq!(error.details["targetMayHaveMutated"], accepted);
            assert_eq!(error.details["retrySafe"], !accepted);
            assert_eq!(error.details["automaticRetryProhibited"], accepted);
        }
    }
}
