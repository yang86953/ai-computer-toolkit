//! Linux 精确进程终止 Module；显式风险路线、deadline 与结果语义不下沉到 pidfd Adapter。

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    adapters::linux::process_termination_pidfd::SystemPidfdTerminationPort,
    capabilities,
    components::{
        linux_process_termination,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    domain::{AppControlError, AppResult},
};

/// 平台保护域只投影封闭类别，不公开 PID、UID 或 procfs 内容。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxProcessProtection {
    RootExecution,
    PrivilegedExecution,
    InitProcess,
    CurrentToolOrAncestor,
    DifferentUser,
    ProtectionStateUnknown,
}

impl LinuxProcessProtection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RootExecution => "root-execution",
            Self::PrivilegedExecution => "privileged-execution",
            Self::InitProcess => "init-process",
            Self::CurrentToolOrAncestor => "current-tool-or-ancestor",
            Self::DifferentUser => "different-user",
            Self::ProtectionStateUnknown => "protection-state-unknown",
        }
    }
}

/// 温和与强制风险必须由不同 capability/operation 静态选择，input 不得改写。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminationRoute {
    Graceful,
    Force,
}

impl TerminationRoute {
    const fn capability(self) -> &'static str {
        match self {
            Self::Graceful => capabilities::PROCESS_TERMINATE_GRACEFUL_V2,
            Self::Force => capabilities::PROCESS_TERMINATE_FORCE_V2,
        }
    }

    const fn operation(self) -> &'static str {
        match self {
            Self::Graceful => "terminate-graceful",
            Self::Force => "terminate-force",
        }
    }

    const fn risk_level(self) -> &'static str {
        match self {
            Self::Graceful => "high",
            Self::Force => "critical",
        }
    }

    const fn mechanism(self) -> &'static str {
        match self {
            Self::Graceful => "process-self-termination-request",
            Self::Force => "kernel-forced-process-termination",
        }
    }

    const fn assessment_signal(self) -> &'static str {
        match self {
            Self::Graceful => "graceful-self-termination-request",
            Self::Force => "explicit-kernel-forced-termination",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Force => "forced",
        }
    }
}

/// Adapter 失败只携带领域可判定的阶段事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxProcessTerminationFailure {
    Stale,
    Ambiguous,
    InventoryIncomplete,
    Protected(LinuxProcessProtection),
    PermissionDenied,
    ProviderUnavailable,
    DispatchFailed,
    WaitFailed,
}

/// pidfd lease 保证后续 signal 与退出等待绑定同一内核进程对象。
pub(crate) trait LinuxProcessTerminationLease {
    fn dispatch_graceful(&self) -> Result<(), LinuxProcessTerminationFailure>;
    fn dispatch_force(&self) -> Result<(), LinuxProcessTerminationFailure>;
    fn wait_exited(&self, timeout: Duration) -> Result<bool, LinuxProcessTerminationFailure>;
}

/// Module 的窄平台端口；原生 PID、UID 与 fd 不跨过该边界。
pub(crate) trait LinuxProcessTerminationPort {
    fn prepare(
        &self,
        target_id: &str,
    ) -> Result<Box<dyn LinuxProcessTerminationLease>, LinuxProcessTerminationFailure>;
}

/// 执行固定 SIGTERM 语义；确认始终先于 input、目标和 procfs/pidfd。
pub(crate) fn perform(
    target_id: Option<&str>,
    confirmed: bool,
    input: Option<&Value>,
) -> AppResult<Value> {
    perform_with(
        &SystemPidfdTerminationPort,
        TerminationRoute::Graceful,
        target_id,
        confirmed,
        input,
    )
}

/// 执行显式 force capability；不会先尝试温和终止，也不是温和路线的 fallback。
pub(crate) fn perform_force(
    target_id: Option<&str>,
    confirmed: bool,
    input: Option<&Value>,
) -> AppResult<Value> {
    perform_with(
        &SystemPidfdTerminationPort,
        TerminationRoute::Force,
        target_id,
        confirmed,
        input,
    )
}

/// 只读预检精确进程保护域和 pidfd 可达性，不发送信号。
pub(crate) fn assess(target_id: &str) -> AppResult<Value> {
    assess_with(TerminationRoute::Graceful, target_id)
}

