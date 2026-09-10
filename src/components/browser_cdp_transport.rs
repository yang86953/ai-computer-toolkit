//! 提供只连接 IPv4 回环且方法集合封闭的 WebSocket/CDP 传输。

// 导入回环网络、随机掩码、时间与 I/O 工具。
use std::{
    // 导入同步读写。
    io::{Read, Write},
    // 导入固定 IPv4 回环连接。
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    // 导入 deadline 工具。
    time::{Duration, Instant},
};

// 导入 Base64 编码器。
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入 SHA-1 摘要接口。
use sha1::{Digest, Sha1};

// 导入统一错误和安全随机 nonce。
use crate::{
    // 导入随机源。
    components::secure_nonce_windows::random_nonce,
    // 导入统一错误。
    domain::{AppControlError, AppResult},
};

// 加载私有 WebSocket 帧编解码组件。
#[path = "browser_cdp_transport_frame.rs"]
mod frame;

// 导入窄帧读写入口。
use frame::{read_message, write_frame};

// 固定 WebSocket RFC GUID。
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
// 固定升级响应头上限。
const MAXIMUM_HANDSHAKE_BYTES: usize = 16 * 1024;
// 固定单个 WebSocket/CDP 消息上限。
const MAXIMUM_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
// 固定 browser websocket path token 上限。
const MAXIMUM_PATH_BYTES: usize = 512;
// 固定控制帧 payload 上限。
const MAXIMUM_CONTROL_BYTES: usize = 125;

// 表示私有实现允许发送的 CDP 方法。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CdpMethod {
    // 创建工具自有空白页面 target。
    TargetCreateTarget,
    // 以 flatten 模式附着工具 target。
    TargetAttachToTarget,
    // 启用 Page domain。
    PageEnable,
    // 启用 DOM domain。
    DomEnable,
    // 启用 Accessibility domain。
    AccessibilityEnable,
    // 读取固定文档 readyState。
    RuntimeEvaluate,
    // 导航页面。
    PageNavigate,
    // 读取文档根。
    DomGetDocument,
    // 读取完整语义树。
    AccessibilityGetFullAxTree,
    // 读取元素盒模型。
    DomGetBoxModel,
    // 聚焦元素。
    DomFocus,
    // 派发鼠标事件。
    InputDispatchMouseEvent,
    // 插入 UTF-8 文本。
    InputInsertText,
    // 派发固定键盘事件。
    InputDispatchKeyEvent,
    // 捕获 PNG 页面截图。
    PageCaptureScreenshot,
}

// 把封闭方法映射为固定 CDP 字符串。
impl CdpMethod {
    // 返回 worker 内部唯一方法文本。
    const fn as_str(self) -> &'static str {
        // 穷举全部允许方法。
        match self {
            // 映射 target 创建。
            Self::TargetCreateTarget => "Target.createTarget",
            // 映射 target 附着。
            Self::TargetAttachToTarget => "Target.attachToTarget",
            // 映射 Page 启用。
            Self::PageEnable => "Page.enable",
            // 映射 DOM 启用。
            Self::DomEnable => "DOM.enable",
            // 映射 Accessibility 启用。
            Self::AccessibilityEnable => "Accessibility.enable",
            // 映射固定文档状态读取。
            Self::RuntimeEvaluate => "Runtime.evaluate",
            // 映射导航。
            Self::PageNavigate => "Page.navigate",
            // 映射文档读取。
            Self::DomGetDocument => "DOM.getDocument",
            // 映射语义树读取。
            Self::AccessibilityGetFullAxTree => "Accessibility.getFullAXTree",
            // 映射盒模型读取。
            Self::DomGetBoxModel => "DOM.getBoxModel",
            // 映射聚焦。
            Self::DomFocus => "DOM.focus",
            // 映射鼠标事件。
            Self::InputDispatchMouseEvent => "Input.dispatchMouseEvent",
            // 映射文本输入。
            Self::InputInsertText => "Input.insertText",
            // 映射键盘事件。
            Self::InputDispatchKeyEvent => "Input.dispatchKeyEvent",
            // 映射截图。
            Self::PageCaptureScreenshot => "Page.captureScreenshot",
        }
    }
}

