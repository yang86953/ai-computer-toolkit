//! 桌面 broker 请求、身份与重放验证。
use super::*;

/// 严格请求只允许同一 broker epoch 内的生命周期与会话级输入操作。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum BrokerRequest {
    Open {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        confirmed: bool,
        #[serde(rename = "foregroundConsent")]
        foreground_consent: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        #[serde(rename = "timeoutMs")]
        timeout_ms: u32,
    },
    Sessions {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
    },
    Inspect {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    Close {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    InputKey {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "foregroundConsent")]
        foreground_consent: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    InputPointer {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "foregroundConsent")]
        foreground_consent: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    Interact {
        /// Optional post-input observation, validated before dispatch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observation: Option<Value>,
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "foregroundConsent")]
        foreground_consent: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    Observe {
        #[serde(
            default,
            rename = "changeDetection",
            skip_serializing_if = "Option::is_none"
        )]
        change_detection: Option<Value>,
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    ObserveSubscribe {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    ObserveNext {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    ObserveUnsubscribe {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        input: Value,
    },
    CaptureFrame {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "sessionId")]
        session_id: String,
        confirmed: bool,
        #[serde(rename = "strictIsolation")]
        strict_isolation: bool,
        input: Value,
    },
    InputCancel {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        #[serde(rename = "targetRequestNonce")]
        target_request_nonce: String,
    },
    Shutdown {
        #[serde(rename = "contractVersion")]
        contract_version: String,
        #[serde(rename = "brokerEpoch")]
        broker_epoch: String,
        #[serde(rename = "requestNonce")]
        request_nonce: String,
    },
}

impl BrokerRequest {
    pub(super) fn contract_version(&self) -> &str {
        match self {
            Self::Open {
                contract_version, ..
            }
            | Self::Sessions {
                contract_version, ..
            }
            | Self::Inspect {
                contract_version, ..
            }
            | Self::Close {
                contract_version, ..
            }
            | Self::InputKey {
                contract_version, ..
            }
            | Self::InputPointer {
                contract_version, ..
            }
            | Self::Interact {
                contract_version, ..
            }
            | Self::Observe {
                contract_version, ..
            }
            | Self::ObserveSubscribe {
                contract_version, ..
            }
            | Self::ObserveNext {
                contract_version, ..
            }
            | Self::ObserveUnsubscribe {
                contract_version, ..
            }
            | Self::CaptureFrame {
                contract_version, ..
            }
            | Self::InputCancel {
                contract_version, ..
            }
            | Self::Shutdown {
                contract_version, ..
            } => contract_version,
        }
    }

    pub(super) fn broker_epoch(&self) -> &str {
        match self {
            Self::Open { broker_epoch, .. }
            | Self::Sessions { broker_epoch, .. }
            | Self::Inspect { broker_epoch, .. }
            | Self::Close { broker_epoch, .. }
            | Self::InputKey { broker_epoch, .. }
            | Self::InputPointer { broker_epoch, .. }
            | Self::Interact { broker_epoch, .. }
            | Self::Observe { broker_epoch, .. }
            | Self::ObserveSubscribe { broker_epoch, .. }
            | Self::ObserveNext { broker_epoch, .. }
            | Self::ObserveUnsubscribe { broker_epoch, .. }
            | Self::CaptureFrame { broker_epoch, .. }
            | Self::InputCancel { broker_epoch, .. }
            | Self::Shutdown { broker_epoch, .. } => broker_epoch,
        }
    }

    pub(super) fn request_nonce(&self) -> &str {
        match self {
            Self::Open { request_nonce, .. }
            | Self::Sessions { request_nonce, .. }
            | Self::Inspect { request_nonce, .. }
            | Self::Close { request_nonce, .. }
            | Self::InputKey { request_nonce, .. }
            | Self::InputPointer { request_nonce, .. }
            | Self::Interact { request_nonce, .. }
            | Self::Observe { request_nonce, .. }
            | Self::ObserveSubscribe { request_nonce, .. }
            | Self::ObserveNext { request_nonce, .. }
            | Self::ObserveUnsubscribe { request_nonce, .. }
            | Self::CaptureFrame { request_nonce, .. }
            | Self::InputCancel { request_nonce, .. }
            | Self::Shutdown { request_nonce, .. } => request_nonce,
        }
    }

    pub(super) const fn operation(&self) -> &'static str {
        match self {
            Self::Open { .. } => "open",
            Self::Sessions { .. } => "sessions",
            Self::Inspect { .. } => "inspect",
            Self::Close { .. } => "close",
            Self::InputKey { .. } => "input-key",
            Self::InputPointer { .. } => "input-pointer",
            Self::Observe { .. } => "observe",
            Self::ObserveSubscribe { .. } => "observe-subscribe",
            Self::ObserveNext { .. } => "observe-next",
            Self::ObserveUnsubscribe { .. } => "observe-unsubscribe",
            Self::Interact { .. } => "interact",
            Self::CaptureFrame { .. } => "capture-frame",
            Self::InputCancel { .. } => "input-cancel",
            Self::Shutdown { .. } => "shutdown",
        }
    }

    pub(super) const fn is_input(&self) -> bool {
        matches!(
            self,
            Self::InputKey { .. } | Self::InputPointer { .. } | Self::Interact { .. }
        )
    }

    pub(super) fn cancellation_target(&self) -> Option<&str> {
        match self {
            Self::InputCancel {
                target_request_nonce,
                ..
            } => Some(target_request_nonce),
            _ => None,
        }
    }

    pub(super) const fn retry_safe_on_success(&self) -> bool {
        matches!(
            self,
            Self::Sessions { .. } | Self::Inspect { .. } | Self::InputCancel { .. }
        )
    }

    pub(super) fn fingerprint(&self) -> Result<String, AppControlError> {
        serde_json::to_string(self).map_err(|_| {
            AppControlError::new(
                "BROKER_PROTOCOL_FAILED",
                "The desktop session request could not be normalized.",
            )
        })
    }

    pub(super) fn validate(&self, current_epoch: &str) -> Result<(), AppControlError> {
        if !canonical_nonce(self.request_nonce()) {
            return Err(protocol_error("request-nonce-is-not-canonical"));
        }
        if let Self::InputCancel {
            target_request_nonce,
            ..
        } = self
            && !canonical_nonce(target_request_nonce)
        {
            return Err(protocol_error("target-request-nonce-is-not-canonical"));
        }
        if self.contract_version() != CONTRACT_VERSION {
            return Err(protocol_error("contract-version-mismatch"));
        }
        if self.broker_epoch() != current_epoch {
            return Err(AppControlError::with_details(
                "STALE_BROKER_EPOCH",
                "The desktop session broker epoch is stale.",
                json!({
                    "outcome": "failed",
                    "acceptedMayHaveOccurred": false,
                    "retrySafe": false,
                    "automaticRetryProhibited": true,
                }),
            ));
        }
        Ok(())
    }
}
