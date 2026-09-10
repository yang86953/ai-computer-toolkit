//! 固定 Chromium 参数的隔离浏览器截图 worker 协议入口。

// 导入有界标准输入输出、文件、进程与时间工具。
use std::{
    // 导入文件读取。
    fs,
    // 导入标准输入与输出 trait。
    io::{Read, Write},
    // 导入路径类型。
    path::Path,
    // 导入子进程与无窗口 stdio 配置。
    process::{Child, Command, ExitStatus, Stdio},
    // 导入轮询休眠。
    thread,
    // 导入单调 deadline。
    time::{Duration, Instant},
};

// 导入严格请求反序列化。
use serde::Deserialize;
// 导入语言中立 JSON。
use serde_json::{Value, json};

// 把错误码实现保留为 Browser Worker 协议边界的普通私有类型。
#[path = "browser_worker_error.rs"]
mod error_code;
// 导入当前 Worker 私有封闭错误码。
use error_code::BrowserWorkerErrorCode;

// 导入前景门禁、runtime、摘要与领域错误。
use crate::{
    // 导入前景不变检查与只读 HWND 快照。
    adapters::{window::ensure_foreground_unchanged, windows::foreground_hwnd},
    // 导入私有 Chromium 发现与稳定字节摘要。
    components::{browser_runtime, byte_digest},
    // 导入结构化错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 worker 协议版本。
const CONTRACT_VERSION: &str = "act/browser-screenshot-worker/v1";
// 限制请求为 64KiB。
const MAXIMUM_REQUEST_BYTES: u64 = 64 * 1024;
// 限制 PNG 为 64MiB。
const MAXIMUM_PNG_BYTES: u64 = 64 * 1024 * 1024;
// 固定 PNG 文件签名。
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
// 固定工具自有 profile 根目录名。
const PROFILE_ROOT_NAME: &str = "ai-computer-toolkit-browser";
// 固定工具自有 profile 子目录前缀。
const PROFILE_NAME_PREFIX: &str = "act-browser-";

// 定义严格版本化 worker 请求。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝协议外字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerRequest {
    // 保存协议版本。
    contract_version: String,
    // 保存逐操作确认。
    confirmed: bool,
    // 保存唯一 URL。
    url: String,
    // 保存视口宽度。
    width: u32,
    // 保存视口高度。
    height: u32,
    // 保存浏览器 deadline。
    timeout_ms: u32,
    // 保存父 Module 独占 staging 路径。
    staging_path: String,
    // 保存父 Module 独占 profile 路径。
    profile_path: String,
}

// 保存验证后的 PNG 事实。
struct PngEvidence {
    // 保存实际字节数。
    bytes: u64,
    // 保存 IHDR 宽度。
    width: u32,
    // 保存 IHDR 高度。
    height: u32,
    // 保存完整 PNG 摘要。
    digest: String,
}

// 严格解析并按确认优先顺序验证请求。
fn parse_request(text: &str) -> AppResult<WorkerRequest> {
    // 兼容 Windows stdio 的单个 UTF-8 BOM。
    let normalized = text.trim_start_matches('\u{feff}').trim();
    // 只接受一个完整 JSON 对象。
    let request = serde_json::from_str::<WorkerRequest>(normalized).map_err(|_| {
        // 不回显输入内容。
        invalid_argument("The browser worker request violates protocol v1.")
    })?;
    // worker 必须先独立核对显式确认。
    if !request.confirmed {
        // 确认缺失优先于 URL、profile 与路径解析。
        return Err(BrowserWorkerErrorCode::ConfirmationRequired.error(
            // 不回显输入。
            "Isolated browser screenshot requires explicit confirmation.",
        ));
    }
    // 协议版本必须精确匹配。
    if request.contract_version != CONTRACT_VERSION {
        // 未知版本不得扩展 worker 控制面。
        return Err(BrowserWorkerErrorCode::CapabilityGap.error(
            // 说明固定版本。
            "The browser worker accepts screenshot protocol v1 only.",
        ));
    }
    // URL 必须非空、有界并使用认证 scheme。
    if !(1..=8_192).contains(&request.url.len())
        // 只允许三个认证 scheme。
        || !(request.url.starts_with("https://")
            // 允许明文 HTTP 以保持契约兼容。
            || request.url.starts_with("http://")
            // 允许仓库自有 file fixture。
            || request.url.starts_with("file://"))
    {
        // URL 不合法时拒绝启动 Chromium。
        return Err(invalid_argument(
            // 不回显 URL 内容。
            "The browser worker accepts bounded http, https or file URLs only.",
        ));
    }
    // 视口必须位于固定公开范围。
    if !(1..=10_000).contains(&request.width) || !(1..=10_000).contains(&request.height) {
        // 越界尺寸不得静默收缩。
        return Err(invalid_argument(
            // 说明固定范围。
            "The browser worker viewport must be 1..=10000.",
        ));
    }
    // deadline 必须位于固定公开范围。
    if !(1_000..=300_000).contains(&request.timeout_ms) {
        // 拒绝无界或过短运行。
        return Err(invalid_argument(
            // 说明固定范围。
            "The browser worker timeout must be 1000..=300000ms.",
        ));
    }
    // staging 必须仍为父 Module 独占的普通空文件。
    validate_staging(Path::new(&request.staging_path))?;
    // profile 必须仍为工具自有根下的独占子目录。
    validate_profile(Path::new(&request.profile_path))?;
    // 返回完全验证的请求。
    Ok(request)
}

