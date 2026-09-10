#![cfg(target_os = "windows")]

//! 验证固定浏览器会话 worker 的真实 stdio 与 runtime 生命周期。

// 导入进程、管道、路径与 JSON 测试工具。
use std::{
    // 导入行读取与协议写入。
    io::{BufRead, BufReader, Read, Write},
    // 导入测试路径。
    path::{Path, PathBuf},
    // 导入固定子进程与管道类型。
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    // 导入原子测试目录序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入中立 JSON 值。
use serde_json::{Value, json};

// 固定生产 browser-session worker 路径。
const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker");
// 固定仓库自有 session runtime fixture 路径。
const RUNTIME: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-runtime-fixture");
// 固定协议版本。
const CONTRACT_VERSION: &str = "act/browser-session-worker/v1";
// 固定页面命令协议版本。
const PAGE_CONTRACT_VERSION: &str = "act/browser-page-command-worker/v1";
// 固定测试请求 nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定页面命令请求 nonce。
const PAGE_REQUEST_NONCE: &str = "abcdef0123456789abcdef0123456789";
// 固定页面等待请求 nonce。
const WAIT_REQUEST_NONCE: &str = "11111111111111111111111111111111";
// 固定页面查询请求 nonce。
const QUERY_REQUEST_NONCE: &str = "22222222222222222222222222222222";
// 固定零命中查询请求 nonce。
const EMPTY_QUERY_NONCE: &str = "33333333333333333333333333333333";
// 固定文本等待请求 nonce。
const TEXT_WAIT_NONCE: &str = "44444444444444444444444444444444";
// 固定点击请求 nonce。
const CLICK_REQUEST_NONCE: &str = "55555555555555555555555555555555";
// 固定输入请求 nonce。
const TYPE_REQUEST_NONCE: &str = "66666666666666666666666666666666";
// 固定截图请求 nonce。
const SCREENSHOT_REQUEST_NONCE: &str = "77777777777777777777777777777777";
// 固定 stale 元素请求 nonce。
const STALE_ELEMENT_NONCE: &str = "88888888888888888888888888888888";
// 保存进程内测试目录序列。
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 保存一个真实 worker 测试进程及其 parent 管道。
struct WorkerFixture {
    // 保存 worker 进程所有权。
    child: Child,
    // 保存可继续发送 cancel 的 stdin。
    stdin: ChildStdin,
    // 保存逐行读取 stdout 的缓冲器。
    stdout: BufReader<ChildStdout>,
    // 保存隔离临时根供清理验证。
    temporary_root: PathBuf,
}

// 为测试 fixture 提供确定性启动与回收。
impl WorkerFixture {
    // 启动绑定固定 runtime 的生产 worker。
    fn spawn(runtime_mode: Option<&str>) -> Self {
        // 为本测试生成唯一临时根。
        let temporary_root = std::env::temp_dir().join(format!(
            // 使用固定前缀、进程与序列避免碰撞。
            "act-browser-session-integration-{}-{}",
            // 注入测试进程 ID。
            std::process::id(),
            // 取得当前唯一序列。
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // 清除理论上的陈旧同名目录。
        let _ = std::fs::remove_dir_all(&temporary_root);
        // 创建 worker 可写临时根。
        std::fs::create_dir(&temporary_root)
            // 测试环境失败时提供明确诊断。
            .unwrap_or_else(|error| panic!("temporary root failed: {error}"));
        // 构造固定生产 worker 命令。
        let mut command = Command::new(WORKER);
        // 让 runtime 发现只命中仓库 fixture。
        command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", RUNTIME);
        // 隔离本测试 profile 根。
        command.env("TEMP", &temporary_root);
        // 同步 Windows 另一临时变量。
        command.env("TMP", &temporary_root);
        // 按固定枚举配置 runtime 行为。
        if let Some(mode) = runtime_mode {
            // 只由测试代码设置 fixture 模式。
            command.env("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE", mode);
        }
        // 建立 parent 可写 stdin。
        command.stdin(Stdio::piped());
        // 建立 parent 可读 stdout。
        command.stdout(Stdio::piped());
        // 隔离诊断输出。
        command.stderr(Stdio::null());
        // 启动真实固定 worker。
        let mut child = command
            // 执行无 shell spawn。
            .spawn()
            // 测试环境失败时提供明确诊断。
            .unwrap_or_else(|error| panic!("worker spawn failed: {error}"));
        // 取得唯一 stdin 写端。
        let stdin = child
            // 移交 parent stdin。
            .stdin
            // 缺失管道表示启动模板漂移。
            .take()
            // 显式终止测试。
            .unwrap_or_else(|| panic!("worker stdin missing"));
        // 取得唯一 stdout 读端。
        let stdout = child
            // 移交 parent stdout。
            .stdout
            // 缺失管道表示启动模板漂移。
            .take()
            // 显式终止测试。
            .unwrap_or_else(|| panic!("worker stdout missing"));
        // 返回完整进程 fixture。
        Self {
            // 保存进程。
            child,
            // 保存 stdin。
            stdin,
            // 缓冲 stdout。
            stdout: BufReader::new(stdout),
            // 保存临时根。
            temporary_root,
        }
    }

    // 写入一条完整 JSON Lines 输入。
    fn write(&mut self, value: &Value) {
        // 序列化固定测试输入。
        let text = serde_json::to_string(value)
            // 测试 fixture 必须可序列化。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入单行并 flush。
        writeln!(self.stdin, "{text}")
            // 管道失败时提供明确诊断。
            .and_then(|()| self.stdin.flush())
            // 终止失败测试。
            .unwrap_or_else(|error| panic!("worker stdin write failed: {error}"));
    }

    // 读取一条必需协议帧。
    fn read_frame(&mut self) -> Value {
        // 保存一行输出。
        let mut line = String::new();
        // 阻塞读取下一个 worker 帧。
        let read = self
            // 从缓冲 stdout 读取。
            .stdout
            // 读取到换行。
            .read_line(&mut line)
            // 管道失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("worker stdout read failed: {error}"));
        // 必需帧不得为 EOF。
        assert_ne!(read, 0, "worker stdout ended before required frame");
        // 严格解析 JSON。
        serde_json::from_str(line.trim_end())
            // 非 JSON 输出暴露协议污染。
            .unwrap_or_else(|error| panic!("worker frame parse failed: {error}"))
    }

    // 关闭 stdin 并等待 worker 退出。
    fn close_and_wait(mut self) -> u32 {
        // 先关闭 parent 输入端触发资源回收。
        drop(self.stdin);
        // 等待真实 worker 退出。
        let status = self
            // 回收主进程。
            .child
            // 阻塞等待测试边界内的固定 worker。
            .wait()
            // 等待失败时给出明确诊断。
            .unwrap_or_else(|error| panic!("worker wait failed: {error}"));
        // Windows 测试退出码必须可用。
        status
            // 取得进程退出码。
            .code()
            // 转换为无符号结果。
            .and_then(|code| u32::try_from(code).ok())
            // 缺失退出码表示测试进程被外部终止。
            .unwrap_or(u32::MAX)
    }

    // 返回工具 profile 固定根。
    fn profile_root(&self) -> PathBuf {
        // 与生产 BrowserProfile 组合规则保持一致。
        self.temporary_root
            // 进入固定工具根。
            .join("ai-computer-toolkit-browser")
    }
}

// 构造固定 isolated-profile open。
fn open_request(timeout_ms: u32) -> Value {
    // 返回无路径、无 executable、无 argv 的打开请求。
    json!({
        // 使用唯一打开变体。
        "kind": "open",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 固定测试关联值。
        "requestNonce": REQUEST_NONCE,
        // 注入测试 deadline。
        "timeoutMs": timeout_ms,
        // 选择空隔离 profile 来源。
        "source": { "kind": "isolated-profile" },
    })
}

// 构造关联 cancel。
fn cancel_request() -> Value {
    // 返回无额外字段的幂等取消。
    json!({
        // 使用取消变体。
        "kind": "cancel",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联同一打开请求。
        "requestNonce": REQUEST_NONCE,
    })
}

// 构造初始固定页面导航。
fn navigate_request(
    // 借用公开会话身份。
    session_id: &str,
    // 借用可选当前私有页面引用。
    page_ref: Option<&str>,
    // 接收调用方观察代际。
    navigation_generation: u64,
) -> Value {
    // 委托固定默认期限构造。
    navigate_request_with_timeout(session_id, page_ref, navigation_generation, 5_000)
}

// 构造可注入短期限的固定页面导航。
fn navigate_request_with_timeout(
    // 借用公开会话身份。
    session_id: &str,
    // 借用可选当前私有页面引用。
    page_ref: Option<&str>,
    // 接收调用方观察代际。
    navigation_generation: u64,
    // 接收页面命令总期限。
    timeout_ms: u32,
) -> Value {
    // 返回不含 CDP、脚本或原生身份的页面命令。
    json!({
        // 使用页面 command 变体。
        "kind": "command",
        // 使用冻结页面协议。
        "contractVersion": PAGE_CONTRACT_VERSION,
        // 绑定打开阶段签发的 opaque 会话。
        "sessionId": session_id,
        // 使用独立页面命令 nonce。
        "requestNonce": PAGE_REQUEST_NONCE,
        // 使用有界总 deadline。
        "timeoutMs": timeout_ms,
        // 初始导航没有旧页面引用。
        "pageRef": page_ref,
        // 使用调用方观察的导航代际。
        "navigationGeneration": navigation_generation,
        // 只请求强类型导航。
        "operation": {
            // 使用导航种类。
            "kind": "navigate",
            // 使用 fixture 固定 URL。
            "url": "https://example.test/page",
        },
    })
}

// 构造绑定当前页面的通用命令。
fn page_request(
    // 借用公开会话身份。
    session_id: &str,
    // 借用页面命令 nonce。
    request_nonce: &str,
    // 借用当前私有页面引用。
    page_ref: &str,
    // 接收强类型操作。
    operation: Value,
) -> Value {
    // 返回冻结页面命令 envelope。
    json!({
        // 使用 command 变体。
        "kind": "command",
        // 使用冻结页面协议。
        "contractVersion": PAGE_CONTRACT_VERSION,
        // 绑定当前会话。
        "sessionId": session_id,
        // 关联本次命令。
        "requestNonce": request_nonce,
        // 使用有界总 deadline。
        "timeoutMs": 5_000,
        // 绑定当前私有页面。
        "pageRef": page_ref,
        // 当前测试已完成一次导航。
        "navigationGeneration": 1,
        // 注入强类型操作。
        "operation": operation,
    })
}

// 核对 accepted 固定事实。
fn assert_accepted(frame: &Value) {
    // 核对帧种类。
    assert_eq!(
        frame.get("kind").and_then(Value::as_str),
        Some("open-accepted")
    );
    // 核对 dispatch 事实。
    assert_eq!(
        // 读取布尔字段。
        frame.get("dispatchAccepted").and_then(Value::as_bool),
        // accepted 必须为真。
        Some(true)
    );
}

// 核对 final 结果类别。
fn assert_outcome(frame: &Value, outcome: &str) {
    // 核对唯一 final 种类。
    assert_eq!(
        frame.get("kind").and_then(Value::as_str),
        Some("open-final")
    );
    // 核对封闭 outcome。
    assert_eq!(frame.get("outcome").and_then(Value::as_str), Some(outcome));
}

// 验证真实 runtime fixture 建立 ready 并在 cancel 后清理 profile。
#[test]
// 覆盖启动、协议探测、关闭与资源清理。
fn isolated_session_opens_closes_and_cleans_profile() {
    // 启动 ready runtime。
    let mut fixture = WorkerFixture::spawn(None);
    // 保存待验证 profile 根。
    let profile_root = fixture.profile_root();
    // 发送打开请求。
    fixture.write(&open_request(5_000));
    // 第一帧必须是 accepted。
    assert_accepted(&fixture.read_frame());
    // 第二帧必须是 ready。
    let final_frame = fixture.read_frame();
    // 核对 ready 结果。
    assert_outcome(&final_frame, "ready");
    // opaque 会话不得泄漏 endpoint 或路径。
    assert!(
        final_frame
            // 读取会话 ID。
            .get("sessionId")
            // 转换为字符串。
            .and_then(Value::as_str)
            // 核对固定 opaque 形状。
            .is_some_and(|value| value.starts_with("s2:bs:") && value.len() == 38)
    );
    // 取得公开 opaque 会话身份。
    let session_id = final_frame
        // 读取会话字段。
        .get("sessionId")
        // 转换为字符串。
        .and_then(Value::as_str)
        // ready 已验证必须存在。
        .unwrap_or_else(|| panic!("ready session id missing"));
    // 发送初始页面导航。
    fixture.write(&navigate_request(session_id, None, 0));
    // 页面命令先输出 accepted。
    let accepted = fixture.read_frame();
    // 核对页面 accepted 种类。
    assert_eq!(
        // 读取帧种类。
        accepted.get("kind").and_then(Value::as_str),
        // 必须是 command accepted。
        Some("command-accepted")
    );
    // 读取页面导航 final。
    let navigated = fixture.read_frame();
    // 核对完成结果。
    assert_eq!(
        // 读取 outcome。
        navigated.get("outcome").and_then(Value::as_str),
        // 导航必须确定完成。
        Some("completed")
    );
    // 导航必须推进代际。
    assert_eq!(
        // 读取导航代际。
        navigated
            // 取得字段。
            .get("navigationGeneration")
            // 转换为整数。
            .and_then(Value::as_u64),
        // 初次成功导航推进为一。
        Some(1)
    );
    // 取得新的 worker 私有页面引用。
    let page_ref = navigated
        // 读取数据内页面引用。
        .pointer("/data/pageRef")
        // 转换为字符串。
        .and_then(Value::as_str)
        // 页面导航成功必须返回引用。
        .unwrap_or_else(|| panic!("page ref missing"));
    // 核对 canonical 私有形状。
    assert!(page_ref.starts_with("w1:bp:") && page_ref.len() == 38);
    // 发送文档就绪等待。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用等待 nonce。
        WAIT_REQUEST_NONCE,
        // 绑定当前页面。
        page_ref,
        // 使用 document-ready 条件。
        json!({ "kind": "wait", "condition": { "kind": "document-ready" } }),
    ));
    // wait 必须先 accepted。
    assert_eq!(
        // 读取帧种类。
        fixture.read_frame().get("kind").and_then(Value::as_str),
        // 核对 accepted。
        Some("command-accepted")
    );
    // wait 必须确定完成。
    assert_eq!(
        // 读取完成数据。
        fixture
            // 读取 final。
            .read_frame()
            // 读取条件事实。
            .pointer("/data/conditionMet")
            // 转换布尔。
            .and_then(Value::as_bool),
        // 条件已经满足。
        Some(true)
    );
    // 发送可见文本等待。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用文本等待 nonce。
        TEXT_WAIT_NONCE,
        // 绑定当前页面。
        page_ref,
        // 使用大小写不敏感包含匹配。
        json!({
            // 使用 wait 操作。
            "kind": "wait",
            // 等待固定文本。
            "condition": { "kind": "text-present", "text": "welcome", "exact": false },
        }),
    ));
    // 文本 wait 必须先 accepted。
    let _ = fixture.read_frame();
    // 文本 wait 必须确定完成。
    assert_eq!(
        // 读取条件事实。
        fixture
            // 读取 final。
            .read_frame()
            // 读取完成数据。
            .pointer("/data/conditionMet")
            // 转换布尔。
            .and_then(Value::as_bool),
        // 文本已经命中。
        Some(true)
    );
    // 发送会产生多命中且截断的查询。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用查询 nonce。
        QUERY_REQUEST_NONCE,
        // 绑定当前页面。
        page_ref,
        // 查询同名按钮并限制一个结果。
        json!({
            // 使用 query 操作。
            "kind": "query",
            // 使用 provider-neutral selector。
            "selector": { "role": "button", "name": "Submit", "text": null, "exact": true },
            // 只返回一个结果。
            "maxResults": 1,
        }),
    ));
    // query 必须先 accepted。
    assert_eq!(
        // 读取帧种类。
        fixture.read_frame().get("kind").and_then(Value::as_str),
        // 核对 accepted。
        Some("command-accepted")
    );
    // 读取查询 final。
    let queried = fixture.read_frame();
    // 完整命中数必须为二。
    assert_eq!(
        // 读取命中总数。
        queried.pointer("/data/matchCount").and_then(Value::as_u64),
        // fixture 有两个同名按钮。
        Some(2)
    );
    // 返回数组受 maxResults 截断。
    assert_eq!(
        // 读取截断事实。
        queried.pointer("/data/truncated").and_then(Value::as_bool),
        // 必须已截断。
        Some(true)
    );
    // 取得首个私有元素引用。
    let element_ref = queried
        // 读取首个元素引用。
        .pointer("/data/matches/0/elementRef")
        // 转换为字符串。
        .and_then(Value::as_str)
        // 查询成功必须签发引用。
        .unwrap_or_else(|| panic!("element ref missing"));
    // 私有元素引用不得暴露 backend node ID。
    assert!(element_ref.starts_with("w1:be:") && element_ref.len() == 38);
    // 首个匹配按钮必须投影为 enabled。
    assert_eq!(
        // 读取 enabled 事实。
        queried
            // 定位首个 match。
            .pointer("/data/matches/0/enabled")
            // 转换布尔。
            .and_then(Value::as_bool),
        // 第一个 fixture 按钮可用。
        Some(true)
    );
    // 发送零命中查询。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用独立 nonce。
        EMPTY_QUERY_NONCE,
        // 绑定当前页面。
        page_ref,
        // 查询不存在名称。
        json!({
            // 使用 query 操作。
            "kind": "query",
            // 使用 provider-neutral selector。
            "selector": { "role": null, "name": "Missing", "text": null, "exact": true },
            // 返回上限为五。
            "maxResults": 5,
        }),
    ));
    // 零命中 query 仍先 accepted。
    let _ = fixture.read_frame();
    // 读取零命中 final。
    let empty = fixture.read_frame();
    // 核对零命中可信数据。
    assert_eq!(
        // 读取命中总数。
        empty.pointer("/data/matchCount").and_then(Value::as_u64),
        // 明确为零。
        Some(0)
    );
    // 点击当前元素。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用点击 nonce。
        CLICK_REQUEST_NONCE,
        // 绑定当前页面。
        page_ref,
        // 使用已确认点击。
        json!({ "kind": "click", "confirmed": true, "elementRef": element_ref }),
    ));
    // 点击必须先 accepted。
    let _ = fixture.read_frame();
    // 点击必须确定完成。
    assert_eq!(
        // 读取 clicked 事实。
        fixture
            // 读取 final。
            .read_frame()
            // 读取完成数据。
            .pointer("/data/clicked")
            // 转换布尔。
            .and_then(Value::as_bool),
        // 点击完成。
        Some(true)
    );
    // 替换输入当前元素文本。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用输入 nonce。
        TYPE_REQUEST_NONCE,
        // 绑定当前页面。
        page_ref,
        // 使用确认过的替换输入。
        json!({
            // 使用 type 操作。
            "kind": "type",
            // 显式确认。
            "confirmed": true,
            // 绑定当前元素。
            "elementRef": element_ref,
            // 输入固定文本。
            "text": "fixture text",
            // 替换现有文本。
            "replace": true,
        }),
    ));
    // 输入必须先 accepted。
    let _ = fixture.read_frame();
    // 读取输入 final。
    let typed = fixture.read_frame();
    // UTF-8 字节数必须精确。
    assert_eq!(
        // 读取字节数。
        typed.pointer("/data/utf8Bytes").and_then(Value::as_u64),
        // ASCII fixture 文本共十二字节。
        Some(12)
    );
    // 捕获固定 PNG。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用截图 nonce。
        SCREENSHOT_REQUEST_NONCE,
        // 绑定当前页面。
        page_ref,
        // 截图不接受额外参数。
        json!({ "kind": "screenshot" }),
    ));
    // 截图必须先 accepted。
    let _ = fixture.read_frame();
    // 读取截图 final。
    let screenshot = fixture.read_frame();
    // PNG 尺寸必须来自 IHDR。
    assert_eq!(
        // 读取宽度。
        screenshot.pointer("/data/width").and_then(Value::as_u64),
        // fixture 为一像素宽。
        Some(1)
    );
    // PNG 高度必须来自 IHDR。
    assert_eq!(
        // 读取高度。
        screenshot.pointer("/data/height").and_then(Value::as_u64),
        // fixture 为一像素高。
        Some(1)
    );
    // 摘要必须为固定十六进制形状。
    assert!(
        screenshot
            // 读取摘要。
            .pointer("/data/digest")
            // 转换为字符串。
            .and_then(Value::as_str)
            // 核对固定形状。
            .is_some_and(
                |value| value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            )
    );
    // 使用伪造元素引用验证 dispatch 前 stale 门禁。
    fixture.write(&page_request(
        // 绑定当前会话。
        session_id,
        // 使用 stale nonce。
        STALE_ELEMENT_NONCE,
        // 绑定当前页面。
        page_ref,
        // 伪造 canonical 但未签发的元素引用。
        json!({
            // 使用点击操作。
            "kind": "click",
            // 显式确认。
            "confirmed": true,
            // 使用未签发引用。
            "elementRef": "w1:be:00000000000000000000000000000000",
        }),
    ));
    // stale 元素不得先 accepted。
    let stale_element = fixture.read_frame();
    // 核对确定未派发。
    assert_eq!(
        // 读取 outcome。
        stale_element.get("outcome").and_then(Value::as_str),
        // 必须未派发。
        Some("not-dispatched")
    );
    // 核对元素 stale 错误码。
    assert_eq!(
        // 读取错误码。
        stale_element.pointer("/error/code").and_then(Value::as_str),
        // 使用稳定分类。
        Some("STALE_ELEMENT")
    );
    // 使用旧代际和缺失页面引用再次请求导航。
    fixture.write(&navigate_request(session_id, None, 0));
    // stale 门禁不得先输出 accepted。
    let stale = fixture.read_frame();
    // 核对确定未派发。
    assert_eq!(
        // 读取 outcome。
        stale.get("outcome").and_then(Value::as_str),
        // stale 必须在 CDP dispatch 前失败。
        Some("not-dispatched")
    );
    // 核对稳定 stale 错误码。
    assert_eq!(
        // 读取安全错误码。
        stale.pointer("/error/code").and_then(Value::as_str),
        // 使用页面 stale 分类。
        Some("STALE_PAGE")
    );
    // 发送关联 cancel 关闭会话。
    fixture.write(&cancel_request());
    // 正常关闭返回零。
    assert_eq!(fixture.close_and_wait(), 0);
    // profile 根必须在浏览器退出后被删除。
    assert!(!Path::new(&profile_root).exists());
}

