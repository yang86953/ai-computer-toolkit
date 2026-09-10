//! 固定浏览器会话 runtime 测试替身。

// 导入文件、回环监听、进程参数与等待工具。
use std::{
    // 导入私有事实文件写入。
    fs,
    // 导入最小 HTTP 与 WebSocket 读写。
    io::{Read, Write},
    // 导入回环 TCP listener 和 stream。
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    // 导入路径所有权。
    path::PathBuf,
    // 导入短轮询休眠。
    thread,
    // 导入固定等待时长。
    time::Duration,
};

// 导入 Base64 编码器。
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入 SHA-1 摘要接口。
use sha1::{Digest, Sha1};

// 固定 fixture browser websocket token。
const FIXTURE_TOKEN: &str = "fixture_browser_session";
// 固定 WebSocket RFC GUID。
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
// 固定握手请求上限。
const MAXIMUM_HANDSHAKE_BYTES: usize = 16 * 1024;
// 固定单帧请求上限。
const MAXIMUM_FRAME_BYTES: usize = 64 * 1024;
// 固定一像素 PNG 的标准 Base64。
const FIXTURE_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

// 从固定 Chromium 参数中提取工具自有 profile。
fn profile_path() -> Option<PathBuf> {
    // 遍历进程参数但不接受额外控制面。
    std::env::args().skip(1).find_map(|argument| {
        // 只识别生产 worker 安装的 user-data-dir 参数。
        argument
            // 移除固定参数前缀。
            .strip_prefix("--user-data-dir=")
            // 转换为测试私有路径。
            .map(PathBuf::from)
    })
}

// 处理一次固定 DevTools version 探测。
fn serve_probe(listener: &TcpListener, port: u16) -> bool {
    // 等待 worker 建立回环连接。
    let Ok((mut stream, _)) = listener.accept() else {
        // 监听失败终止 fixture。
        return false;
    };
    // 限制测试请求读取时长。
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    // 保存有界 HTTP 请求。
    let mut request = [0_u8; 2048];
    // 读取一次固定请求。
    let Ok(read) = stream.read(&mut request) else {
        // 请求读取失败。
        return false;
    };
    // 只接受 worker 固定 version 探测。
    if !request[..read].starts_with(b"GET /json/version HTTP/1.0\r\n") {
        // 拒绝其他测试请求。
        return false;
    }
    // 构造不含真实浏览器数据的固定响应体。
    let body = format!(
        // 回显与事实文件一致的 websocket 路径。
        "{{\"Browser\":\"ACT fixture\",\"webSocketDebuggerUrl\":\"ws://127.0.0.1:{port}/devtools/browser/{FIXTURE_TOKEN}\"}}"
    );
    // 构造确定性 HTTP 响应。
    let response = format!(
        // 使用关闭连接的 HTTP/1.0 响应。
        "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        // 注入准确 UTF-8 字节长度。
        body.len()
    );
    // 写入完整响应并确认 flush。
    stream.write_all(response.as_bytes()).is_ok() && stream.flush().is_ok()
}