// 执行固定 Chromium 截图模板。
fn execute_request(request: WorkerRequest) -> AppResult<Value> {
    // worker 独立私下发现 runtime，不接受 executable 字段。
    let browser = browser_runtime::find().ok_or_else(|| {
        // 缺失 runtime 不降级到用户浏览器。
        BrowserWorkerErrorCode::BrowserUnavailable.error(
            // 不公开搜索路径。
            "No certified Chromium runtime is available for isolated capture.",
        )
    })?;
    // 冻结路径借用。
    let staging = Path::new(&request.staging_path);
    // 冻结 profile 借用。
    let profile = Path::new(&request.profile_path);
    // 记录 Chromium 启动前前景窗口。
    let foreground_before = foreground_hwnd();
    // 构造固定无 shell 命令。
    let mut command = hidden_command(&browser);
    // 添加认证的固定无头参数。
    command.args([
        // 使用当前 Chromium 无头实现。
        "--headless=new",
        // 禁止首次运行流程。
        "--no-first-run",
        // 禁止默认浏览器提示。
        "--no-default-browser-check",
        // 等待 compositor 完成本帧。
        "--run-all-compositor-stages-before-draw",
        // 禁止后台网络组件干扰 fixture。
        "--disable-background-networking",
    ]);
    // 只使用父 Module 独占 profile。
    command.arg(format!("--user-data-dir={}", profile.display()));
    // 只写父 Module 独占 staging。
    command.arg(format!("--screenshot={}", staging.display()));
    // 使用有界视口动态槽。
    command.arg(format!(
        "--window-size={},{}",
        request.width, request.height
    ));
    // URL 始终作为独立 argv，不经过 shell。
    command.arg(&request.url);
    // worker stdout 必须只保留协议 JSON。
    command.stdout(Stdio::null());
    // Chromium 诊断不进入 worker stderr 协议。
    command.stderr(Stdio::null());
    // 启动固定 runtime。
    let mut child = command.spawn().map_err(|_| {
        // 不公开 runtime 路径或 OS 错误。
        BrowserWorkerErrorCode::BrowserStartFailed.error(
            // 使用安全诊断。
            "The isolated Chromium process could not be started.",
        )
    })?;
    // 保存执行结果以便前景核验优先于候选处理。
    let status = wait_for_exit(&mut child, request.timeout_ms);
    // 记录 Chromium 退出或被终止后的前景窗口。
    let foreground_after = foreground_hwnd();
    // worker 独立拒绝任何前景变化。
    ensure_foreground_unchanged(foreground_before, foreground_after)
        // 映射为浏览器兼容错误码。
        .map_err(|_| foreground_changed())?;
    // 传播 timeout 或等待失败。
    let status = status?;
    // 非零退出码拒绝候选。
    if !status.success() {
        // 不公开具体退出码或 runtime 细节。
        return Err(BrowserWorkerErrorCode::BrowserFailed.error(
            // 使用封闭诊断。
            "The isolated Chromium process did not complete successfully.",
        ));
    }
    // 重新验证 staging 类型，防止 runtime 替换为链接或目录。
    validate_staging_type(staging)?;
    // 读取并验证 PNG signature、IHDR、尺寸与上限。
    let png = inspect_png(staging, request.width, request.height)?;
    // 返回不含路径和原生标识的封闭候选事实。
    Ok(json!({
        // 输出实际 PNG 字节数。
        "pngBytes": png.bytes,
        // 输出 IHDR 宽度。
        "width": png.width,
        // 输出 IHDR 高度。
        "height": png.height,
        // 输出稳定内容摘要。
        "pngDigest": png.digest,
        // 声明候选已完整写入。
        "candidateWritten": true,
        // 声明 profile 独占。
        "isolatedProfile": true,
        // 声明 worker 前景不变。
        "foregroundUnchanged": true,
    }))
}