// 验证 deadline 在 accepted 后产生确定失败并清理资源。
#[test]
// 使用永不就绪 runtime fixture。
fn opening_deadline_reaps_runtime_and_reports_failed() {
    // 启动永不就绪 runtime。
    let mut fixture = WorkerFixture::spawn(Some("never-ready"));
    // 保存待验证 profile 根。
    let profile_root = fixture.profile_root();
    // 使用短而合法的总 deadline。
    fixture.write(&open_request(50));
    // dispatch 前必须先 accepted。
    assert_accepted(&fixture.read_frame());
    // deadline 后必须产生 final。
    let final_frame = fixture.read_frame();
    // 核对确定失败。
    assert_outcome(&final_frame, "failed");
    // 核对稳定 deadline 错误码。
    assert_eq!(
        final_frame.pointer("/error/code").and_then(Value::as_str),
        Some("DEADLINE_EXCEEDED")
    );
    // worker 已自行结束。
    assert_eq!(fixture.close_and_wait(), 2);
    // 失败路径同样清理 profile。
    assert!(!Path::new(&profile_root).exists());
}

// 验证关联取消在 ready 前确定终止 runtime。
#[test]
// 使用永不就绪 runtime fixture 触发取消路径。
fn opening_cancel_reaps_runtime_and_reports_failed() {
    // 启动永不就绪 runtime。
    let mut fixture = WorkerFixture::spawn(Some("never-ready"));
    // 发送有充足预算的打开请求。
    fixture.write(&open_request(5_000));
    // 取得 accepted 事实。
    assert_accepted(&fixture.read_frame());
    // 发送关联取消。
    fixture.write(&cancel_request());
    // worker 必须输出确定失败。
    let final_frame = fixture.read_frame();
    // 核对失败结果。
    assert_outcome(&final_frame, "failed");
    // 核对稳定取消错误码。
    assert_eq!(
        final_frame.pointer("/error/code").and_then(Value::as_str),
        Some("CANCELLED")
    );
    // worker 必须完整退出。
    assert_eq!(fixture.close_and_wait(), 2);
}