/// 只读预检显式强制路线；仍不发送信号或降低保护门禁。
pub(crate) fn assess_force(target_id: &str) -> AppResult<Value> {
    assess_with(TerminationRoute::Force, target_id)
}

fn assess_with(route: TerminationRoute, target_id: &str) -> AppResult<Value> {
    let parsed = OpaqueTargetId::parse(target_id).ok_or_else(|| {
        AppControlError::new(
            "STALE_SESSION",
            "The assessment target is not a current canonical opaque session.",
        )
    })?;
    if parsed.kind() != OpaqueTargetKind::Process {
        return Err(AppControlError::new(
            "INVALID_TARGET_KIND",
            format!(
                "Linux {} process termination requires a canonical process target.",
                route.label()
            ),
        ));
    }
    let lease = LinuxProcessTerminationPort::prepare(&SystemPidfdTerminationPort, target_id)
        .map_err(|failure| before_dispatch_error(failure, target_id))?;
    drop(lease);
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "capability": route.capability(),
        "targetId": target_id,
        "decision": "confirmation-required",
        "executionRealm": "host-background",
        "requiresConfirmation": true,
        "requiresForegroundConsent": false,
        "reasons": ["procfs-owner-generation-bound-same-user-process-is-pidfd-addressable"],
        "constraints": {
            "scope": "exact-current-process-generation",
            "processOwnerGenerationRequired": true,
            "sameNonRootUserRequired": true,
            "protectedTargetsDenied": true,
            "signal": route.assessment_signal(),
            "riskLevel": route.risk_level(),
            "explicitForceSelectionRequired": route == TerminationRoute::Force,
            "forceFallback": false,
            "noFallback": true,
        },
        "evidence": {
            "targetKind": "running-process",
            "platform": "linux",
            "implementationState": "production-route-kernel-verified",
            "pidfdAvailable": true,
            "processOwnerGenerationBound": true,
            "signalAttempted": false,
            "nativeIdentityExposed": false,
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
        },
    }))
}

fn perform_with(
    port: &dyn LinuxProcessTerminationPort,
    route: TerminationRoute,
    target_id: Option<&str>,
    confirmed: bool,
    input: Option<&Value>,
) -> AppResult<Value> {
    if !confirmed {
        return Err(AppControlError::with_details(
            "CONFIRMATION_REQUIRED",
            format!(
                "Linux {} process termination requires explicit confirmation.",
                route.label()
            ),
            json!({
                "targetResolved": false,
                "signalAttempted": false,
                "acceptedMayHaveOccurred": false,
                "retrySafe": true,
            }),
        ));
    }
    let input = input.ok_or_else(|| {
        AppControlError::new(
            "INVALID_ARGUMENT",
            format!(
                "args.input is required for Linux {} process termination.",
                route.label()
            ),
        )
    })?;
    let input = match route {
        TerminationRoute::Graceful => linux_process_termination::parse_graceful(input)?,
        TerminationRoute::Force => linux_process_termination::parse_force(input)?,
    };
    let target_id = target_id
        .filter(|target| {
            OpaqueTargetId::parse(target)
                .is_some_and(|parsed| parsed.kind() == OpaqueTargetKind::Process)
        })
        .ok_or_else(|| {
            AppControlError::new(
                "INVALID_TARGET_KIND",
                format!(
                    "Linux {} process termination requires a canonical process target.",
                    route.label()
                ),
            )
        })?;
    let started = Instant::now();
    let deadline = started.checked_add(input.timeout()).ok_or_else(|| {
        AppControlError::new("INVALID_ARGUMENT", "The termination deadline is invalid.")
    })?;
    let lease = port
        .prepare(target_id)
        .map_err(|failure| before_dispatch_error(failure, target_id))?;
    remaining(deadline)
        .ok_or_else(|| before_dispatch_timeout(route, target_id, "prepare-pidfd"))?;
    let dispatch = match route {
        TerminationRoute::Graceful => lease.dispatch_graceful(),
        TerminationRoute::Force => lease.dispatch_force(),
    };
    dispatch.map_err(|failure| dispatch_rejected_error(failure, target_id))?;
    let Some(remaining) = remaining(deadline) else {
        return Err(after_dispatch_unknown(
            route,
            target_id,
            "deadline-after-dispatch",
        ));
    };
    match lease.wait_exited(remaining) {
        Ok(true) => Ok(success(route, target_id)),
        Ok(false) => Err(after_dispatch_unknown(route, target_id, "wait-timeout")),
        Err(_) => Err(after_dispatch_unknown(route, target_id, "wait-pidfd")),
    }
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
}

