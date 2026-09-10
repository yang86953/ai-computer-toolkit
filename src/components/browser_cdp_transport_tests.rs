// 导入本机 fixture 网络、同步和时间工具。
use std::{
    // 导入固定 I/O。
    io::{Read, Write},
    // 导入回环 listener。
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    // 导入线程同步通道。
    sync::mpsc,
    // 导入 fixture 线程。
    thread,
    // 导入 deadline。
    time::{Duration, Instant},
};

// 导入 Base64 和 SHA-1 构造正确握手。
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
// 导入 JSON 值与构造。
use serde_json::{Value, json};
// 导入 SHA-1 摘要接口。
use sha1::{Digest, Sha1};

// 导入父传输实现。
use super::{CdpConnection, CdpMethod, WEBSOCKET_GUID};

// 固定测试 WebSocket path。
const PATH: &str = "/devtools/browser/fixture_transport";

// 表示 fixture 响应行为。
#[derive(Clone, Copy)]
enum FixtureMode {
    // 正常响应。
    Success,
    // 返回错误 accept。
    BadAccept,
    // 返回漂移响应 ID。
    WrongCorrelation,
    // 返回 masked server frame。
    MaskedServerFrame,
}

// 启动一个固定单连接 WebSocket/CDP fixture。
fn spawn_fixture(mode: FixtureMode) -> (u16, thread::JoinHandle<()>) {
    // 绑定系统分配的回环端口。
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture bind failed: {error}"));
    // 读取实际端口。
    let port = listener
        // 取得本地地址。
        .local_addr()
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture address failed: {error}"))
        // 只返回端口。
        .port();
    // 启动单连接 fixture 线程。
    let handle = thread::spawn(move || {
        // 接受唯一连接。
        let (mut stream, peer) = listener
            // 等待客户端。
            .accept()
            // 测试环境失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("fixture accept failed: {error}"));
        // 客户端必须是 IPv4 回环。
        assert!(peer.ip().is_loopback());
        // 读取升级请求。
        let request = read_headers(&mut stream);
        // 请求必须指向固定 path。
        assert!(request.starts_with(&format!("GET {PATH} HTTP/1.1\r\n")));
        // 提取唯一 key。
        let key = request
            // 遍历 header 行。
            .split("\r\n")
            // 找到固定 key header。
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            // 缺失 key 时终止测试。
            .unwrap_or_else(|| panic!("fixture key missing"));
        // 计算正确 accept。
        let accept = if matches!(mode, FixtureMode::BadAccept) {
            // 错误模式使用固定伪造值。
            "invalid-proof".to_owned()
        } else {
            // 正常模式计算 RFC proof。
            websocket_accept(key)
        };
        // 构造严格升级响应。
        let response = format!(
            // 只发送必需 headers。
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        // 写入握手响应。
        stream
            // 写入全部字节。
            .write_all(response.as_bytes())
            // 测试环境失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("fixture handshake failed: {error}"));
        // 错误握手不继续读命令。
        if matches!(mode, FixtureMode::BadAccept) {
            // 结束 fixture。
            return;
        }
        // 读取并解码一个 masked client text frame。
        let request = read_client_text(&mut stream);
        // 严格解析 CDP request。
        let value = serde_json::from_slice::<Value>(&request)
            // 协议污染时给出明确诊断。
            .unwrap_or_else(|error| panic!("fixture request JSON failed: {error}"));
        // 方法必须来自封闭枚举。
        assert_eq!(
            value.get("method").and_then(Value::as_str),
            Some("Page.enable")
        );
        // 参数必须是对象。
        assert!(value.get("params").is_some_and(Value::is_object));
        // 读取请求 ID。
        let id = value
            // 读取 id。
            .get("id")
            // 转换整数。
            .and_then(Value::as_u64)
            // 缺失时终止测试。
            .unwrap_or_else(|| panic!("fixture request id missing"));
        // 漂移模式使用错误 ID。
        let response_id = if matches!(mode, FixtureMode::WrongCorrelation) {
            // 返回下一个错误 ID。
            id.saturating_add(1)
        } else {
            // 正常关联。
            id
        };
        // 构造固定 CDP result。
        let response = serde_json::to_vec(&json!({
            // 关联请求 ID。
            "id": response_id,
            // 返回空对象结果。
            "result": { "enabled": true }
        }))
        // fixture 必须可序列化。
        .unwrap_or_else(|error| panic!("fixture response JSON failed: {error}"));
        // 写入 server text frame。
        write_server_text(
            // 借用 socket。
            &mut stream,
            // 借用响应。
            &response,
            // 特殊模式伪造 masked server frame。
            matches!(mode, FixtureMode::MaskedServerFrame),
        );
    });
    // 返回端口和线程。
    (port, handle)
}

// 读取 HTTP headers 到双 CRLF。
fn read_headers(stream: &mut TcpStream) -> String {
    // 保存累计字节。
    let mut bytes = Vec::new();
    // 保存单字节缓冲。
    let mut byte = [0_u8; 1];
    // 持续读取到 headers 完成。
    while !bytes.ends_with(b"\r\n\r\n") {
        // 读取单字节。
        stream
            // 执行精确读取。
            .read_exact(&mut byte)
            // 测试环境失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("fixture header read failed: {error}"));
        // 追加字节。
        bytes.push(byte[0]);
        // fixture 请求必须有界。
        assert!(bytes.len() <= 16 * 1024);
    }
    // 严格转换 UTF-8。
    String::from_utf8(bytes)
        // 非文本握手时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture header UTF-8 failed: {error}"))
}

// 读取并验证一个 masked client text frame。
fn read_client_text(stream: &mut TcpStream) -> Vec<u8> {
    // 读取基本 header。
    let mut header = [0_u8; 2];
    // 读取两个字节。
    stream
        // 精确读取。
        .read_exact(&mut header)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture frame header failed: {error}"));
    // 必须是 FIN text。
    assert_eq!(header[0], 0x81);
    // client frame 必须 masked。
    assert_ne!(header[1] & 0x80, 0);
    // 解析长度。
    let length = read_length(stream, header[1] & 0x7f);
    // 读取四字节 mask。
    let mut mask = [0_u8; 4];
    // 精确读取 mask。
    stream
        // 读取 mask。
        .read_exact(&mut mask)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture mask failed: {error}"));
    // 分配有界 payload。
    let mut payload = vec![0_u8; length];
    // 读取完整 payload。
    stream
        // 精确读取。
        .read_exact(&mut payload)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture payload failed: {error}"));
    // 解 mask。
    for (index, byte) in payload.iter_mut().enumerate() {
        // 应用循环 mask。
        *byte ^= mask[index % mask.len()];
    }
    // 返回原始文本字节。
    payload
}

// 读取 fixture 需要的 WebSocket 长度。
fn read_length(stream: &mut TcpStream, short: u8) -> usize {
    // 按长度标记处理。
    match short {
        // 小长度直接返回。
        0..=125 => usize::from(short),
        // 读取 16 位长度。
        126 => {
            // 保存两字节。
            let mut bytes = [0_u8; 2];
            // 精确读取。
            stream
                // 读取长度。
                .read_exact(&mut bytes)
                // 测试环境失败时给出明确诊断。
                .unwrap_or_else(|error| panic!("fixture 16-bit length failed: {error}"));
            // 转换为 usize。
            usize::from(u16::from_be_bytes(bytes))
        }
        // 测试请求不应达到 64 位长度。
        _ => panic!("fixture request unexpectedly used 64-bit length"),
    }
}

// 写入一个 unmasked 或故意 masked 的 server text frame。
fn write_server_text(stream: &mut TcpStream, payload: &[u8], masked: bool) {
    // fixture payload 必须使用小长度。
    assert!(payload.len() <= 125);
    // 构造 frame。
    let mut frame = Vec::new();
    // 写入 FIN text。
    frame.push(0x81);
    // 写入长度和可选非法 mask 位。
    frame.push((if masked { 0x80 } else { 0 }) | u8::try_from(payload.len()).unwrap_or(125));
    // masked 模式写入固定 mask 和 masked payload。
    if masked {
        // 固定测试 mask。
        let mask = [1_u8, 2, 3, 4];
        // 写入 mask。
        frame.extend_from_slice(&mask);
        // 写入 masked payload。
        frame.extend(
            payload
                // 遍历响应。
                .iter()
                // 结合索引。
                .enumerate()
                // 应用固定 mask。
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
    } else {
        // 正常 server frame 不 mask。
        frame.extend_from_slice(payload);
    }
    // 写入完整 frame。
    stream
        // 写入全部字节。
        .write_all(&frame)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture response failed: {error}"));
}

// 计算 fixture WebSocket accept。
fn websocket_accept(key: &str) -> String {
    // 初始化 SHA-1。
    let mut hasher = Sha1::new();
    // 写入 client key。
    hasher.update(key.as_bytes());
    // 写入固定 GUID。
    hasher.update(WEBSOCKET_GUID.as_bytes());
    // 编码摘要。
    BASE64_STANDARD.encode(hasher.finalize())
}

// 验证严格握手、masked 请求、固定方法和响应关联。
#[test]
// 使用真实 loopback socket fixture。
fn loopback_handshake_and_correlated_call_succeed() {
    // 启动正常 fixture。
    let (port, fixture) = spawn_fixture(FixtureMode::Success);
    // 建立私有连接。
    let mut connection = CdpConnection::connect(port, PATH, Duration::from_secs(2))
        // 合法 fixture 不得失败。
        .unwrap_or_else(|error| panic!("CDP connect failed: {error}"));
    // 调用封闭 Page.enable。
    let result = connection
        // 执行固定调用。
        .call(
            // 使用封闭枚举。
            CdpMethod::PageEnable,
            // 使用空参数对象。
            json!({}),
            // 设置绝对 deadline。
            Instant::now() + Duration::from_secs(2),
        )
        // 合法响应不得失败。
        .unwrap_or_else(|error| panic!("CDP call failed: {error}"));
    // 核对结果字段。
    assert_eq!(result.get("enabled").and_then(Value::as_bool), Some(true));
    // 关闭连接。
    connection.close();
    // 等待 fixture 完成。
    fixture
        // join 线程。
        .join()
        // fixture panic 时终止测试。
        .unwrap_or_else(|_| panic!("fixture thread panicked"));
}

// 验证握手 proof 不匹配时失败闭合。
#[test]
// 使用错误 accept fixture。
fn invalid_upgrade_proof_is_rejected() {
    // 启动错误握手 fixture。
    let (port, fixture) = spawn_fixture(FixtureMode::BadAccept);
    // 连接必须失败。
    let error = CdpConnection::connect(port, PATH, Duration::from_secs(2))
        // 取得错误。
        .err()
        // 缺失错误时终止测试。
        .unwrap_or_else(|| panic!("bad accept must fail"));
    // 核对稳定认证错误码。
    assert_eq!(error.code, "BROWSER_PROTOCOL_AUTH_FAILED");
    // 等待 fixture 完成。
    fixture
        // join 线程。
        .join()
        // fixture panic 时终止测试。
        .unwrap_or_else(|_| panic!("fixture thread panicked"));
}

// 验证响应 ID 漂移和 masked server frame 都拒绝。
#[test]
// 逐一运行两类协议污染。
fn response_correlation_and_server_mask_are_strict() {
    // 遍历固定污染模式。
    for mode in [
        FixtureMode::WrongCorrelation,
        FixtureMode::MaskedServerFrame,
    ] {
        // 启动当前 fixture。
        let (port, fixture) = spawn_fixture(mode);
        // 建立合法握手。
        let mut connection = CdpConnection::connect(port, PATH, Duration::from_secs(2))
            // 握手不得失败。
            .unwrap_or_else(|error| panic!("CDP connect failed: {error}"));
        // 调用必须失败。
        let error = connection
            // 执行固定方法。
            .call(
                // 使用 Page.enable。
                CdpMethod::PageEnable,
                // 使用空参数。
                json!({}),
                // 设置 deadline。
                Instant::now() + Duration::from_secs(2),
            )
            // 取得错误。
            .err()
            // 缺失错误时终止测试。
            .unwrap_or_else(|| panic!("protocol drift must fail"));
        // 必须使用协议失败码。
        assert_eq!(error.code, "BROWSER_PROTOCOL_FAILED");
        // 等待 fixture 完成。
        fixture
            // join 线程。
            .join()
            // fixture panic 时终止测试。
            .unwrap_or_else(|_| panic!("fixture thread panicked"));
    }
}

// 验证 endpoint、deadline、参数与 flatten session 形状门禁。
#[test]
// 不创建网络连接即可覆盖本地门禁。
fn endpoint_and_command_inputs_are_closed() {
    // 非回环 browser path 形状被拒绝。
    let error = CdpConnection::connect(1, "/devtools/page/native", Duration::from_secs(1))
        // 取得错误。
        .err()
        // 缺失错误时终止测试。
        .unwrap_or_else(|| panic!("invalid path must fail"));
    // 核对参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 建立正常 fixture 供后续本地门禁。
    let (port, fixture) = spawn_fixture(FixtureMode::Success);
    // 建立连接。
    let mut connection = CdpConnection::connect(port, PATH, Duration::from_secs(2))
        // 合法 fixture 不得失败。
        .unwrap_or_else(|error| panic!("CDP connect failed: {error}"));
    // 非对象 params 必须在网络写入前拒绝。
    let error = connection
        // 调用固定方法。
        .call(
            // 使用固定方法。
            CdpMethod::PageEnable,
            // 传入非法数组。
            json!([]),
            // 设置 deadline。
            Instant::now() + Duration::from_secs(2),
        )
        // 取得错误。
        .err()
        // 缺失错误时终止测试。
        .unwrap_or_else(|| panic!("non-object params must fail"));
    // 核对参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 非 canonical target session 被拒绝。
    let error = connection
        // 设置危险 token。
        .set_session_id("native/session".to_owned())
        // 取得错误。
        .err()
        // 缺失错误时终止测试。
        .unwrap_or_else(|| panic!("invalid target session must fail"));
    // 核对协议错误。
    assert_eq!(error.code, "BROWSER_PROTOCOL_FAILED");
    // 最终执行一次合法调用让 fixture 完成。
    let _ = connection.call(
        // 使用固定方法。
        CdpMethod::PageEnable,
        // 使用空对象。
        json!({}),
        // 设置 deadline。
        Instant::now() + Duration::from_secs(2),
    );
    // 等待 fixture 完成。
    fixture
        // join 线程。
        .join()
        // fixture panic 时终止测试。
        .unwrap_or_else(|_| panic!("fixture thread panicked"));
}

// 验证 timeout 使用绝对 deadline 且不会无限等待。
#[test]
// fixture 完成握手后不返回 CDP frame。
fn call_deadline_is_enforced() {
    // 绑定回环 listener。
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("deadline fixture bind failed: {error}"));
    // 读取端口。
    let port = listener
        // 取得地址。
        .local_addr()
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("deadline fixture address failed: {error}"))
        // 读取端口。
        .port();
    // 建立通知通道避免 fixture 提前退出。
    let (sender, receiver) = mpsc::channel();
    // 启动 deadline fixture。
    let fixture = thread::spawn(move || {
        // 接受连接。
        let (mut stream, _) = listener
            // 等待客户端。
            .accept()
            // 测试环境失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("deadline fixture accept failed: {error}"));
        // 读取 headers。
        let request = read_headers(&mut stream);
        // 提取 key。
        let key = request
            // 遍历行。
            .split("\r\n")
            // 查找 key。
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            // 缺失时终止测试。
            .unwrap_or_else(|| panic!("deadline fixture key missing"));
        // 构造正确握手。
        let response = format!(
            // 使用严格 headers。
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            // 计算 proof。
            websocket_accept(key)
        );
        // 写入握手。
        stream
            // 写入全部字节。
            .write_all(response.as_bytes())
            // 测试环境失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("deadline handshake failed: {error}"));
        // 读取客户端命令以确认 dispatch。
        let _ = read_client_text(&mut stream);
        // 通知测试命令已经接收。
        let _ = sender.send(());
        // 保持连接超过 client deadline。
        thread::sleep(Duration::from_millis(200));
    });
    // 建立连接。
    let mut connection = CdpConnection::connect(port, PATH, Duration::from_secs(2))
        // 合法握手不得失败。
        .unwrap_or_else(|error| panic!("deadline CDP connect failed: {error}"));
    // 使用短绝对 deadline 调用。
    let error = connection
        // 执行固定方法。
        .call(
            // 使用固定方法。
            CdpMethod::PageEnable,
            // 使用空对象。
            json!({}),
            // 设置短 deadline。
            Instant::now() + Duration::from_millis(50),
        )
        // 取得错误。
        .err()
        // 缺失错误时终止测试。
        .unwrap_or_else(|| panic!("deadline call must fail"));
    // socket timeout 映射为断开，不伪造 completed。
    assert_eq!(error.code, "BROWSER_PROTOCOL_DISCONNECTED");
    // 命令确实已经 dispatch。
    receiver
        // 等待通知。
        .recv_timeout(Duration::from_secs(1))
        // 缺失通知时给出明确诊断。
        .unwrap_or_else(|error| panic!("deadline dispatch missing: {error}"));
    // 等待 fixture 完成。
    fixture
        // join 线程。
        .join()
        // fixture panic 时终止测试。
        .unwrap_or_else(|_| panic!("deadline fixture panicked"));
}