// 验证 runtime 异常退出不会伪造 ready。
#[test]
// 使用固定提前退出 runtime。
fn abnormal_runtime_exit_is_a_correlated_failure() {
    // 启动提前退出模式。
    let mut fixture = WorkerFixture::spawn(Some("exit"));
    // 发送打开请求。
    fixture.write(&open_request(5_000));
    // spawn 前必须先 accepted。
    assert_accepted(&fixture.read_frame());
    // 读取确定失败。
    let final_frame = fixture.read_frame();
    // 核对失败结果。
    assert_outcome(&final_frame, "failed");
    // 核对稳定异常退出错误码。
    assert_eq!(
        final_frame.pointer("/error/code").and_then(Value::as_str),
        Some("BROWSER_EXITED")
    );
    // worker 返回失败退出码。
    assert_eq!(fixture.close_and_wait(), 2);
}

// 验证 CDP WebSocket 断开不会在仅有 HTTP 探测时伪造 ready。
#[test]
// 使用固定握手前断开 runtime。
fn cdp_disconnect_before_target_bootstrap_is_a_correlated_failure() {
    // 启动 CDP 断开模式。
    let mut fixture = WorkerFixture::spawn(Some("cdp-disconnect"));
    // 发送有充足预算的打开请求。
    fixture.write(&open_request(5_000));
    // spawn 前必须先输出 accepted。
    assert_accepted(&fixture.read_frame());
    // 读取 target 初始化失败 final。
    let final_frame = fixture.read_frame();
    // 核对确定失败而非 ready。
    assert_outcome(&final_frame, "failed");
    // 核对稳定协议断开错误码。
    assert_eq!(
        // 读取统一错误码。
        final_frame.pointer("/error/code").and_then(Value::as_str),
        // 断开必须保留传输分类。
        Some("BROWSER_PROTOCOL_DISCONNECTED")
    );
    // worker 必须回收并失败退出。
    assert_eq!(fixture.close_and_wait(), 2);
}

// 验证零帧输入不会制造未关联 final。
#[test]
// 直接关闭 stdin 模拟 parent 在 open 前断开。
fn zero_frame_disconnect_has_no_protocol_output() {
    // 启动生产 worker。
    let mut fixture = WorkerFixture::spawn(None);
    // 关闭输入触发零帧退出。
    drop(fixture.stdin);
    // 读取完整 stdout。
    let mut output = String::new();
    // 零帧路径必须到达 EOF。
    fixture
        // 借用 stdout reader。
        .stdout
        // 读取剩余输出。
        .read_to_string(&mut output)
        // 测试读取失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("zero-frame stdout failed: {error}"));
    // 等待 worker 退出。
    let status = fixture
        // 回收进程。
        .child
        // 等待固定快速退出。
        .wait()
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("zero-frame wait failed: {error}"));
    // worker 必须失败。
    assert!(!status.success());
    // 未取得可信 nonce 时不得输出帧。
    assert!(output.is_empty());
    // 清理测试临时根。
    std::fs::remove_dir_all(&fixture.temporary_root)
        // 清理失败暴露测试污染。
        .unwrap_or_else(|error| panic!("zero-frame cleanup failed: {error}"));
}
