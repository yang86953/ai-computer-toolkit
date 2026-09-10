#![cfg(target_os = "windows")]

//! 验证真实 broker stdio 纵切可达 click、type 与 screenshot。

// 导入测试 sibling 复制、路径与进程启动工具。
use std::{
    // 导入测试目录内的固定镜像复制与临时根创建。
    fs,
    // 导入测试 helper 的路径借用与所有权。
    path::{Path, PathBuf},
    // 导入带固定测试环境的 broker 进程启动器。
    process::{Command, Stdio},
};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入只读前景窗口查询。
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

// 导入父集成测试的固定 sibling、页面服务器与断言 helper。
use super::{
    // 导入生产 broker 固定 sibling 名称。
    BROKER_FILE_NAME,
    // 导入生产 broker 源镜像。
    BROKER_SOURCE,
    // 导入 broker 进程 owner。
    BrokerProcess,
    // 导入认证 launcher 固定 sibling 名称。
    COMMAND_FILE_NAME,
    // 导入认证 launcher 源镜像。
    COMMAND_SOURCE,
    // 导入完整 broker 操作预算。
    COMMAND_TIMEOUT_MS,
    // 导入测试断言提取 trait。
    Must,
    // 导入测试目录 owner。
    TestDirectory,
    // 导入生产 worker 固定 sibling 名称。
    WORKER_FILE_NAME,
    // 导入精确字段断言。
    assert_exact_keys,
    // 导入不泄漏私有事实断言。
    assert_no_private_text,
    // 导入 worker 零残留断言。
    assert_no_worker_now,
    // 导入 launcher 输出解析器。
    output_json,
    // 导入独立 launcher 执行器。
    run_launcher,
};
// 固定 Cargo 构建的真实生产 Browser Session worker。
const PRODUCTION_WORKER_SOURCE: &str =
    // 由 Cargo 为当前集成测试提供精确二进制路径。
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker");
// 固定 Cargo 构建的仓库自有 CDP runtime fixture。
const RUNTIME_SOURCE: &str =
    // 由 Cargo 为当前集成测试提供精确二进制路径。
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-runtime-fixture");
// 固定测试目录中的 runtime fixture 文件名。
const RUNTIME_FILE_NAME: &str = "ai-computer-toolkit-browser-session-runtime-fixture.exe";
// 固定 worker fixture 接受但不会公开回显的页面 URL。
const FIXTURE_PAGE_URL: &str = "https://example.test/page";

// 固定 runtime fixture 返回的一像素 PNG Base64。
const FIXTURE_PNG_BASE64: &str =
    // 保持测试期望不依赖生产私有 CDP 类型。
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

// 安装 production broker、认证 launcher、真实 worker 与 runtime fixture。
fn install_production_siblings(
    // 借用本测试独占目录。
    directory: &TestDirectory,
) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    // 定位固定 broker sibling。
    let broker = directory.file(BROKER_FILE_NAME);
    // 定位固定认证 launcher sibling。
    let command = directory.file(COMMAND_FILE_NAME);
    // 定位生产 worker 固定 sibling。
    let worker = directory.file(WORKER_FILE_NAME);
    // 定位测试专用 runtime fixture。
    let runtime = directory.file(RUNTIME_FILE_NAME);
    // 复制真实 production broker。
    fs::copy(BROKER_SOURCE, &broker).must("production broker sibling copy should succeed");
    // 复制认证 launcher fixture 到生产主程序名称。
    fs::copy(COMMAND_SOURCE, &command).must("command fixture sibling copy should succeed");
    // 复制真实 production worker 到固定 companion 名称。
    fs::copy(PRODUCTION_WORKER_SOURCE, &worker)
        // 复制失败必须停止整链验收。
        .must("production worker sibling copy should succeed");
    // 复制仓库自有 CDP runtime fixture。
    fs::copy(RUNTIME_SOURCE, &runtime).must("runtime fixture sibling copy should succeed");
    // 返回整条物理纵切的精确镜像集合。
    (broker, command, worker, runtime)
}

