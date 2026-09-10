//! 协作式 UIX Agent 精确窗口关闭等待 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, Failure, RevisionWaitOutcomeKind},
    capabilities,
    components::uix_window_closed_wait_contract::UixWindowClosedWaitInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 在总 deadline 内等待 Agent 证明精确窗口 generation 已关闭。
pub(crate) fn wait_closed(target: &str, value: &Value) -> AppResult<Value> {
    let input = UixWindowClosedWaitInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome =
        uix_agent::wait_closed(target, input.timeout_ms).map_err(uix_window::public_error)?;
    if outcome.kind != RevisionWaitOutcomeKind::Closed || !outcome.closed {
        return Err(uix_window::public_error(Failure::Protocol));
    }
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-closed-wait/v2",
        "capability": capabilities::WINDOW_CLOSED_WAIT_V2,
        "targetId": target,
        "outcome": "closed",
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "closed": true,
        "exactGenerationConfirmed": true,
        "targetReResolved": true,
        "timeoutMs": input.timeout_ms,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "safety": {
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "windowInventoryPolled": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}
