//! 定义 spike 的许可参数与无敏感信息输出契约。

use serde::Serialize;

pub(crate) const CONTRACT_VERSION: &str = "linux-portal-eis-spike/v1";
pub(crate) const MINIMUM_TIMEOUT_MS: u32 = 10_000;
pub(crate) const MAXIMUM_TIMEOUT_MS: u32 = 300_000;

pub(crate) const HELP: &str = "用法: ai-computer-toolkit-linux-portal-eis-spike --confirmed \
--allow-foreground --timeout-ms <10000..300000>";

/// 保存已经通过 confirmation-first 门禁的运行参数。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RunConfig {
    pub(crate) timeout_ms: u32,
}

/// 区分帮助输出与真实运行，确保帮助不会连接 D-Bus。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CliAction {
    Help,
    Run(RunConfig),
}

/// 保存不会携带调用方字符串或基础设施细节的稳定失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) stage: &'static str,
}

impl Failure {
    pub(crate) const fn new(code: &'static str, stage: &'static str) -> Self {
        Self { code, stage }
    }
}

/// 解析严格参数集合；缺少许可时在任何基础设施访问前失败。
pub(crate) fn parse_cli<I, S>(arguments: I) -> Result<CliAction, Failure>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let values = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    if values.as_slice() == ["--help"] || values.as_slice() == ["-h"] {
        return Ok(CliAction::Help);
    }

    let mut confirmed = false;
    let mut foreground = false;
    let mut timeout_ms = None;
    let mut index = 0_usize;
    while index < values.len() {
        match values[index].as_str() {
            "--confirmed" if !confirmed => confirmed = true,
            "--allow-foreground" if !foreground => foreground = true,
            "--timeout-ms" if timeout_ms.is_none() => {
                index = index.saturating_add(1);
                let value = values
                    .get(index)
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|value| (MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(value))
                    .ok_or_else(|| Failure::new("INVALID_ARGUMENT", "preflight"))?;
                timeout_ms = Some(value);
            }
            _ => return Err(Failure::new("INVALID_ARGUMENT", "preflight")),
        }
        index = index.saturating_add(1);
    }

    if !confirmed {
        return Err(Failure::new("CONFIRMATION_REQUIRED", "preflight"));
    }
    if !foreground {
        return Err(Failure::new("FOREGROUND_CONSENT_REQUIRED", "preflight"));
    }
    let timeout_ms = timeout_ms.ok_or_else(|| Failure::new("INVALID_ARGUMENT", "preflight"))?;
    Ok(CliAction::Run(RunConfig { timeout_ms }))
}

/// 保存真实 Portal 与 EIS 验证后允许公开的最小事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Verification {
    pub(crate) remote_desktop_version: u32,
    pub(crate) screen_cast_version: u32,
    pub(crate) authorized_device_classes: Vec<&'static str>,
    pub(crate) stream_count: usize,
    pub(crate) mapping_id_count: usize,
}

/// 成功输出不包含任何 session、request、FD、node 或 mapping 身份。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuccessSummary {
    contract_version: &'static str,
    outcome: &'static str,
    remote_desktop_version: u32,
    screen_cast_version: u32,
    authorized_device_classes: Vec<&'static str>,
    stream_count: usize,
    mapping_id_count: usize,
    eis_handshake_complete: bool,
    pipe_wire_remote_obtained: bool,
    session_closed: bool,
    restore_token_retained: bool,
    input_events_sent: u32,
    pixels_consumed: u32,
}

impl From<Verification> for SuccessSummary {
    fn from(value: Verification) -> Self {
        Self {
            contract_version: CONTRACT_VERSION,
            outcome: "verified",
            remote_desktop_version: value.remote_desktop_version,
            screen_cast_version: value.screen_cast_version,
            authorized_device_classes: value.authorized_device_classes,
            stream_count: value.stream_count,
            mapping_id_count: value.mapping_id_count,
            eis_handshake_complete: true,
            pipe_wire_remote_obtained: true,
            session_closed: true,
            restore_token_retained: false,
            input_events_sent: 0,
            pixels_consumed: 0,
        }
    }
}

/// 失败输出仅投影稳定阶段与 session 清理事实。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureSummary {
    contract_version: &'static str,
    outcome: &'static str,
    code: &'static str,
    stage: &'static str,
    session_closed: bool,
    restore_token_retained: bool,
    input_events_sent: u32,
    pixels_consumed: u32,
}

impl FailureSummary {
    pub(crate) const fn new(failure: Failure, session_closed: bool) -> Self {
        Self {
            contract_version: CONTRACT_VERSION,
            outcome: "failed",
            code: failure.code,
            stage: failure.stage,
            session_closed,
            restore_token_retained: false,
            input_events_sent: 0,
            pixels_consumed: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: Serialize>(value: &T) -> String {
        match serde_json::to_string(value) {
            Ok(text) => text,
            Err(error) => panic!("测试 JSON 编码失败: {error}"),
        }
    }

    #[test]
    fn cli_requires_both_explicit_permissions_before_run() {
        let missing_confirmation = parse_cli(["--allow-foreground", "--timeout-ms", "120000"]);
        assert_eq!(
            missing_confirmation,
            Err(Failure::new("CONFIRMATION_REQUIRED", "preflight"))
        );
        let missing_foreground = parse_cli(["--confirmed", "--timeout-ms", "120000"]);
        assert_eq!(
            missing_foreground,
            Err(Failure::new("FOREGROUND_CONSENT_REQUIRED", "preflight"))
        );
    }

    #[test]
    fn cli_accepts_only_bounded_complete_argument_set() {
        let parsed = parse_cli([
            "--timeout-ms",
            "120000",
            "--allow-foreground",
            "--confirmed",
        ]);
        assert_eq!(
            parsed,
            Ok(CliAction::Run(RunConfig {
                timeout_ms: 120_000
            }))
        );
        assert!(parse_cli(["--confirmed", "--confirmed"]).is_err());
        assert!(parse_cli(["--confirmed", "--allow-foreground"]).is_err());
    }

    #[test]
    fn help_is_non_running_action() {
        assert_eq!(parse_cli(["--help"]), Ok(CliAction::Help));
    }

    #[test]
    fn success_output_freezes_zero_side_effects_and_no_sensitive_fields() {
        let text = json(&SuccessSummary::from(Verification {
            remote_desktop_version: 2,
            screen_cast_version: 5,
            authorized_device_classes: vec!["keyboard", "pointer"],
            stream_count: 1,
            mapping_id_count: 1,
        }));
        assert!(text.contains("\"inputEventsSent\":0"));
        assert!(text.contains("\"pixelsConsumed\":0"));
        assert!(text.contains("\"sessionClosed\":true"));
        for forbidden in ["sessionHandle", "requestHandle", "nodeId", "mappingId\""] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn failure_output_never_echoes_infrastructure_details() {
        let text = json(&FailureSummary::new(
            Failure::new("PORTAL_PROTOCOL_ERROR", "start"),
            false,
        ));
        assert!(text.contains("\"outcome\":\"failed\""));
        assert!(text.contains("\"restoreTokenRetained\":false"));
        assert!(!text.contains("/org/freedesktop"));
    }
}