// 等待 Chromium 到达 deadline。
fn wait_for_exit(child: &mut Child, timeout_ms: u32) -> AppResult<ExitStatus> {
    // 记录单调起点。
    let started = Instant::now();
    // 有界轮询子进程状态。
    loop {
        // 尝试非阻塞取得退出状态。
        match child.try_wait() {
            // 正常退出时返回状态。
            Ok(Some(status)) => return Ok(status),
            // 仍运行且 deadline 已到。
            Ok(None) if started.elapsed() >= Duration::from_millis(u64::from(timeout_ms)) => {
                // 终止直接子进程；父 Job 随后回收完整进程树。
                let _ = child.kill();
                // 等待直接子进程释放句柄。
                let _ = child.wait();
                // 返回稳定浏览器超时。
                return Err(BrowserWorkerErrorCode::BrowserTimeout.error(
                    // 说明隔离任务已终止。
                    "The isolated Chromium process exceeded its deadline and was terminated.",
                ));
            }
            // 仍运行且未超时。
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            // 无法读取进程状态时终止直接子进程。
            Err(_) => {
                // 尝试终止直接子进程。
                let _ = child.kill();
                // 等待句柄回收。
                let _ = child.wait();
                // 返回稳定执行失败。
                return Err(BrowserWorkerErrorCode::BrowserFailed.error(
                    // 不公开 OS 错误。
                    "The isolated Chromium process could not be monitored.",
                ));
            }
        }
    }
}

// 构造无控制台窗口的进程命令。
fn hidden_command(program: &Path) -> Command {
    // 创建不经过 shell 的命令。
    let mut command = Command::new(program);
    // 仅 Windows 安装 CREATE_NO_WINDOW。
    #[cfg(windows)]
    // 限制平台私有代码块。
    {
        // 导入 Windows CommandExt。
        use std::os::windows::process::CommandExt;
        // 固定不创建控制台窗口标志。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // 安装无窗口创建标志。
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // 返回固定命令模板。
    command
}

// 验证父 Module 预留的 staging 空文件。
fn validate_staging(path: &Path) -> AppResult<()> {
    // 先验证类型。
    let metadata = validate_staging_type(path)?;
    // worker 启动前 staging 必须仍为空。
    if metadata.len() != 0 {
        // 拒绝复用或已污染候选。
        return Err(invalid_output_path());
    }
    // 返回预留验证成功。
    Ok(())
}

// 验证 staging 是不跟随链接的普通文件。
fn validate_staging_type(path: &Path) -> AppResult<fs::Metadata> {
    // 使用不跟随元数据读取。
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_output_path())?;
    // 只接受真实普通文件。
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        // 拒绝链接、目录与特殊文件。
        return Err(invalid_output_path());
    }
    // 返回类型证据。
    Ok(metadata)
}

