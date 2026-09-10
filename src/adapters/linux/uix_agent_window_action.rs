//! UIX Agent targetless 窗口动作的私有 Adapter。

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::components::{
    uix_key_input_contract::UixKeyInput, uix_pointer_input_contract::UixPointerInput,
    uix_window_lifecycle_contract::UixWindowLifecycleAction,
};

use super::{
    ActionOutcome, AgentClient, Failure, RequestFailure, WindowActivationOutcome, WirePerformed,
    resolve_until, window_session_id,
};

/// 已进入 targetless 窗口动作 Adapter 后的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowActionFailure {
    Transport(Failure),
    UnsupportedAction,
    StaleRevision,
    Forbidden,
    NotPresentable,
    NotInteractable,
    WindowOperationFailed,
    DidNotSettle,
    OutcomeUnknown,
}

/// 重新解析窗口后，在总 deadline 内执行一次 UIX 生命周期动作。
pub(crate) fn perform_window(
    target: &str,
    action: UixWindowLifecycleAction,
    timeout_ms: u32,
) -> Result<ActionOutcome, WindowActionFailure> {
    with_client(target, timeout_ms, |client, window| {
        client.perform_window(window.window_id, window.generation, window.revision, action)
    })
}

/// 重新解析窗口后，在总 deadline 内提交一次 UIX 平台关闭请求。
pub(crate) fn perform_close(
    target: &str,
    timeout_ms: u32,
) -> Result<ActionOutcome, WindowActionFailure> {
    with_client(target, timeout_ms, |client, window| {
        client.perform_close(window.window_id, window.generation, window.revision)
    })
}

/// 重新解析窗口后，在总 deadline 内执行一次 UIX 应用内完整按键。
pub(crate) fn perform_key(
    target: &str,
    input: &UixKeyInput,
) -> Result<ActionOutcome, WindowActionFailure> {
    with_client(target, input.timeout_ms(), |client, window| {
        client.perform_key(window.window_id, window.generation, window.revision, input)
    })
}

/// 重新解析窗口后，在总 deadline 内执行一次 UIX 应用内指针动作。
pub(crate) fn perform_pointer(
    target: &str,
    input: &UixPointerInput,
) -> Result<ActionOutcome, WindowActionFailure> {
    with_client(target, input.timeout_ms(), |client, window| {
        client.perform_pointer(window.window_id, window.generation, window.revision, input)
    })
}

/// 重新解析窗口后提交一次前台激活请求，并尽力观察同一代际焦点事实。
pub(crate) fn perform_activation(
    target: &str,
    timeout_ms: u32,
) -> Result<WindowActivationOutcome, WindowActionFailure> {
    with_client(target, timeout_ms, |client, window| {
        client.perform_activation(window, target)
    })
}

fn with_client<T>(
    target: &str,
    timeout_ms: u32,
    operation: impl FnOnce(&mut AgentClient, &super::WindowRecord) -> Result<T, WindowActionFailure>,
) -> Result<T, WindowActionFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    let window = resolve_until(target, deadline).map_err(WindowActionFailure::Transport)?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)
        .map_err(WindowActionFailure::Transport)?;
    operation(&mut client, &window)
}