// 启动只通过测试环境选择仓库自有 runtime fixture 的真实 broker。
fn start_broker_with_runtime(
    // 借用 production broker sibling。
    image: &Path,
    // 借用 production worker 唯一允许发现的 runtime fixture。
    runtime: &Path,
    // 借用测试自有 profile 临时根。
    temporary_root: &Path,
) -> BrokerProcess {
    // 创建无参数 production broker 命令。
    let mut command = Command::new(image);
    // 让真实 worker 只发现仓库自有 runtime fixture。
    command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", runtime);
    // 隔离 worker 创建的空 profile 根。
    command.env("TEMP", temporary_root);
    // 同步 Windows 的第二个临时目录变量。
    command.env("TMP", temporary_root);
    // broker 不继承测试 stdin。
    command.stdin(Stdio::null());
    // 捕获 broker 的固定安全 stdout。
    command.stdout(Stdio::piped());
    // 禁止测试旁路输出生产内部诊断。
    command.stderr(Stdio::null());
    // 启动真实 production broker。
    let child = command
        // 不传入任何 argv 或私有控制面。
        .spawn()
        // 启动失败必须停止整链验收。
        .must("production broker should start with the runtime fixture environment");
    // 建立当前测试唯一 broker owner。
    let mut broker = BrokerProcess { child };
    // 在 launcher 连接前等待固定 endpoint 发布。
    broker.wait_until_listening();
    // 返回已发布 endpoint 的 broker。
    broker
}