// 验证 profile 位于固定工具临时根下。
fn validate_profile(path: &Path) -> AppResult<()> {
    // 使用不跟随元数据拒绝 profile 链接。
    let metadata = fs::symlink_metadata(path).map_err(|_| profile_error())?;
    // 只接受真实目录。
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        // profile 边界失效时失败。
        return Err(profile_error());
    }
    // 文件名必须使用工具固定前缀。
    if !path
        // 读取最后一个路径片段。
        .file_name()
        // 转换为 UTF-8 仅用于前缀验证。
        .and_then(|value| value.to_str())
        // 核对固定前缀。
        .is_some_and(|value| value.starts_with(PROFILE_NAME_PREFIX))
    {
        // 非工具目录不得作为 profile。
        return Err(profile_error());
    }
    // 构造固定预期根。
    let expected_root = std::env::temp_dir().join(PROFILE_ROOT_NAME);
    // 规范化真实根路径。
    let canonical_root = fs::canonicalize(&expected_root).map_err(|_| profile_error())?;
    // 规范化 profile 父目录。
    let canonical_parent = path
        // 取得父目录。
        .parent()
        // 缺失父目录失败。
        .ok_or_else(profile_error)
        // 解析真实父目录。
        .and_then(|parent| fs::canonicalize(parent).map_err(|_| profile_error()))?;
    // profile 必须是固定根的直接子目录。
    if canonical_parent != canonical_root {
        // 禁止跨出工具临时根。
        return Err(profile_error());
    }
    // 返回 profile 所有权形状验证成功。
    Ok(())
}

// 读取并验证 PNG 固定头和尺寸。
fn inspect_png(path: &Path, expected_width: u32, expected_height: u32) -> AppResult<PngEvidence> {
    // 读取候选元数据。
    let metadata = validate_staging_type(path)?;
    // 应用 PNG 最短头与公开字节上限。
    if !(24..=MAXIMUM_PNG_BYTES).contains(&metadata.len()) {
        // 空、截断或过大候选失败。
        return Err(screenshot_missing());
    }
    // 读取有界候选全部字节供摘要。
    let bytes = fs::read(path).map_err(|_| screenshot_missing())?;
    // 签名必须精确匹配 PNG。
    if bytes.get(0..8) != Some(PNG_SIGNATURE.as_slice()) {
        // 错误容器失败闭合。
        return Err(screenshot_missing());
    }
    // IHDR 宽度位于固定大端偏移。
    let width = u32::from_be_bytes(
        // 将四字节切片转换为数组。
        bytes[16..20]
            // 执行固定长度转换。
            .try_into()
            // 前置长度门禁保证成功，否则仍失败闭合。
            .map_err(|_| screenshot_missing())?,
    );
    // IHDR 高度位于固定大端偏移。
    let height = u32::from_be_bytes(
        // 将四字节切片转换为数组。
        bytes[20..24]
            // 执行固定长度转换。
            .try_into()
            // 前置长度门禁保证成功，否则仍失败闭合。
            .map_err(|_| screenshot_missing())?,
    );
    // 实际尺寸必须精确匹配请求。
    if width != expected_width || height != expected_height {
        // 禁止接受 Chromium  silently changed viewport。
        return Err(screenshot_missing());
    }
    // 计算稳定完整容器摘要。
    let digest = byte_digest::digest(&bytes);
    // 返回封闭 PNG 事实。
    Ok(PngEvidence {
        // 保存元数据长度。
        bytes: metadata.len(),
        // 保存宽度。
        width,
        // 保存高度。
        height,
        // 保存摘要。
        digest,
    })
}

// 构造 worker 成功 envelope。
fn success_envelope(data: Value) -> Value {
    // 返回固定三字段成功形状。
    json!({
        // 标记成功。
        "ok": true,
        // 输出固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出封闭候选事实。
        "data": data,
    })
}

// 构造 worker 失败 envelope。
fn error_envelope(error: &AppControlError) -> Value {
    // 返回不含路径、PID 或原生事实的错误。
    json!({
        // 标记失败。
        "ok": false,
        // 输出固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出固定错误对象。
        "error": {
            // 输出稳定错误码。
            "code": error.code,
            // 输出安全消息。
            "message": error.message,
        },
    })
}