impl AgentClient {
    /// 核对关闭动作目录后发送一次无 target 请求。
    pub(super) fn perform_close(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        if !self.window_actions.contains("close_window") {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        self.perform_targetless(
            window_id,
            generation,
            expected_revision,
            json!({ "kind": "close_window" }),
        )
    }

    /// 核对生命周期动作目录后发送一次无 target 请求。
    pub(super) fn perform_window(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        action: UixWindowLifecycleAction,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        if !self.window_actions.contains(action.provider_action()) {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        self.perform_targetless(
            window_id,
            generation,
            expected_revision,
            action.provider_value(),
        )
    }

    /// 核对窗口动作、键名与修饰键目录后发送一次完整按键。
    pub(super) fn perform_key(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        input: &UixKeyInput,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        if !self.window_actions.contains("press_key")
            || !self.key_names.contains(input.provider_key())
            || input
                .provider_modifiers()
                .iter()
                .any(|modifier| !self.key_modifiers.contains(*modifier))
        {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        self.perform_targetless(
            window_id,
            generation,
            expected_revision,
            input.provider_value(),
        )
    }

    /// 核对窗口动作目录后发送一次应用内指针动作。
    pub(super) fn perform_pointer(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        input: &UixPointerInput,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        if !self
            .window_actions
            .contains(input.action().provider_action())
        {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        self.perform_targetless(
            window_id,
            generation,
            expected_revision,
            input.provider_value(),
        )
    }

    /// 提交激活请求后只把同连接、同代际的焦点字段作为额外观察。
    pub(super) fn perform_activation(
        &mut self,
        window: &super::WindowRecord,
        target: &str,
    ) -> Result<WindowActivationOutcome, WindowActionFailure> {
        if !self.window_actions.contains("activate_window") {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        self.perform_targetless(
            window.window_id,
            window.generation,
            window.revision,
            json!({ "kind": "activate_window" }),
        )?;
        let (focus_observed_after_dispatch, target_generation_current_after_dispatch) =
            self.activation_observation(window, target);
        Ok(WindowActivationOutcome {
            focus_observed_after_dispatch,
            target_generation_current_after_dispatch,
        })
    }

    /// 观察失败不改变已经成功提交的激活结果，避免诱导重复激活。
    fn activation_observation(
        &mut self,
        window: &super::WindowRecord,
        target: &str,
    ) -> (Option<bool>, Option<bool>) {
        let Ok(windows) = self.list_windows() else {
            return (None, None);
        };
        if windows.len() > super::MAX_WINDOWS {
            return (None, None);
        }
        let mut matching = windows
            .iter()
            .filter(|candidate| candidate.window_id == window.window_id);
        let Some(candidate) = matching.next() else {
            return (None, Some(false));
        };
        if matching.next().is_some() {
            return (None, None);
        }
        let current = !candidate.closed
            && candidate.generation == window.generation
            && candidate.revision >= window.revision
            && candidate.presented_revision >= window.presented_revision
            && candidate.presented_revision <= candidate.revision
            && window_session_id(&window.endpoint, candidate) == target;
        if !current {
            return (None, Some(false));
        }
        let focused = if self.window_state_fields.contains("focused") {
            candidate.focused
        } else {
            None
        };
        (focused, Some(true))
    }

    /// 在已认证连接上发送无 target 的窗口动作。
    fn perform_targetless(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        action: Value,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        self.perform_targetless_with_request_id(
            "act-perform-window",
            window_id,
            generation,
            expected_revision,
            action,
        )
    }

    /// 使用阶段专属 request id 发送窗口动作，避免拖拽释放误认延迟响应。
    pub(super) fn perform_targetless_with_request_id(
        &mut self,
        request_id: &'static str,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        action: Value,
    ) -> Result<ActionOutcome, WindowActionFailure> {
        if !self.request_types.contains("perform") {
            return Err(WindowActionFailure::UnsupportedAction);
        }
        let payload = json!({
            "window_id": window_id,
            "generation": generation,
            "expected_revision": expected_revision,
            "action": action,
        });
        let reply = self
            .request(request_id, "perform", payload)
            .map_err(window_request_failure)?;
        performed_window_outcome(reply, window_id, generation, expected_revision)
    }
}

fn performed_window_outcome(
    reply: Value,
    window_id: u64,
    generation: u64,
    expected_revision: u64,
) -> Result<ActionOutcome, WindowActionFailure> {
    let performed = serde_json::from_value::<WirePerformed>(reply)
        .map_err(|_| WindowActionFailure::Transport(Failure::Protocol))?;
    if performed.window_id != window_id
        || performed.generation != generation
        || performed.revision < expected_revision
        || performed.presented_revision > performed.revision
    {
        return Err(WindowActionFailure::Transport(Failure::Protocol));
    }
    Ok(ActionOutcome {
        revision: performed.revision,
        presented_revision: performed.presented_revision,
        settled: performed.settled,
        application_confirmation_performed: false,
    })
}

fn window_request_failure(error: RequestFailure) -> WindowActionFailure {
    match error {
        // perform 帧开始发送后失去可信终态时禁止自动重试。
        RequestFailure::Transport(_) => WindowActionFailure::OutcomeUnknown,
        RequestFailure::Remote { code, .. } => match code.as_str() {
            "unauthorized" => WindowActionFailure::Transport(Failure::PermissionDenied),
            // Agent 的 timeout 表示 UI 执行前已成功取消。
            "timeout" => WindowActionFailure::Transport(Failure::Timeout),
            "window_not_found" | "stale_window" => WindowActionFailure::Transport(Failure::Stale),
            // 请求发送后应用关闭可能正是本次 close 的结果，不能降格为业务前 stale。
            "app_closed" => WindowActionFailure::OutcomeUnknown,
            "stale_revision" => WindowActionFailure::StaleRevision,
            "unsupported_action" => WindowActionFailure::UnsupportedAction,
            "forbidden" => WindowActionFailure::Forbidden,
            "not_presentable" => WindowActionFailure::NotPresentable,
            "not_interactable" => WindowActionFailure::NotInteractable,
            "window_operation_failed" => WindowActionFailure::WindowOperationFailed,
            "did_not_settle" => WindowActionFailure::DidNotSettle,
            "outcome_unknown" => WindowActionFailure::OutcomeUnknown,
            _ => WindowActionFailure::Transport(Failure::Protocol),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        thread,
    };

    use super::*;

    #[test]
    fn activation_uses_negotiated_action_and_same_connection_focus_observation() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for expected in ["perform", "list_windows"] {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != expected {
                    return Err("测试请求类型不匹配".to_owned());
                }
                let reply = if expected == "perform" {
                    if request["action"]["kind"] != "activate_window" {
                        return Err("激活请求必须使用 activate_window".to_owned());
                    }
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "perform",
                        "window_id": 7,
                        "generation": 3,
                        "revision": 6,
                        "presented_revision": 6,
                        "settled": true
                    })
                } else {
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "list_windows",
                        "windows": [{
                            "window_id": 7,
                            "generation": 3,
                            "title": "fixture",
                            "visible": true,
                            "presentable": true,
                            "focused": true,
                            "revision": 6,
                            "presented_revision": 6,
                            "closed": false
                        }]
                    })
                };
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });

        let descriptor = super::super::EndpointDescriptor {
            schema: super::super::PROTOCOL_SCHEMA.to_owned(),
            process_id: std::process::id(),
            endpoint: "/fixture/agent.sock".to_owned(),
            token: "7".repeat(64),
            state: "ready".to_owned(),
        };
        let original = super::super::WireWindow {
            window_id: 7,
            generation: 3,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            logical_width: None,
            logical_height: None,
            maximized: None,
            minimized: None,
            fullscreen: None,
            focused: false.into(),
            revision: 5,
            presented_revision: 5,
            closed: false,
        };
        let target = window_session_id(&descriptor, &original);
        let window = super::super::WindowRecord {
            session_id: target.clone(),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: Some(false),
            revision: 5,
            presented_revision: 5,
            state: None,
            screenshot_supported: false,
            activation_supported: true,
            pointer_drag_supported: false,
            endpoint: descriptor,
            window_id: 7,
            generation: 3,
        };
        let mut client = AgentClient {
            reader: BufReader::new(client_stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["list_windows".to_owned(), "perform".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from(["activate_window".to_owned()]),
            window_state_fields: BTreeSet::from(["focused".to_owned()]),
            key_names: BTreeSet::new(),
            key_modifiers: BTreeSet::new(),
            screenshot_limits: None,
        };
        let Ok(outcome) = client.perform_activation(&window, &target) else {
            panic!("测试激活必须成功");
        };
        assert_eq!(outcome.focus_observed_after_dispatch, Some(true));
        assert_eq!(outcome.target_generation_current_after_dispatch, Some(true));
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn post_send_transport_and_partial_key_failure_never_become_retryable() {
        assert_eq!(
            window_request_failure(RequestFailure::Transport(Failure::Timeout)),
            WindowActionFailure::OutcomeUnknown
        );
        assert_eq!(
            window_request_failure(RequestFailure::Remote {
                code: "not_interactable".to_owned(),
                confirm_id: None,
            }),
            WindowActionFailure::NotInteractable
        );
        assert_eq!(
            window_request_failure(RequestFailure::Remote {
                code: "app_closed".to_owned(),
                confirm_id: None,
            }),
            WindowActionFailure::OutcomeUnknown
        );
    }
}
