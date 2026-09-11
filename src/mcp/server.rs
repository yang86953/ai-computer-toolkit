//! MCP stdio JSON-RPC 服务。
//!
//! JSONL 读取与唯一执行 owner 分离；并发工具调用按 BUSY 拒绝，取消只绑定正在执行的请求。

use crate::components::{
    cancellation, desktop_session_input_cancellation::DesktopInputCancellation,
};
use std::{
    io::{BufRead, Read, Write},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use serde_json::{Value, json};

use super::desktop::Desktop;
use super::tools::tool_catalog;

/// 支持的 MCP 协议版本；未声明版本时回落到最新版。
const SUPPORTED_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// 单行请求上限。
const MAXIMUM_LINE_BYTES: usize = 1024 * 1024;

/// 服务端身份。
const SERVER_NAME: &str = "computer-control-toolkit";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 面向模型的固定引导语。
const INSTRUCTIONS: &str = "Use computer_connect, computer_observe, then the direct keyboard/mouse tools. For long deterministic sequences prefer computer_run: it refreshes the frame between batches on the server, so one call can drive many batches instead of one round trip per batch. Confirm the target and focus from returned screenshots. Connect with authorizationMode=session to cover the whole session with one explicit confirmation; per-call confirmation fields may then be omitted. On Linux, rememberAuthorization=true reuses the saved Portal authorization across connections (view or revoke it with computer_authorization). There is no implicit authorization and no automatic input replay.";

/// 读取线程只做帧、BUSY 和取消路由；唯一 owner 顺序执行桌面操作。
pub fn run_stdio() -> i32 {
    let writer = Arc::new(Mutex::new(std::io::stdout()));
    let active: Arc<Mutex<Option<(Value, DesktopInputCancellation)>>> = Arc::new(Mutex::new(None));
    let (sender, receiver) = mpsc::sync_channel(16);
    let reader_active = active.clone();
    let reader_writer = writer.clone();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut line = Vec::new();
            match reader
                .by_ref()
                .take((MAXIMUM_LINE_BYTES + 1) as u64)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => break,
                Ok(_) if line.len() <= MAXIMUM_LINE_BYTES && line.ends_with(b"\n") => {}
                _ => {
                    let _ = emit(
                        &mut *reader_writer.lock().unwrap(),
                        error_response(None, -32600, "Invalid or oversized request frame"),
                    );
                    break;
                }
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let request: Value = match serde_json::from_slice(&line) {
                Ok(value) => value,
                Err(_) => {
                    let _ = emit(
                        &mut *reader_writer.lock().unwrap(),
                        error_response(None, -32700, "Parse error"),
                    );
                    continue;
                }
            };
            let mut token = None;
            if request["jsonrpc"] == "2.0" {
                if request.get("id").is_none()
                    && matches!(
                        request["method"].as_str(),
                        Some("notifications/cancelled" | "$/cancelRequest")
                    )
                {
                    if let Some((id, cancel)) = reader_active.lock().unwrap().as_ref() {
                        if request["params"]["requestId"] == *id {
                            cancel.cancel();
                        }
                    }
                    continue;
                }
                if request["method"] == "tools/call"
                    && request
                        .get("id")
                        .is_some_and(|v| v.is_string() || v.is_number())
                {
                    let mut current = reader_active.lock().unwrap();
                    if current.is_some() {
                        let _ = emit(
                            &mut *reader_writer.lock().unwrap(),
                            result_response(
                                request["id"].clone(),
                                json!({"isError":true,"content":[{"type":"text","text":"BUSY: no operation queued; wait for the current result before deciding the next action."}]}),
                            ),
                        );
                        continue;
                    }
                    let cancel = DesktopInputCancellation::new();
                    *current = Some((request["id"].clone(), cancel.clone()));
                    token = Some(cancel);
                }
            }
            if sender.try_send((request, token)).is_err() {
                break;
            }
        }
        // EOF、管道错误与超大帧都取消正在执行的输入，让 owner 释放会话。
        if let Some((_, token)) = reader_active.lock().unwrap().as_ref() {
            token.cancel();
        }
    });
    let mut session = Session::new();
    loop {
        if cancellation::is_cancelled() {
            if let Some((_, token)) = active.lock().unwrap().as_ref() {
                token.cancel();
            }
            break;
        }
        let (request, token) = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        if let Some(cancel) = &token {
            session.desktop.set_cancellation(cancel.clone());
        }
        let responses = session.handle(&request);
        if token.is_some() {
            *active.lock().unwrap() = None;
        }
        for response in responses {
            if emit(&mut *writer.lock().unwrap(), response).is_err() {
                session.dispose();
                return 0;
            }
        }
    }
    session.dispose();
    0
}