// 从标准输入执行一次请求并向标准输出写一行 JSON。
pub fn run_stdio() -> i32 {
    // 创建请求文本缓冲区。
    let mut input = String::new();
    // 将读取限制为 64KiB 加一个溢出探针字节。
    let read_result = std::io::stdin()
        // 安装读取上限。
        .take(MAXIMUM_REQUEST_BYTES.saturating_add(1))
        // 读取 UTF-8 文本。
        .read_to_string(&mut input);
    // 解析或执行请求。
    let result = match read_result {
        // 超过硬上限时拒绝。
        Ok(_) if u64::try_from(input.len()).unwrap_or(u64::MAX) > MAXIMUM_REQUEST_BYTES => {
            // 返回参数错误。
            Err(invalid_argument(
                // 不回显输入。
                "The browser worker request exceeded 64 KiB.",
            ))
        }
        // 成功读取后解析并执行。
        Ok(_) => parse_request(&input).and_then(execute_request),
        // 读取失败时结构化返回。
        Err(_) => Err(invalid_argument(
            // 不公开 I/O 细节。
            "The browser worker could not read its request.",
        )),
    };
    // 投影 envelope 与退出码。
    let (envelope, exit_code) = match result {
        // 成功返回零。
        Ok(data) => (success_envelope(data), 0),
        // 失败返回二。
        Err(error) => (error_envelope(&error), 2),
    };
    // 序列化单行 JSON。
    let text = match serde_json::to_string(&envelope) {
        // 保存成功文本。
        Ok(text) => text,
        // 极端序列化失败只能用进程码报告。
        Err(_) => return 2,
    };
    // 只向 stdout 写入协议结果。
    if writeln!(std::io::stdout(), "{text}").is_err() {
        // 管道关闭时返回失败。
        return 2;
    }
    // 返回协议退出码。
    exit_code
}

// 构造稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回固定参数错误码。
    BrowserWorkerErrorCode::InvalidArgument.error(message)
}

// 构造 staging 路径错误。
fn invalid_output_path() -> AppControlError {
    // 返回不含路径的稳定错误。
    BrowserWorkerErrorCode::InvalidOutputPath.error(
        // 说明 reservation 失效。
        "The browser screenshot staging file is not an owned regular file.",
    )
}

// 构造 profile 边界错误。
fn profile_error() -> AppControlError {
    // 返回不含路径的稳定错误。
    BrowserWorkerErrorCode::TempProfileFailed.error(
        // 说明工具所有权要求。
        "The browser profile is outside the owned temporary boundary.",
    )
}

// 构造截图候选缺失错误。
fn screenshot_missing() -> AppControlError {
    // 返回不含候选内容的稳定错误。
    BrowserWorkerErrorCode::ScreenshotMissing.error(
        // 说明 PNG 证明缺失。
        "The isolated browser did not produce the requested PNG dimensions.",
    )
}

// 构造前景变化错误。
fn foreground_changed() -> AppControlError {
    // 返回不含 HWND 的稳定错误。
    BrowserWorkerErrorCode::ForegroundChanged.error(
        // 说明候选必须拒绝。
        "The isolated browser unexpectedly changed the foreground window.",
    )
}

// 声明纯协议解析测试。
#[cfg(test)]
// 保持测试靠近 worker 私有协议。
mod tests {
    // 导入 JSON 夹具宏。
    use serde_json::json;

    // 导入父模块错误类型、envelope 与解析器。
    use super::{BrowserWorkerErrorCode, error_envelope, parse_request};

    // 验证确认优先于其他字段语义。
    #[test]
    // 使用完整形状但非法值的请求。
    fn missing_confirmation_is_rejected_first() {
        // 解析未确认请求。
        let Err(error) = parse_request(
            // 同时提供非法 URL 与路径以证明优先级。
            r#"{"contractVersion":"act/browser-screenshot-worker/v1","confirmed":false,"url":"--flag","width":0,"height":0,"timeoutMs":0,"stagingPath":"x","profilePath":"y"}"#,
        ) else {
            // 请求意外成功时终止。
            panic!("missing confirmation must fail");
        };
        // 确认错误必须优先。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    // 验证 Worker 自有错误保持 v1 失败 envelope。
    #[test]
    fn worker_owned_error_keeps_v1_envelope() {
        // 构造不含 provider 详情的浏览器执行失败。
        let error = BrowserWorkerErrorCode::BrowserFailed.error("worker fixture");
        // 把领域错误投影为固定 Worker envelope。
        let envelope = error_envelope(&error);
        // 核对协议版本、稳定码与安全消息。
        assert_eq!(
            envelope,
            // 期望值不得包含 details、路径或进程事实。
            json!({
                // 标记失败。
                "ok": false,
                // 保持 Browser Worker v1 协议版本。
                "contractVersion": "act/browser-screenshot-worker/v1",
                // 保持最小错误对象。
                "error": {
                    // 保持封闭错误码。
                    "code": "BROWSER_FAILED",
                    // 保持安全消息。
                    "message": "worker fixture",
                },
            })
        );
    }
}