// 服务固定 CDP 握手和 target 初始化序列。
fn serve_cdp(listener: &TcpListener, mode: Option<&str>) -> bool {
    // 等待 worker 的唯一 CDP 连接。
    let Ok((mut stream, peer)) = listener.accept() else {
        // 监听失败终止 fixture。
        return false;
    };
    // 客户端必须来自回环地址。
    if !peer.ip().is_loopback() {
        // 拒绝非回环连接。
        return false;
    }
    // 断开模式在协议握手前模拟 endpoint 消失。
    if mode == Some("cdp-disconnect") {
        // 关闭 socket 即完成故障注入。
        return true;
    }
    // 读取有界升级请求。
    let Some(request) = read_headers(&mut stream) else {
        // 无效握手终止 fixture。
        return false;
    };
    // 请求必须命中固定 browser websocket path。
    if !request.starts_with(&format!(
        // 只允许该测试 token。
        "GET /devtools/browser/{FIXTURE_TOKEN} HTTP/1.1\r\n"
    )) {
        // 拒绝路径漂移。
        return false;
    }
    // 提取唯一 WebSocket key。
    let Some(key) = request
        // 遍历全部 header 行。
        .split("\r\n")
        // 只读取标准 key header。
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
    else {
        // 缺失 key 时拒绝握手。
        return false;
    };
    // 计算 RFC 6455 握手证明。
    let accept = websocket_accept(key);
    // 构造严格升级响应。
    let response = format!(
        // 只返回传输验证所需 headers。
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    // 写入完整握手。
    if stream.write_all(response.as_bytes()).is_err() {
        // 写入失败终止 fixture。
        return false;
    }
    // 固定初始化方法序列。
    let methods = [
        // 先创建工具 target。
        "Target.createTarget",
        // 再附着 flatten session。
        "Target.attachToTarget",
        // 然后启用页面 domain。
        "Page.enable",
        // 启用 DOM domain。
        "DOM.enable",
        // 最后启用可访问性 domain。
        "Accessibility.enable",
    ];
    // 按冻结顺序处理五个初始化调用。
    for (index, expected_method) in methods.into_iter().enumerate() {
        // 读取一个 masked client text frame。
        let Some((0x1, payload)) = read_client_frame(&mut stream) else {
            // 缺失命令表示初始化未完成。
            return false;
        };
        // 严格解析 CDP 请求。
        let Ok(request) = serde_json::from_slice::<Value>(&payload) else {
            // 非 JSON 请求终止 fixture。
            return false;
        };
        // 方法必须严格符合固定序列。
        if request.get("method").and_then(Value::as_str) != Some(expected_method) {
            // 拒绝方法漂移。
            return false;
        }
        // 初始化 target 后的调用必须携带私有 flatten session。
        if index >= 2
            // 核对固定 session token。
            && request.get("sessionId").and_then(Value::as_str) != Some("fixture_session")
        {
            // 拒绝未绑定的页面 domain 调用。
            return false;
        }
        // 读取关联请求 ID。
        let Some(id) = request.get("id").and_then(Value::as_u64) else {
            // 缺失 ID 时无法关联响应。
            return false;
        };
        // 为当前固定方法构造最小结果。
        let result = match index {
            // 创建返回私有 target token。
            0 => json!({ "targetId": "fixture_target" }),
            // 附着返回私有 session token。
            1 => json!({ "sessionId": "fixture_session" }),
            // domain enable 返回空对象。
            _ => json!({}),
        };
        // 序列化关联响应。
        let Ok(response) = serde_json::to_vec(&json!({ "id": id, "result": result })) else {
            // 固定值序列化失败终止 fixture。
            return false;
        };
        // 写入 unmasked server text frame。
        if !write_server_text(&mut stream, &response) {
            // 写入失败终止 fixture。
            return false;
        }
    }
    // 初始化完成后持有连接直至 worker 关闭。
    while let Some((opcode, payload)) = read_client_frame(&mut stream) {
        // close frame 结束固定 runtime。
        if opcode == 0x8 {
            // 正常结束连接持有。
            break;
        }
        // 页面命令必须使用 text frame。
        if opcode != 0x1 {
            // 拒绝其他数据帧。
            return false;
        }
        // 严格解析固定页面 CDP 请求。
        let Ok(request) = serde_json::from_slice::<Value>(&payload) else {
            // 非 JSON 请求终止 fixture。
            return false;
        };
        // 页面调用必须绑定 flatten session。
        if request.get("sessionId").and_then(Value::as_str) != Some("fixture_session") {
            // 拒绝未绑定页面调用。
            return false;
        }
        // 读取关联请求 ID。
        let Some(id) = request.get("id").and_then(Value::as_u64) else {
            // 缺失 ID 时无法关联响应。
            return false;
        };
        // 测试显式要求时在受测页面动作已经越过 worker accepted 后发布进程外见证。
        if matches!(
            // 按封闭故障模式选择唯一可取消的 CDP 阶段。
            (mode, request.get("method").and_then(Value::as_str)),
            // 既有页面导航取消只接受对应延迟模式。
            (Some("page-delay"), Some("Page.navigate"))
                // 新增 click 取消只在盒模型请求已经派发后见证。
                | (Some("click-delay"), Some("DOM.getBoxModel"))
        )
            // 只接受测试进程环境提供的私有 marker 路径。
            && let Some(marker) = std::env::var_os(
                // 使用不会进入生产控制面的固定测试环境变量。
                "ACT_BROWSER_SESSION_RUNTIME_FIXTURE_ACCEPTED_MARKER",
            )
        {
            // marker 写入失败只使上层测试等待失败，不改变 runtime 响应语义。
            let _ = fs::write(marker, []);
        }
        // 断开模式在受测 CDP 请求已接收后关闭 socket 而不返回响应。
        if matches!(
            // 按封闭模式和公开动作唯一物理阶段配对。
            (mode, request.get("method").and_then(Value::as_str)),
            // 导航断开保持既有语义。
            (Some("page-disconnect"), Some("Page.navigate"))
                // click 在盒模型阶段断开。
                | (Some("click-disconnect"), Some("DOM.getBoxModel"))
                // type 在聚焦阶段断开。
                | (Some("type-disconnect"), Some("DOM.focus"))
                // screenshot 在捕获阶段断开。
                | (Some("screenshot-disconnect"), Some("Page.captureScreenshot"))
        ) {
            // 返回成功使作用域关闭 stream。
            return true;
        }
        // 延迟模式让总 deadline 或 cancel-command 在动作响应前到达。
        if matches!(
            // 按封闭模式和公开动作唯一物理阶段配对。
            (mode, request.get("method").and_then(Value::as_str)),
            // 导航延迟保持既有语义。
            (Some("page-delay"), Some("Page.navigate"))
                // click 在盒模型阶段延迟。
                | (Some("click-delay"), Some("DOM.getBoxModel"))
                // type 在聚焦阶段延迟。
                | (Some("type-delay"), Some("DOM.focus"))
                // screenshot 在捕获阶段延迟。
                | (Some("screenshot-delay"), Some("Page.captureScreenshot"))
        ) {
            // 保持 socket 阻塞直到取消或测试预算。
            thread::sleep(Duration::from_secs(2));
        }
        // 确定失败模式返回有 ID 的 CDP error，验证 worker 形成可信 failed final。
        let inject_failure = matches!(
            // 按当前测试模式与固定方法联合判定。
            (
                // 读取唯一故障模式。
                mode,
                // 读取当前封闭 CDP 方法。
                request.get("method").and_then(Value::as_str),
            ),
            // 页面导航失败用于验证 mutation Command 的公开失败真值。
            (Some("page-failed"), Some("Page.navigate"))
                // 页面查询失败用于验证只读 Query 的公开失败真值。
                | (Some("query-failed"), Some("Accessibility.getFullAXTree"))
                // click 在首个有副作用前的盒模型阶段形成可信失败。
                | (Some("click-failed"), Some("DOM.getBoxModel"))
                // type 在聚焦阶段形成可信失败。
                | (Some("type-failed"), Some("DOM.focus"))
                // screenshot 在唯一捕获阶段形成可信失败。
                | (Some("screenshot-failed"), Some("Page.captureScreenshot"))
        );
        // 只在显式测试模式下返回 provider rejection。
        if inject_failure {
            // 构造不会被生产边界公开的私有 CDP error。
            let Ok(response) = serde_json::to_vec(&json!({
                // 关联当前固定请求。
                "id": id,
                // 返回最小 provider rejection 形状。
                "error": {
                    // 使用固定私有错误号。
                    "code": -32_000,
                    // 使用不应越过 worker 边界的测试说明。
                    "message": "fixture page failure",
                },
            })) else {
                // 固定值序列化失败终止 fixture。
                return false;
            };
            // 写入关联 error frame。
            if !write_server_text(&mut stream, &response) {
                // 写入失败终止 fixture。
                return false;
            }
            // worker 取得确定失败后，runtime 保持 live 供显式 close。
            continue;
        }
        // 按封闭方法构造确定性结果。
        let result = match request.get("method").and_then(Value::as_str) {
            // 导航只接受固定测试 URL。
            Some("Page.navigate")
                if request.pointer("/params/url").and_then(Value::as_str)
                    == Some("https://example.test/page") =>
            {
                // 返回私有 frame ID。
                json!({ "frameId": "fixture_frame" })
            }
            // 固定 readyState 表达式返回 complete。
            Some("Runtime.evaluate")
                if request
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    == Some("document.readyState") =>
            {
                // 返回 by-value 字符串。
                json!({ "result": { "type": "string", "value": "complete" } })
            }
            // 可访问性查询返回固定多命中与文本节点。
            Some("Accessibility.getFullAXTree") => json!({
                // 返回固定语义节点。
                "nodes": [
                    // 第一个可用提交按钮。
                    {
                        // 保存私有 AX node ID。
                        "nodeId": "fixture_ax_1",
                        // 保存私有 backend DOM ID。
                        "backendDOMNodeId": 42,
                        // 节点可参与查询。
                        "ignored": false,
                        // 返回按钮 role。
                        "role": { "type": "role", "value": "button" },
                        // 返回按钮名称。
                        "name": { "type": "computedString", "value": "Submit" },
                        // 没有 disabled 属性。
                        "properties": [],
                    },
                    // 第二个同名禁用按钮用于多命中。
                    {
                        // 保存私有 AX node ID。
                        "nodeId": "fixture_ax_2",
                        // 保存私有 backend DOM ID。
                        "backendDOMNodeId": 43,
                        // 节点可参与查询。
                        "ignored": false,
                        // 返回按钮 role。
                        "role": { "type": "role", "value": "button" },
                        // 返回按钮名称。
                        "name": { "type": "computedString", "value": "Submit" },
                        // 标记禁用属性。
                        "properties": [
                            // 使用标准 AX disabled 属性。
                            { "name": "disabled", "value": { "type": "boolean", "value": true } },
                        ],
                    },
                    // 文本节点用于 text-present。
                    {
                        // 保存私有 AX node ID。
                        "nodeId": "fixture_ax_3",
                        // 保存私有 backend DOM ID。
                        "backendDOMNodeId": 44,
                        // 节点可参与查询。
                        "ignored": false,
                        // 返回文本 role。
                        "role": { "type": "role", "value": "StaticText" },
                        // 返回可见文本。
                        "name": { "type": "computedString", "value": "Welcome Home" },
                        // 没有禁用属性。
                        "properties": [],
                    },
                ],
            }),
            // 盒模型只接受 query 签发的首个私有映射。
            Some("DOM.getBoxModel")
                if request
                    .pointer("/params/backendNodeId")
                    .and_then(Value::as_u64)
                    == Some(42) =>
            {
                // 返回固定十乘十内容矩形。
                json!({ "model": { "content": [10, 5, 20, 5, 20, 15, 10, 15] } })
            }
            // 点击事件必须命中固定中心。
            Some("Input.dispatchMouseEvent")
                if request.pointer("/params/x").and_then(Value::as_f64) == Some(15.0)
                    && request.pointer("/params/y").and_then(Value::as_f64) == Some(10.0) =>
            {
                // 鼠标事件返回空结果。
                json!({})
            }
            // 聚焦只接受 query 签发的首个私有映射。
            Some("DOM.focus")
                if request
                    .pointer("/params/backendNodeId")
                    .and_then(Value::as_u64)
                    == Some(42) =>
            {
                // 聚焦返回空结果。
                json!({})
            }
            // 固定替换序列的键盘事件返回空结果。
            Some("Input.dispatchKeyEvent") => json!({}),
            // 文本插入只接受固定测试文本。
            Some("Input.insertText")
                if request.pointer("/params/text").and_then(Value::as_str)
                    == Some("fixture text") =>
            {
                // 文本输入返回空结果。
                json!({})
            }
            // 截图必须固定 PNG 和 fromSurface。
            Some("Page.captureScreenshot")
                if request.pointer("/params/format").and_then(Value::as_str) == Some("png")
                    && request
                        .pointer("/params/fromSurface")
                        .and_then(Value::as_bool)
                        == Some(true) =>
            {
                // 返回固定一像素 PNG。
                json!({ "data": FIXTURE_PNG_BASE64 })
            }
            // 拒绝其他 CDP 方法。
            _ => return false,
        };
        // 构造关联 CDP 响应。
        let Ok(response) = serde_json::to_vec(&json!({
            // 关联当前请求。
            "id": id,
            // 返回当前固定结果。
            "result": result,
        })) else {
            // 固定值序列化失败终止 fixture。
            return false;
        };
        // 写入导航响应。
        if !write_server_text(&mut stream, &response) {
            // 写入失败终止 fixture。
            return false;
        }
    }
    // 完整服务成功。
    true
}

// 读取到 HTTP headers 终止符。
fn read_headers(stream: &mut TcpStream) -> Option<String> {
    // 保存累计字节。
    let mut bytes = Vec::new();
    // 保存单字节缓冲。
    let mut byte = [0_u8; 1];
    // 持续读取到双 CRLF。
    while !bytes.ends_with(b"\r\n\r\n") {
        // 读取下一字节。
        stream.read_exact(&mut byte).ok()?;
        // 追加到有界缓冲。
        bytes.push(byte[0]);
        // 拒绝超限握手。
        if bytes.len() > MAXIMUM_HANDSHAKE_BYTES {
            // 返回无效。
            return None;
        }
    }
    // 严格转换 UTF-8 文本。
    String::from_utf8(bytes).ok()
}

// 读取并解码一个 masked client frame。
fn read_client_frame(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    // 读取基础 frame header。
    let mut header = [0_u8; 2];
    // EOF 或读取失败结束连接。
    stream.read_exact(&mut header).ok()?;
    // 只接受单帧且无扩展位。
    if header[0] & 0xf0 != 0x80
        // client frame 必须 masked。
        || header[1] & 0x80 == 0
    {
        // 拒绝协议漂移。
        return None;
    }
    // 取得 opcode。
    let opcode = header[0] & 0x0f;
    // 解码 payload 长度。
    let length = read_frame_length(stream, header[1] & 0x7f)?;
    // 拒绝超出 fixture 边界的请求。
    if length > MAXIMUM_FRAME_BYTES {
        // 返回无效。
        return None;
    }
    // 读取四字节 mask。
    let mut mask = [0_u8; 4];
    // mask 必须完整。
    stream.read_exact(&mut mask).ok()?;
    // 分配有界 payload。
    let mut payload = vec![0_u8; length];
    // 读取完整 payload。
    stream.read_exact(&mut payload).ok()?;
    // 就地解除 mask。
    for (index, byte) in payload.iter_mut().enumerate() {
        // 应用循环四字节 mask。
        *byte ^= mask[index % mask.len()];
    }
    // 返回 opcode 与原始 payload。
    Some((opcode, payload))
}

// 读取 WebSocket payload 长度。
fn read_frame_length(stream: &mut TcpStream, short: u8) -> Option<usize> {
    // 按 RFC 长度标记解码。
    match short {
        // 小长度直接返回。
        0..=125 => Some(usize::from(short)),
        // 读取十六位长度。
        126 => {
            // 保存网络序长度。
            let mut bytes = [0_u8; 2];
            // 必须完整读取。
            stream.read_exact(&mut bytes).ok()?;
            // 转换为本机长度。
            Some(usize::from(u16::from_be_bytes(bytes)))
        }
        // 读取六十四位长度。
        127 => {
            // 保存网络序长度。
            let mut bytes = [0_u8; 8];
            // 必须完整读取。
            stream.read_exact(&mut bytes).ok()?;
            // 有界转换到 usize。
            usize::try_from(u64::from_be_bytes(bytes)).ok()
        }
        // u8 已穷举所有标记。
        _ => None,
    }
}

// 写入一个 unmasked server text frame。
fn write_server_text(stream: &mut TcpStream, payload: &[u8]) -> bool {
    // 构造完整 frame。
    let mut frame = Vec::new();
    // 写入 FIN text opcode。
    frame.push(0x81);
    // 按 payload 长度写入 canonical header。
    if payload.len() <= 125 {
        // 小长度使用单字节。
        frame.push(u8::try_from(payload.len()).unwrap_or(125));
    } else if let Ok(length) = u16::try_from(payload.len()) {
        // 十六位长度使用标记。
        frame.push(126);
        // 写入网络序长度。
        frame.extend_from_slice(&length.to_be_bytes());
    } else {
        // fixture 响应不允许超出十六位长度。
        return false;
    }
    // 写入 JSON payload。
    frame.extend_from_slice(payload);
    // 写入并 flush 完整响应。
    stream
        .write_all(&frame)
        .and_then(|()| stream.flush())
        .is_ok()
}

// 计算 RFC 6455 Sec-WebSocket-Accept。
fn websocket_accept(key: &str) -> String {
    // 创建 SHA-1 摘要器。
    let mut hasher = Sha1::new();
    // 写入客户端 key。
    hasher.update(key.as_bytes());
    // 写入标准 GUID。
    hasher.update(WEBSOCKET_GUID.as_bytes());
    // 编码二十字节摘要。
    BASE64_STANDARD.encode(hasher.finalize())
}

// 执行固定会话 runtime fixture。
fn main() {
    // 提前退出模式用于验证 accepted 后异常退出。
    if std::env::var_os("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE").as_deref()
        // 核对固定模式。
        == Some("exit".as_ref())
    {
        // 使用固定非零退出码。
        std::process::exit(7);
    }
    // 取得 worker 自己创建的 profile。
    let Some(profile) = profile_path() else {
        // 缺失固定参数表示调用边界漂移。
        std::process::exit(2);
    };
    // 永不就绪模式只保持进程存活供 deadline 测试。
    if std::env::var_os("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE").as_deref()
        // 核对固定模式。
        == Some("never-ready".as_ref())
    {
        // 等待父 Job 或 worker 终止。
        loop {
            // 使用短休眠避免占用 CPU。
            thread::sleep(Duration::from_millis(100));
        }
    }
    // 绑定由系统分配的 IPv4 回环端口。
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        // 测试环境无法监听时立即失败。
        .unwrap_or_else(|_| std::process::exit(2));
    // 读取实际回环端口。
    let port = listener
        // 取得本地地址。
        .local_addr()
        // 地址读取失败时退出。
        .unwrap_or_else(|_| std::process::exit(2))
        // 只读取端口且不公开给 worker 之外。
        .port();
    // 构造 Chromium 兼容两行事实。
    let active_port = format!("{port}\n/devtools/browser/{FIXTURE_TOKEN}\n");
    // 写入工具 profile 内的固定事实文件。
    if fs::write(profile.join("DevToolsActivePort"), active_port).is_err() {
        // 写入失败时退出。
        std::process::exit(2);
    }
    // 服务唯一打开探测。
    if !serve_probe(&listener, port) {
        // 探测失败时退出。
        std::process::exit(2);
    }
    // 读取可选固定 CDP 故障模式。
    let mode = std::env::var("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE").ok();
    // 服务固定 CDP 生命周期。
    if !serve_cdp(&listener, mode.as_deref()) {
        // 协议漂移时退出。
        std::process::exit(2);
    }
}