fn before_dispatch_timeout(
    route: TerminationRoute,
    target_id: &str,
    stage: &'static str,
) -> AppControlError {
    AppControlError::with_details(
        "TIMEOUT",
        format!(
            "Linux {} process termination expired before signal dispatch.",
            route.label()
        ),
        json!({
            "targetId": target_id,
            "stage": stage,
            "acceptedMayHaveOccurred": false,
            "signalAttempted": false,
            "retrySafe": true,
        }),
    )
}

fn before_dispatch_error(
    failure: LinuxProcessTerminationFailure,
    target_id: &str,
) -> AppControlError {
    let (code, message, details) = match failure {
        LinuxProcessTerminationFailure::Stale => (
            "STALE_SESSION",
            "The exact Linux process generation no longer exists.",
            json!({}),
        ),
        LinuxProcessTerminationFailure::Ambiguous => (
            "AMBIGUOUS_TARGET",
            "The opaque process target matched more than one current process.",
            json!({}),
        ),
        LinuxProcessTerminationFailure::InventoryIncomplete => (
            "PROCESS_SNAPSHOT_FAILED",
            "The bounded Linux process inventory could not prove target absence.",
            json!({"inventoryComplete": false}),
        ),
        LinuxProcessTerminationFailure::Protected(kind) => (
            "PERMISSION_DENIED",
            "The exact Linux process is inside a protected termination domain.",
            json!({"protectedTarget": true, "protectedKind": kind.as_str()}),
        ),
        LinuxProcessTerminationFailure::PermissionDenied => (
            "PERMISSION_DENIED",
            "Linux denied the exact pidfd termination request.",
            json!({"protectedTarget": false}),
        ),
        LinuxProcessTerminationFailure::ProviderUnavailable => (
            "CAPABILITY_UNAVAILABLE",
            "The Linux pidfd termination provider is unavailable.",
            json!({"providerState": "pidfd-unavailable"}),
        ),
        LinuxProcessTerminationFailure::DispatchFailed
        | LinuxProcessTerminationFailure::WaitFailed => (
            "OPERATION_FAILED",
            "The Linux pidfd termination request failed before acceptance.",
            json!({}),
        ),
    };
    AppControlError::with_details(
        code,
        message,
        merge_details(
            details,
            json!({
                "targetId": target_id,
                "acceptedMayHaveOccurred": false,
                "signalAttempted": false,
                "retrySafe": true,
                "fallback": "none",
            }),
        ),
    )
}

fn dispatch_rejected_error(
    failure: LinuxProcessTerminationFailure,
    target_id: &str,
) -> AppControlError {
    let mut error = before_dispatch_error(failure, target_id);
    if let Some(details) = error.details.as_object_mut() {
        details.insert("stage".to_owned(), json!("signal-dispatch"));
        details.insert("signalAttempted".to_owned(), json!(true));
    }
    error
}

fn merge_details(mut left: Value, right: Value) -> Value {
    if let (Some(left), Some(right)) = (left.as_object_mut(), right.as_object()) {
        left.extend(right.clone());
    }
    left
}

fn after_dispatch_unknown(
    route: TerminationRoute,
    target_id: &str,
    stage: &'static str,
) -> AppControlError {
    AppControlError::with_details(
        "OUTCOME_UNKNOWN",
        format!(
            "Linux accepted the {} process termination request, but exact exit was not certified.",
            route.label()
        ),
        json!({
            "targetId": target_id,
            "stage": stage,
            "outcome": "unknown",
            "accepted": true,
            "acceptedMayHaveOccurred": true,
            "signalAttempted": true,
            "sameProcessGenerationBoundByPidfd": true,
            "retrySafe": false,
            "automaticRetryProhibited": true,
            "reobserveWith": "process.discover@1",
            "fallback": "none",
        }),
    )
}