// 验证 click、type、screenshot 经真实 broker/dispatcher/System/Module 完成。
#[test]
fn production_broker_routes_page_actions_and_screenshot_without_foreground_change() {
    // 创建不接触生产安装目录的 sibling 目录。
    let directory = TestDirectory::create();
    // 安装 production broker、认证 launcher、真实 worker 与 runtime fixture。
    let (broker_image, command_image, worker_image, runtime_image) =
        // 使用固定 sibling 集合，不接触生产安装目录。
        install_production_siblings(&directory);
    // 创建 worker profile 的测试专用临时根。
    let temporary_root = directory.file("profiles");
    // 临时根创建失败必须停止生命周期验收。
    fs::create_dir(&temporary_root).must("browser profile root should be created");
    // 启动真实 production broker host 并限定 runtime fixture。
    let mut broker = start_broker_with_runtime(&broker_image, &runtime_image, &temporary_root);
    // 记录页面动作前宿主前景窗口。
    let foreground_before = unsafe { GetForegroundWindow() };
    // 经独立认证 launcher 执行完整页面动作纵切。
    let output = run_launcher(
        // 使用固定主程序 sibling。
        &command_image,
        // 只传固定 operation、本地 URL 与总预算。
        json!({ "operation": "page-action-roundtrip", "url": FIXTURE_PAGE_URL, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 页面动作返回后再次只读宿主前景窗口。
    let foreground_after = unsafe { GetForegroundWindow() };
    // 页面动作结束后 broker host 必须仍可服务后续请求。
    assert!(
        broker.is_running(),
        "page action roundtrip must not stop the broker host"
    );
    // 整条生产纵切不得改变宿主前景。
    assert_eq!(
        foreground_after, foreground_before,
        "page actions must preserve the host foreground window"
    );
    // 解析唯一安全 JSON 输出。
    let envelope = output_json(&output);
    // 三项 operation、stale 预检与最终 close 必须全部成功。
    assert!(
        // 检查 launcher 退出状态。
        output.status.success(),
        // 失败时只报告公开稳定错误码。
        "page action roundtrip failed with {} at {} completed={:?}",
        // 读取安全 error code 或固定占位符。
        envelope
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or("UNAVAILABLE"),
        // 读取固定阶段标签或占位符。
        envelope
            .pointer("/error/details/stage")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        // 读取安全完成事实以区分 wire 恢复与 broker unknown。
        envelope
            .pointer("/error/details/completed")
            .and_then(Value::as_bool),
    );
    // 成功 envelope 只允许 ok 与 result。
    assert_exact_keys(&envelope, &["ok", "result"]);
    // 读取完整页面结果。
    let result = envelope
        // 取得 result 对象。
        .get("result")
        // 成功必须携带 result。
        .expect("page action roundtrip should return result");
    // 页面结果只允许冻结公开字段。
    assert_exact_keys(
        // 检查 result 对象。
        result,
        // 固定允许的页面动作字段集合。
        &[
            "pageId",
            "elementId",
            "navigationGeneration",
            "clicked",
            "typed",
            "utf8Bytes",
            "staleElementRejected",
            "screenshot",
            "closed",
        ],
    );
    // navigate 必须签发 canonical page identity。
    assert!(
        result
            .get("pageId")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("s2:bp:") && value.len() == 38)
    );
    // query 必须签发 canonical element identity。
    assert!(
        result
            .get("elementId")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("s2:be:") && value.len() == 38)
    );
    // 首次导航代际必须为正数。
    assert!(
        result
            .get("navigationGeneration")
            .and_then(Value::as_u64)
            .is_some_and(|value| value >= 1)
    );
    // click completed 必须可信完成。
    assert_eq!(result.get("clicked").and_then(Value::as_bool), Some(true));
    // type completed 必须可信完成。
    assert_eq!(result.get("typed").and_then(Value::as_bool), Some(true));
    // 固定文本只输出 UTF-8 字节数，不回显原文。
    assert_eq!(result.get("utf8Bytes").and_then(Value::as_u64), Some(12));
    // canonical 未签发 element 必须在业务接受前拒绝。
    assert_eq!(
        result.get("staleElementRejected").and_then(Value::as_bool),
        Some(true)
    );
    // roundtrip 必须完成 session 回收。
    assert_eq!(result.get("closed").and_then(Value::as_bool), Some(true));
    // 读取有界截图投影。
    let screenshot = result
        // 取得 screenshot 对象。
        .get("screenshot")
        // screenshot 成功必须携带结果。
        .expect("screenshot should return data");
    // screenshot 只允许冻结 PNG 字段。
    assert_exact_keys(
        screenshot,
        &[
            "mimeType",
            "pngBase64",
            "pngBytes",
            "width",
            "height",
            "digest",
        ],
    );
    // MIME 必须固定为 PNG。
    assert_eq!(
        screenshot.get("mimeType").and_then(Value::as_str),
        Some("image/png")
    );
    // Base64 必须逐字等于 runtime fixture 的有界 PNG。
    assert_eq!(
        screenshot.get("pngBase64").and_then(Value::as_str),
        Some(FIXTURE_PNG_BASE64)
    );
    // PNG 原始字节数必须与固定资产一致。
    assert_eq!(screenshot.get("pngBytes").and_then(Value::as_u64), Some(68));
    // IHDR 宽度必须为一像素。
    assert_eq!(screenshot.get("width").and_then(Value::as_u64), Some(1));
    // IHDR 高度必须为一像素。
    assert_eq!(screenshot.get("height").and_then(Value::as_u64), Some(1));
    // 摘要必须是固定长度小写十六进制。
    assert!(
        screenshot
            .get("digest")
            .and_then(Value::as_str)
            .is_some_and(|value| value.len() == 16
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    );
    // 全部输出不得泄漏 transport、worker、epoch、nonce、profile 或 CDP 事实。
    assert_no_private_text(&envelope);
    // 序列化输出不得泄漏凭据、Cookie、原始输入或 native 实现字段。
    let serialized = envelope.to_string().to_ascii_lowercase();
    // 检查本批额外隐私词集合。
    for forbidden in ["cookie", "credential", "fixture text", "native"] {
        // 禁止任一额外隐私事实出现。
        assert!(
            !serialized.contains(forbidden),
            "page action output leaked private data"
        );
    }
    // close 完成后固定页面协议 worker 不得残留。
    assert_no_worker_now(&worker_image);
    // 显式停止测试拥有的 broker。
    broker.stop();
    // 所有进程退出后隔离目录必须可确定删除。
    directory.finish();
}