/// 写出一行响应；管道关闭时返回错误以便调用方收尾。
fn emit(writer: &mut impl Write, value: Value) -> std::io::Result<()> {
    writeln!(writer, "{value}")?;
    writer.flush()
}

/// 构造错误响应。
fn error_response(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// 构造成功响应。
fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 一个 MCP 会话的服务端状态。
pub(crate) struct Session {
    desktop: Desktop,
    initialized: bool,
    ready: bool,
}

impl Session {
    pub(crate) fn new() -> Self {
        // 桌面容器立即可用；broker 与私有捕获目录都按需建立，
        // initialize、tools/list 与 computer_status 不触达桌面或文件系统。
        Self {
            desktop: Desktop::new(),
            initialized: false,
            ready: false,
        }
    }

    /// 测试注入：替换捕获目录父路径，经真实 tools/call 边界驱动目录初始化失败。
    #[cfg(test)]
    pub(crate) fn set_capture_parent_for_tests(&mut self, parent: std::path::PathBuf) {
        self.desktop.set_capture_parent_for_tests(parent);
    }

    /// 处理一个请求，返回需要写出的零个或多个响应。
    pub(crate) fn handle(&mut self, request: &Value) -> Vec<Value> {
        let Some(object) = request.as_object() else {
            return vec![error_response(None, -32600, "Invalid request")];
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || !object.get("method").is_some_and(Value::is_string)
        {
            return vec![error_response(None, -32600, "Invalid request")];
        }
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let params = object.get("params").cloned().unwrap_or(json!({}));
        let has_id = object.contains_key("id");
        let id = object.get("id").cloned();

        // 通知（无 id）不产生响应。
        if !has_id {
            match method.as_str() {
                "notifications/initialized" if self.initialized => self.ready = true,
                _ => {}
            }
            return Vec::new();
        }
        let Some(id) = id.filter(|value| value.is_string() || value.is_number()) else {
            return vec![error_response(None, -32600, "Invalid request id")];
        };
        if !params.is_object() {
            return vec![error_response(Some(id), -32602, "Expected object params")];
        }

        match method.as_str() {
            "initialize" => {
                if self.initialized {
                    return vec![error_response(Some(id), -32600, "Already initialized")];
                }
                self.initialized = true;
                let requested = params.get("protocolVersion").and_then(Value::as_str);
                let version = requested
                    .filter(|version| SUPPORTED_VERSIONS.contains(version))
                    .unwrap_or(SUPPORTED_VERSIONS[SUPPORTED_VERSIONS.len() - 1]);
                vec![result_response(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                        "instructions": INSTRUCTIONS,
                    }),
                )]
            }
            "ping" => vec![result_response(id, json!({}))],
            _ if !self.ready => vec![error_response(Some(id), -32002, "Initialize first")],
            "tools/list" => vec![result_response(id, json!({ "tools": tool_catalog() }))],
            "tools/call" => self.call_tool(id, &params),
            _ => vec![error_response(Some(id), -32601, "Method not found")],
        }
    }

    /// 执行一次工具调用，把失败表达为工具层错误而非协议错误。
    fn call_tool(&mut self, id: Value, params: &Value) -> Vec<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        if !super::tools::is_known_tool(&name) {
            return vec![result_response(
                id,
                json!({
                    "isError": true,
                    "content": [{
                        "type": "text",
                        "text": "INVALID_ARGUMENT: unknown tool; call tools/list for the closed catalog."
                    }],
                }),
            )];
        }
        let outcome = self.desktop.call(&name, &arguments);
        vec![result_response(
            id,
            json!({ "isError": outcome.is_error, "content": outcome.content }),
        )]
    }

    /// 释放桌面会话与临时资源。
    pub(crate) fn dispose(&mut self) {
        self.desktop.dispose();
    }
}