fn success(route: TerminationRoute, target_id: &str) -> Value {
    let mut result = json!({
        "ok": true,
        "app": "process",
        "verb": route.operation(),
        "capability": route.capability(),
        "targetId": target_id,
        "action": route.operation(),
        "riskLevel": route.risk_level(),
        "mechanism": route.mechanism(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "finalStateReached": true,
        "state": "exited",
        "targetReresolvedBeforeDispatch": true,
        "sameProcessGenerationVerified": true,
        "pidfdBound": true,
        "sameNonRootUserVerified": true,
        "protectedTargetCheck": true,
        "confirmationEvaluatedBeforeTargetAccess": true,
        "processOwnerGenerationBound": true,
        "reobserveWith": "process.discover@1",
        "expectedObservation": "target-absent",
        "foregroundActivationRequested": false,
        "foregroundStateObserved": false,
        "desktopInputInjected": false,
        "retrySafe": false,
        "automaticRetryProhibited": true,
        "executionRealm": "host-background",
    });
    match route {
        TerminationRoute::Graceful => {
            result["gracefulFallbackToForce"] = json!(false);
        }
        TerminationRoute::Force => {
            result["gracefulAttempted"] = json!(false);
            result["forceWasExplicitlySelected"] = json!(true);
            result["forceFallbackFromGraceful"] = json!(false);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    };

    use super::*;

    struct NoAccessPort;

    impl LinuxProcessTerminationPort for NoAccessPort {
        fn prepare(
            &self,
            _: &str,
        ) -> Result<Box<dyn LinuxProcessTerminationLease>, LinuxProcessTerminationFailure> {
            panic!("未确认请求不得访问目标");
        }
    }

    #[test]
    fn confirmation_precedes_input_and_target_access() {
        let error = perform_with(&NoAccessPort, TerminationRoute::Graceful, None, false, None)
            .err()
            .unwrap_or_else(|| panic!("未确认请求必须失败"));
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        assert_eq!(error.details["targetResolved"], false);
        assert_eq!(error.details["signalAttempted"], false);

        let force = perform_with(&NoAccessPort, TerminationRoute::Force, None, false, None)
            .err()
            .unwrap_or_else(|| panic!("未确认强制请求必须失败"));
        assert_eq!(force.code, "CONFIRMATION_REQUIRED");
        assert_eq!(force.details["targetResolved"], false);
    }

    struct RecordingPort(Arc<AtomicU8>);

    struct RecordingLease(Arc<AtomicU8>);

    impl LinuxProcessTerminationPort for RecordingPort {
        fn prepare(
            &self,
            _: &str,
        ) -> Result<Box<dyn LinuxProcessTerminationLease>, LinuxProcessTerminationFailure> {
            Ok(Box::new(RecordingLease(Arc::clone(&self.0))))
        }
    }

    impl LinuxProcessTerminationLease for RecordingLease {
        fn dispatch_graceful(&self) -> Result<(), LinuxProcessTerminationFailure> {
            self.0.store(1, Ordering::Release);
            Ok(())
        }

        fn dispatch_force(&self) -> Result<(), LinuxProcessTerminationFailure> {
            self.0.store(2, Ordering::Release);
            Ok(())
        }

        fn wait_exited(&self, _: Duration) -> Result<bool, LinuxProcessTerminationFailure> {
            Ok(true)
        }
    }

    #[test]
    fn explicit_route_selects_exactly_one_static_dispatch() {
        let target = OpaqueTargetId::new(OpaqueTargetKind::Process, "fixture").to_string();
        for (route, expected, capability) in [
            (
                TerminationRoute::Graceful,
                1,
                capabilities::PROCESS_TERMINATE_GRACEFUL_V2,
            ),
            (
                TerminationRoute::Force,
                2,
                capabilities::PROCESS_TERMINATE_FORCE_V2,
            ),
        ] {
            let observed = Arc::new(AtomicU8::new(0));
            let result = perform_with(
                &RecordingPort(Arc::clone(&observed)),
                route,
                Some(&target),
                true,
                Some(&json!({"timeoutMs": 1000})),
            )
            .unwrap_or_else(|error| panic!("静态风险路线应成功：{error:?}"));
            assert_eq!(observed.load(Ordering::Acquire), expected);
            assert_eq!(result["capability"], capability);
        }
    }
}