// 保存一个严格握手后的 loopback CDP 连接。
pub(crate) struct CdpConnection {
    // 保存唯一 TCP 流。
    stream: TcpStream,
    // 保存下一请求 ID。
    next_id: u64,
    // 保存可选 flatten session ID，仅在 worker 内使用。
    session_id: Option<String>,
}

// 保存只允许中断 I/O 的窄 CDP 取消句柄。
pub(crate) struct CdpCancellationHandle {
    // 保存同一 socket 的克隆句柄。
    stream: TcpStream,
}

// 为窄取消句柄提供不可恢复 shutdown。
impl CdpCancellationHandle {
    // 中断所有在途和后续 socket I/O。
    pub(crate) fn cancel(self) {
        // 关闭双向 socket 使阻塞 read/write 立即返回。
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

// 为私有 CDP 连接提供固定方法调用。
impl CdpConnection {
    // 复制只用于取消的 socket 句柄。
    pub(crate) fn cancellation_handle(&self) -> AppResult<CdpCancellationHandle> {
        // 克隆同一内核 socket 句柄。
        let stream = self.stream.try_clone().map_err(|_| {
            // 克隆失败时不泄漏 OS 错误。
            cdp_error(
                // 使用稳定资源错误。
                "BROWSER_PROTOCOL_FAILED",
                // 输出安全诊断。
                "The browser protocol cancellation handle could not be created.",
            )
        })?;
        // 返回只有 shutdown 能力的窄句柄。
        Ok(CdpCancellationHandle { stream })
    }

    // 连接一个由 worker 从 DevToolsActivePort 验证的 IPv4 回环端点。
    pub(crate) fn connect(port: u16, path: &str, timeout: Duration) -> AppResult<Self> {
        // 端口、路径和 deadline 必须在固定边界内。
        if port == 0
            // deadline 必须正值有界。
            || timeout.is_zero()
            // 限制单次传输建立预算。
            || timeout > Duration::from_secs(30)
            // 路径必须是 canonical browser websocket。
            || !valid_browser_path(path)
        {
            // 返回稳定参数失败。
            return Err(cdp_error(
                // 使用参数类别。
                "INVALID_ARGUMENT",
                // 不回显端点。
                "The private browser protocol endpoint is invalid.",
            ));
        }
        // 从握手前建立唯一总期限。
        let deadline = Instant::now() + timeout;
        // 只连接 IPv4 回环地址。
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        // 建立有界连接。
        let mut stream = TcpStream::connect_timeout(&address.into(), timeout).map_err(|_| {
            // 不公开端口或 OS 错误。
            cdp_error(
                // 使用连接失败类别。
                "BROWSER_PROTOCOL_UNAVAILABLE",
                // 输出安全诊断。
                "The isolated browser protocol endpoint could not be reached.",
            )
        })?;
        // 安装剩余 deadline。
        install_timeout(&stream, deadline)?;
        // 生成 16 字节随机握手 key 的十六进制来源。
        let nonce = random_nonce()?;
        // 把 canonical hex 转换为 16 字节。
        let key_bytes = decode_nonce(&nonce)?;
        // 编码 WebSocket key。
        let key = BASE64_STANDARD.encode(key_bytes);
        // 构造固定升级请求。
        let request = format!(
            // 禁止额外 headers、cookie 或子协议。
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        // 写入完整握手。
        stream.write_all(request.as_bytes()).map_err(|_| {
            // 映射写入失败。
            cdp_error(
                // 使用断开类别。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 不回显请求。
                "The isolated browser protocol handshake could not be sent.",
            )
        })?;
        // 读取到 HTTP 头终止符。
        let response = read_http_headers(&mut stream, deadline)?;
        // 验证严格升级事实。
        validate_handshake(&response, &key)?;
        // 返回已认证连接。
        Ok(Self {
            // 保存流。
            stream,
            // CDP ID 从一开始。
            next_id: 1,
            // 初始尚未附着 target session。
            session_id: None,
        })
    }

    // 保存 flatten target session ID。
    pub(crate) fn set_session_id(&mut self, session_id: String) -> AppResult<()> {
        // session ID 只能是有界 ASCII token。
        if session_id.is_empty()
            // 限制长度。
            || session_id.len() > 256
            // 限制字符集。
            || !session_id
                // 遍历字节。
                .bytes()
                // 只接受 Chromium token 安全集合。
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            // 拒绝原生身份漂移。
            return Err(cdp_error(
                // 使用协议失败。
                "BROWSER_PROTOCOL_FAILED",
                // 不回显 token。
                "The browser target session identity is invalid.",
            ));
        }
        // 保存 worker 私有 session ID。
        self.session_id = Some(session_id);
        // 返回成功。
        Ok(())
    }

    // 调用一个封闭 CDP 方法并等待同 ID 响应。
    pub(crate) fn call(
        &mut self,
        // 接收封闭方法枚举。
        method: CdpMethod,
        // 接收 worker 内部构造的参数。
        params: Value,
        // 接收不重置的绝对 deadline。
        deadline: Instant,
    ) -> AppResult<Value> {
        // deadline 必须仍有剩余预算。
        if Instant::now() >= deadline {
            // 返回稳定超时。
            return Err(cdp_error(
                // 使用 deadline 类别。
                "DEADLINE_EXCEEDED",
                // 输出安全诊断。
                "The browser protocol command exceeded its deadline.",
            ));
        }
        // 参数必须是 JSON 对象。
        if !params.is_object() {
            // 防止通用负载穿透。
            return Err(cdp_error(
                // 使用参数类别。
                "INVALID_ARGUMENT",
                // 输出固定诊断。
                "The private browser protocol parameters must be an object.",
            ));
        }
        // 分配唯一有界请求 ID。
        let id = self.next_id;
        // 推进下一 ID 并拒绝溢出复用。
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| {
            // 返回协议资源失败。
            cdp_error(
                // 使用资源类别。
                "BROWSER_PROTOCOL_FAILED",
                // 输出固定诊断。
                "The browser protocol request identity space was exhausted.",
            )
        })?;
        // 构造固定 CDP request。
        let mut request = json!({
            // 使用 parent 分配 ID。
            "id": id,
            // 使用封闭方法文本。
            "method": method.as_str(),
            // 使用 worker 内部参数。
            "params": params,
        });
        // flatten target 调用携带私有 session ID。
        if let Some(session_id) = self.session_id.as_ref() {
            // 插入固定 sessionId 字段。
            request
                // 取得已知对象。
                .as_object_mut()
                // request 构造固定为对象。
                .ok_or_else(|| {
                    cdp_error("BROWSER_PROTOCOL_FAILED", "The CDP request shape drifted.")
                })?
                // 插入私有 target session。
                .insert("sessionId".to_owned(), Value::String(session_id.clone()));
        }
        // 序列化有界请求。
        let bytes = serde_json::to_vec(&request).map_err(|_| {
            // 返回协议失败。
            cdp_error(
                // 使用协议类别。
                "BROWSER_PROTOCOL_FAILED",
                // 不回显参数。
                "The browser protocol request could not be serialized.",
            )
        })?;
        // 请求不得超过消息上限。
        if bytes.len() > MAXIMUM_MESSAGE_BYTES {
            // 返回资源失败。
            return Err(cdp_error(
                // 使用输出同类资源码。
                "WORKER_REQUEST_TOO_LARGE",
                // 输出安全诊断。
                "The browser protocol request exceeded its resource boundary.",
            ));
        }
        // 安装剩余 I/O deadline。
        install_timeout(&self.stream, deadline)?;
        // 发送 masked text frame。
        write_frame(&mut self.stream, 0x1, &bytes)?;
        // 持续读取直到同 ID response，跳过有界事件。
        loop {
            // 安装更新后的剩余 deadline。
            install_timeout(&self.stream, deadline)?;
            // 读取一条完整消息。
            let message = read_message(&mut self.stream)?;
            // 严格解析 JSON。
            let value = serde_json::from_slice::<Value>(&message).map_err(|_| {
                // 非 JSON 响应失败闭合。
                cdp_error(
                    // 使用协议失败。
                    "BROWSER_PROTOCOL_FAILED",
                    // 不回显响应。
                    "The browser protocol response was not valid JSON.",
                )
            })?;
            // 无 id 的事件只在当前 deadline 内跳过。
            let Some(response_id) = value.get("id").and_then(Value::as_u64) else {
                // 事件必须至少有有界 method 字符串。
                if !value
                    // 读取 method。
                    .get("method")
                    // 转换字符串。
                    .and_then(Value::as_str)
                    // 限制事件 method 长度。
                    .is_some_and(|method| !method.is_empty() && method.len() <= 128)
                {
                    // 非事件响应失败。
                    return Err(cdp_error(
                        // 使用协议类别。
                        "BROWSER_PROTOCOL_FAILED",
                        // 不回显响应。
                        "The browser protocol message had no valid correlation.",
                    ));
                }
                // 继续等待当前请求响应。
                continue;
            };
            // 响应 ID 必须准确匹配当前请求。
            if response_id != id {
                // 拒绝交错或漂移响应。
                return Err(cdp_error(
                    // 使用协议类别。
                    "BROWSER_PROTOCOL_FAILED",
                    // 不回显 ID。
                    "The browser protocol response correlation drifted.",
                ));
            }
            // CDP error 映射为结构化失败。
            if value.get("error").is_some() {
                // 不把 provider 原生错误穿透边界。
                return Err(cdp_error(
                    // 使用 provider 拒绝类别。
                    "BROWSER_PROTOCOL_REJECTED",
                    // 输出安全诊断。
                    "The browser protocol rejected the fixed page command.",
                ));
            }
            // result 必须是对象。
            let result = value
                .get("result")
                .filter(|result| result.is_object())
                .ok_or_else(|| {
                    // 缺失 result 失败闭合。
                    cdp_error(
                        // 使用协议类别。
                        "BROWSER_PROTOCOL_FAILED",
                        // 输出安全诊断。
                        "The browser protocol response did not contain a result object.",
                    )
                })?;
            // 返回独立 result 值。
            return Ok(result.clone());
        }
    }

    // 发送 WebSocket close 并关闭 TCP 写端。
    pub(crate) fn close(mut self) {
        // 尽力发送无负载 close 控制帧。
        let _ = write_frame(&mut self.stream, 0x8, &[]);
        // 关闭双向 socket。
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

// 安装基于绝对 deadline 的读写 timeout。
fn install_timeout(stream: &TcpStream, deadline: Instant) -> AppResult<()> {
    // 计算剩余预算。
    let remaining = deadline.saturating_duration_since(Instant::now());
    // 零预算立即超时。
    if remaining.is_zero() {
        // 返回 deadline 失败。
        return Err(cdp_error(
            // 使用稳定类别。
            "DEADLINE_EXCEEDED",
            // 输出固定诊断。
            "The browser protocol command exceeded its deadline.",
        ));
    }
    // 安装读取 timeout。
    stream.set_read_timeout(Some(remaining)).map_err(|_| {
        // 映射 socket 配置失败。
        cdp_error(
            // 使用连接失败类别。
            "BROWSER_PROTOCOL_FAILED",
            // 不回显 socket。
            "The browser protocol read deadline could not be configured.",
        )
    })?;
    // 安装写入 timeout。
    stream.set_write_timeout(Some(remaining)).map_err(|_| {
        // 映射 socket 配置失败。
        cdp_error(
            // 使用连接失败类别。
            "BROWSER_PROTOCOL_FAILED",
            // 不回显 socket。
            "The browser protocol write deadline could not be configured.",
        )
    })
}

// 读取有界 HTTP 升级响应头。
fn read_http_headers(stream: &mut TcpStream, deadline: Instant) -> AppResult<Vec<u8>> {
    // 保存累计响应。
    let mut response = Vec::new();
    // 使用单字节读取避免吃掉首个 WebSocket frame。
    let mut byte = [0_u8; 1];
    // 持续读取到双 CRLF。
    loop {
        // 更新剩余 deadline。
        install_timeout(stream, deadline)?;
        // 读取一个字节。
        stream.read_exact(&mut byte).map_err(|_| {
            // 映射断开。
            cdp_error(
                // 使用断开类别。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 输出安全诊断。
                "The browser protocol handshake ended before completion.",
            )
        })?;
        // 追加字节。
        response.push(byte[0]);
        // 超限时失败闭合。
        if response.len() > MAXIMUM_HANDSHAKE_BYTES {
            // 返回资源失败。
            return Err(cdp_error(
                // 使用输出资源码。
                "WORKER_OUTPUT_TOO_LARGE",
                // 输出固定诊断。
                "The browser protocol handshake exceeded its boundary.",
            ));
        }
        // 双 CRLF 表示 headers 完成。
        if response.ends_with(b"\r\n\r\n") {
            // 返回完整 headers。
            return Ok(response);
        }
    }
}

// 验证 WebSocket 升级响应。
fn validate_handshake(response: &[u8], key: &str) -> AppResult<()> {
    // 响应必须是 ASCII/UTF-8 headers。
    let text = std::str::from_utf8(response).map_err(|_| {
        // 返回协议失败。
        cdp_error(
            // 使用协议类别。
            "BROWSER_PROTOCOL_FAILED",
            // 不回显响应。
            "The browser protocol handshake was not valid text.",
        )
    })?;
    // 拆分 CRLF 行。
    let mut lines = text.split("\r\n");
    // 状态行必须严格 101。
    if lines.next() != Some("HTTP/1.1 101 Switching Protocols") {
        // 拒绝非升级响应。
        return Err(cdp_error(
            // 使用协议类别。
            "BROWSER_PROTOCOL_FAILED",
            // 输出安全诊断。
            "The browser protocol endpoint did not accept WebSocket upgrade.",
        ));
    }
    // 保存三个必需 header 事实。
    let mut upgrade = false;
    // 保存 Connection header 事实。
    let mut connection = false;
    // 保存 accept 值。
    let mut accept = None;
    // 遍历剩余非空 headers。
    for line in lines.filter(|line| !line.is_empty()) {
        // 分离 header 名和值。
        let Some((name, value)) = line.split_once(':') else {
            // 非法 header 失败。
            return Err(cdp_error(
                // 使用协议类别。
                "BROWSER_PROTOCOL_FAILED",
                // 输出固定诊断。
                "The browser protocol handshake header was malformed.",
            ));
        };
        // 去除 OWS。
        let value = value.trim();
        // 按不区分大小写 header 名记录事实。
        if name.eq_ignore_ascii_case("Upgrade") {
            // Upgrade 必须 websocket。
            upgrade = value.eq_ignore_ascii_case("websocket");
        } else if name.eq_ignore_ascii_case("Connection") {
            // Connection token 必须包含 upgrade。
            connection = value
                // 按逗号切分 tokens。
                .split(',')
                // 去除空白并忽略大小写。
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
        } else if name.eq_ignore_ascii_case("Sec-WebSocket-Accept") {
            // 只允许一个 accept header。
            if accept.replace(value).is_some() {
                // 重复 header 失败。
                return Err(cdp_error(
                    // 使用协议类别。
                    "BROWSER_PROTOCOL_FAILED",
                    // 输出固定诊断。
                    "The browser protocol handshake repeated its accept proof.",
                ));
            }
        }
    }
    // 计算期望 accept。
    let expected = websocket_accept(key);
    // 三项事实必须同时成立。
    if !upgrade || !connection || accept != Some(expected.as_str()) {
        // 拒绝伪造端点。
        return Err(cdp_error(
            // 使用认证失败类别。
            "BROWSER_PROTOCOL_AUTH_FAILED",
            // 不回显 accept。
            "The browser protocol WebSocket upgrade proof was invalid.",
        ));
    }
    // 握手可信。
    Ok(())
}

// 计算 RFC 6455 accept proof。
fn websocket_accept(key: &str) -> String {
    // 初始化 SHA-1。
    let mut hasher = Sha1::new();
    // 写入客户端 key。
    hasher.update(key.as_bytes());
    // 写入固定 GUID。
    hasher.update(WEBSOCKET_GUID.as_bytes());
    // Base64 编码 20 字节摘要。
    BASE64_STANDARD.encode(hasher.finalize())
}

// 验证 browser WebSocket path 形状。
fn valid_browser_path(path: &str) -> bool {
    // 只接受固定前缀后的有界 token。
    path.len() <= MAXIMUM_PATH_BYTES
        // 移除固定路径前缀。
        && path
            // 只允许 browser endpoint。
            .strip_prefix("/devtools/browser/")
            // 验证 token。
            .is_some_and(|token| {
                // token 必须非空。
                !token.is_empty()
                    // 只允许安全 ASCII。
                    && token
                        // 遍历字节。
                        .bytes()
                        // 限制字符集。
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
}

// 解码 canonical 32 hex nonce 为 16 字节。
fn decode_nonce(nonce: &str) -> AppResult<[u8; 16]> {
    // nonce 必须精确 32 字节。
    if nonce.len() != 32 {
        // 返回随机源编码失败。
        return Err(cdp_error(
            // 使用协议类别。
            "BROWSER_PROTOCOL_FAILED",
            // 输出固定诊断。
            "The browser protocol random nonce was invalid.",
        ));
    }
    // 初始化输出。
    let mut bytes = [0_u8; 16];
    // 逐个字节解析两个 hex 字符。
    for (index, slot) in bytes.iter_mut().enumerate() {
        // 计算字符偏移。
        let offset = index.saturating_mul(2);
        // 解析固定两字符切片。
        *slot = u8::from_str_radix(&nonce[offset..offset.saturating_add(2)], 16).map_err(|_| {
            // 返回安全失败。
            cdp_error(
                // 使用协议类别。
                "BROWSER_PROTOCOL_FAILED",
                // 不回显 nonce。
                "The browser protocol random nonce was invalid.",
            )
        })?;
    }
    // 返回随机字节。
    Ok(bytes)
}

// 构造固定断开错误。
fn disconnected() -> AppControlError {
    // 返回不含 endpoint 的断开诊断。
    cdp_error(
        // 使用稳定断开码。
        "BROWSER_PROTOCOL_DISCONNECTED",
        // 输出安全消息。
        "The isolated browser protocol connection closed unexpectedly.",
    )
}

// 构造固定 frame 协议错误。
fn protocol_frame_error() -> AppControlError {
    // 返回不含 frame 内容的失败。
    cdp_error(
        // 使用稳定协议码。
        "BROWSER_PROTOCOL_FAILED",
        // 输出安全消息。
        "The browser protocol WebSocket frame violated RFC 6455 boundaries.",
    )
}

// 构造统一私有 CDP 错误。
fn cdp_error(code: &'static str, message: &'static str) -> AppControlError {
    // 使用产品级错误 envelope。
    AppControlError::new(code, message)
}

// 加载传输回归测试。
#[cfg(test)]
#[path = "browser_cdp_transport_tests.rs"]
mod tests;
