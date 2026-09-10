//! 组合隔离 Chromium、临时 profile 与原子 PNG 提交的浏览器截图 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "browser_screenshot_error.rs"]
mod error_code;

// 导入路径与 worker deadline。
use std::{path::PathBuf, time::Duration};

// 导入 JSON Map、值与构造器。
use serde_json::{Map, Value, json};

// 导入当前 Module 私有封闭错误码。
use error_code::BrowserScreenshotErrorCode;

// 导入私有前景探测、窄 Component 与领域错误。
use crate::{
    // 导入前景不变门禁。
    adapters::{window::ensure_foreground_unchanged, windows::foreground_hwnd},
    // 导入固定 capability ID。
    capabilities,
    // 导入原子输出、profile、runtime、取消与 worker Component。
    components::{
        // 导入原子单文件提交边界。
        atomic_file::{AtomicFileError, StagedFile},
        // 导入工具自有 profile 生命周期。
        browser_profile::BrowserProfile,
        // 导入私有 runtime 发现。
        browser_runtime,
        // 导入统一取消状态。
        cancellation,
        // 导入非跟随输出门禁。
        output_guard::{OutputGuardError, guard_file_output},
        // 导入 Job 约束 companion 执行器。
        worker_process,
    },
    // 导入语言中立错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 Rust 浏览器截图 worker 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-worker.exe";
// 固定内部 worker 协议版本。
const WORKER_CONTRACT_VERSION: &str = "act/browser-screenshot-worker/v1";
// 限制 worker stdout 为 64KiB。
const MAXIMUM_WORKER_OUTPUT_BYTES: usize = 64 * 1024;
// 限制 PNG 为 64MiB。
const MAXIMUM_PNG_BYTES: u64 = 64 * 1024 * 1024;
// 固定默认视口宽度。
const DEFAULT_WIDTH: u32 = 1_280;
// 固定默认视口高度。
const DEFAULT_HEIGHT: u32 = 720;
// 固定默认浏览器 deadline。
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
// 为 worker 结构化回收保留固定外层宽限。
const WORKER_REAP_GRACE_MS: u64 = 2_000;

// 保存确认后解析的浏览器截图输入。
#[derive(Clone, Debug, Eq, PartialEq)]
struct ScreenshotInput {
    // 保存有界 URL。
    url: String,
    // 保存最终 PNG 路径。
    path: String,
    // 保存视口宽度。
    width: u32,
    // 保存视口高度。
    height: u32,
    // 保存浏览器 deadline。
    timeout_ms: u32,
    // 保存独立覆盖许可。
    overwrite: bool,
}

// 保存经过严格协议验证的候选事实。
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkerScreenshot {
    // 保存 PNG 字节数。
    bytes: u64,
    // 保存视口宽度。
    width: u32,
    // 保存视口高度。
    height: u32,
    // 保存 PNG 字节摘要。
    png_digest: String,
}

// 执行 confirmation-first 的隔离浏览器截图。
pub(crate) fn screenshot(
    // 接收逐操作确认。
    confirmed: bool,
    // 接收唯一 URL 目标。
    target: &Map<String, Value>,
    // 接收封闭参数集合。
    args: &Map<String, Value>,
) -> AppResult<Value> {
    // 确认必须先于 URL、路径、runtime 与文件系统解析。
    if !confirmed {
        // 缺少确认时立即失败。
        return Err(BrowserScreenshotErrorCode::ConfirmationRequired.error(
            // 不回显 URL 或路径。
            "Isolated browser screenshot requires explicit confirmation.",
        ));
    }
    // 确认后解析全部封闭输入。
    let input = parse_input(target, args)?;
    // 在创建任何候选前私下确认 Chromium 可达。
    if browser_runtime::find().is_none() {
        // 缺失 runtime 明确失败且不降级前台。
        return Err(BrowserScreenshotErrorCode::BrowserUnavailable.error(
            // 不公开搜索路径。
            "No certified Chromium runtime is available for isolated capture.",
        ));
    }
    // 转换最终输出路径。
    let destination = PathBuf::from(&input.path);
    // 在 staging 前执行统一非跟随覆盖门禁。
    guard_file_output(&destination, input.overwrite).map_err(output_guard_error)?;
    // 创建本次调用独占的空 profile。
    let profile = BrowserProfile::create()?;
    // 独占目标同目录 staging 文件。
    let staged = StagedFile::reserve(&destination).map_err(atomic_file_error)?;
    // 内部协议只允许 UTF-8 staging 路径。
    let staging_path = staged.path().to_str().ok_or_else(invalid_output_path)?;
    // 内部协议只允许 UTF-8 profile 路径。
    let profile_path = profile.path().to_str().ok_or_else(profile_path_error)?;
    // 定位固定 Rust sibling worker。
    let worker = worker_process::sibling_companion_path(
        // 只使用编译期文件名。
        WORKER_FILE_NAME,
        // 使用不含路径的安全描述。
        "browser screenshot worker",
    )?;
    // 构造严格版本化 worker 请求。
    let request = json!({
        // 固定协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 重复传递确认供 worker 独立核对。
        "confirmed": true,
        // 只传递封闭 URL。
        "url": input.url,
        // 只传递有界视口宽度。
        "width": input.width,
        // 只传递有界视口高度。
        "height": input.height,
        // 只传递有界浏览器 deadline。
        "timeoutMs": input.timeout_ms,
        // 只传递父 Module 独占 staging。
        "stagingPath": staging_path,
        // 只传递父 Module 独占 profile。
        "profilePath": profile_path,
    });
    // 记录运行前前景窗口。
    let foreground_before = foreground_hwnd();
    // 运行结果先保存，确保错误路径仍核验前景并清理 profile/staging。
    let worker_result = worker_process::run_companion(
        // 使用精确 sibling 路径。
        &worker,
        // worker 不接受命令行参数。
        &[],
        // 通过 stdin 传递严格请求。
        &request,
        // 外层 deadline 只增加结构化回收宽限。
        Duration::from_millis(u64::from(input.timeout_ms) + WORKER_REAP_GRACE_MS),
        // 限制 worker stdout。
        MAXIMUM_WORKER_OUTPUT_BYTES,
        // 轮询统一取消状态。
        cancellation::is_cancelled,
    );
    // 记录 worker 整树回收后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 任何前景变化都阻止最终提交。
    ensure_foreground_unchanged(foreground_before, foreground_after)
        // 对齐浏览器兼容错误码。
        .map_err(|_| foreground_changed())?;
    // 将外层 watchdog 超时映射为浏览器 deadline。
    let worker_output = worker_result.map_err(map_worker_process_error)?;
    // 严格解析 worker envelope。
    let capture = parse_worker_envelope(worker_output.exit_code, &worker_output.envelope)?;
    // 读取父 Module 实际持有的 staging 长度。
    let staged_bytes = std::fs::metadata(staged.path())
        // 候选消失时失败闭合。
        .map_err(|_| screenshot_missing())?
        // 只读取字节长度。
        .len();
    // worker 与父 Module 字节事实必须一致且有界。
    if staged_bytes != capture.bytes || !(24..=MAXIMUM_PNG_BYTES).contains(&staged_bytes) {
        // 协议漂移时保留旧目标并清理 staging。
        return Err(worker_protocol_error());
    }
    // 全部门禁通过后原子提交公开输出。
    let commit = staged
        // 独立覆盖许可只进入最终提交点。
        .commit(input.overwrite)
        // 映射提交竞态或持久化失败。
        .map_err(atomic_file_error)?;
    // 投影公开 browser.screenshot@1 schema。
    Ok(render(&input, &capture, commit.replaced_existing))
}

// 在确认后解析封闭 target 与 args。
fn parse_input(
    target: &Map<String, Value>,
    args: &Map<String, Value>,
) -> AppResult<ScreenshotInput> {
    // target 只允许唯一 URL 字段。
    if target.len() != 1 || !target.contains_key("url") {
        // 拒绝 flag、profile 或 runtime 字段。
        return Err(invalid_argument(
            // 使用稳定封闭字段诊断。
            "browser.screenshot target accepts url only.",
        ));
    }
    // args 只允许固定五个动态槽。
    if args
        // 遍历全部调用方字段。
        .keys()
        // 检测协议外字段。
        .any(|key| {
            !matches!(
                key.as_str(),
                "path" | "width" | "height" | "timeoutMs" | "overwrite"
            )
        })
    {
        // 任意 Chromium 参数入口失败闭合。
        return Err(invalid_argument(
            // 说明封闭参数集合。
            "browser.screenshot args accept path, width, height, timeoutMs and overwrite only.",
        ));
    }
    // 读取唯一 URL。
    let url = target
        // 访问 URL 字段。
        .get("url")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 限制非空 UTF-8 字节长度。
        .filter(|value| (1..=8_192).contains(&value.len()))
        // 缺失或超限按参数错误处理。
        .ok_or_else(|| invalid_argument("browser.screenshot requires a bounded URL."))?;
    // 只允许认证的三个 URL scheme。
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("file://")) {
        // 防止 URL 被解释为 Chromium flag。
        return Err(invalid_argument(
            // 不回显调用方 URL。
            "browser.screenshot accepts http, https or file URLs only.",
        ));
    }
    // 读取必需输出路径。
    let path = args
        // 访问 path 字段。
        .get("path")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 应用 Windows UTF-8 路径边界。
        .filter(|value| (5..=32_767).contains(&value.len()))
        // 缺失或超限返回路径错误。
        .ok_or_else(invalid_output_path)?;
    // 输出必须使用小写 PNG 扩展。
    let output = PathBuf::from(path);
    // 检查真实文件名与扩展名。
    if output.file_name().is_none()
        // 扩展名必须精确匹配。
        || output.extension().and_then(|value| value.to_str()) != Some("png")
    {
        // 拒绝非 PNG 目标。
        return Err(invalid_output_path());
    }
    // 输出父目录必须已经存在。
    if !output.parent().is_some_and(|parent| parent.is_dir()) {
        // 禁止隐式创建调用方目录。
        return Err(invalid_output_path());
    }
    // 读取或默认视口宽度。
    let width = bounded_u32(args.get("width"), DEFAULT_WIDTH, 1, 10_000, "width")?;
    // 读取或默认视口高度。
    let height = bounded_u32(args.get("height"), DEFAULT_HEIGHT, 1, 10_000, "height")?;
    // 读取或默认浏览器 deadline。
    let timeout_ms = bounded_u32(
        // 访问可选 timeout。
        args.get("timeoutMs"),
        // 使用固定默认值。
        DEFAULT_TIMEOUT_MS,
        // 使用契约下限。
        1_000,
        // 使用契约上限。
        300_000,
        // 提供安全字段名。
        "timeoutMs",
    )?;
    // 读取独立覆盖许可。
    let overwrite = match args.get("overwrite") {
        // 缺失时禁止覆盖。
        None => false,
        // 只接受布尔值。
        Some(value) => value
            // 解析布尔值。
            .as_bool()
            // 错误类型失败闭合。
            .ok_or_else(|| invalid_argument("browser.screenshot overwrite must be boolean."))?,
    };
    // 返回冻结的领域输入。
    Ok(ScreenshotInput {
        // 保存 URL。
        url: url.to_owned(),
        // 保存最终路径。
        path: path.to_owned(),
        // 保存宽度。
        width,
        // 保存高度。
        height,
        // 保存 deadline。
        timeout_ms,
        // 保存覆盖许可。
        overwrite,
    })
}

// 读取有界 u32 动态槽。
fn bounded_u32(
    // 接收可选 JSON 值。
    value: Option<&Value>,
    // 接收缺省值。
    default: u32,
    // 接收包含下限。
    minimum: u32,
    // 接收包含上限。
    maximum: u32,
    // 接收安全字段名。
    name: &str,
) -> AppResult<u32> {
    // 缺失字段直接使用固定默认值。
    let Some(value) = value else {
        // 返回默认值。
        return Ok(default);
    };
    // 只接受可无损转换的无符号整数。
    let parsed = value
        // 读取 JSON 无符号整数。
        .as_u64()
        // 转换为 u32。
        .and_then(|value| u32::try_from(value).ok())
        // 类型或溢出失败闭合。
        .ok_or_else(|| {
            invalid_argument(format!("browser.screenshot {name} must be an integer."))
        })?;
    // 应用包含边界。
    if !(minimum..=maximum).contains(&parsed) {
        // 越界值不得被静默替换。
        return Err(invalid_argument(format!(
            // 输出不含调用方值的范围诊断。
            "browser.screenshot {name} must be {minimum}..={maximum}."
        )));
    }
    // 返回有界整数。
    Ok(parsed)
}

// 严格解析 worker 成功或失败 envelope。
fn parse_worker_envelope(exit_code: u32, envelope: &Value) -> AppResult<WorkerScreenshot> {
    // 顶层必须是对象。
    let object = envelope.as_object().ok_or_else(worker_protocol_error)?;
    // 固定协议版本必须匹配。
    if object.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT_VERSION) {
        // 未知版本失败闭合。
        return Err(worker_protocol_error());
    }
    // 读取布尔成功标记。
    let ok = object
        // 访问 ok 字段。
        .get("ok")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失时按协议错误处理。
        .ok_or_else(worker_protocol_error)?;
    // 失败 envelope 只允许固定三字段和退出码二。
    if !ok {
        // 验证失败顶层形状。
        if exit_code != 2 || object.len() != 3 || !object.contains_key("error") {
            // 不一致事实失败闭合。
            return Err(worker_protocol_error());
        }
        // 读取固定 error 对象。
        let error = object
            // 访问 error 字段。
            .get("error")
            // 只接受对象。
            .and_then(Value::as_object)
            // 缺失时失败闭合。
            .ok_or_else(worker_protocol_error)?;
        // error 只允许 code 与 message。
        if error.len() != 2 {
            // 额外 provider 事实不得公开。
            return Err(worker_protocol_error());
        }
        // 读取稳定错误码。
        let code = error
            // 访问 code。
            .get("code")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失时失败闭合。
            .ok_or_else(worker_protocol_error)?;
        // 读取安全错误消息。
        let message = error
            // 访问 message。
            .get("message")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失时失败闭合。
            .ok_or_else(worker_protocol_error)?;
        // 只解析固定白名单错误类别。
        let code = worker_error_code(code)?;
        // 通过封闭类型转发安全 worker 消息。
        return Err(code.error(message));
    }
    // 成功 envelope 必须具有固定三字段和零退出码。
    if exit_code != 0 || object.len() != 3 || !object.contains_key("data") {
        // 进程事实不一致失败闭合。
        return Err(worker_protocol_error());
    }
    // 读取固定 data 对象。
    let data = object
        // 访问 data 字段。
        .get("data")
        // 只接受对象。
        .and_then(Value::as_object)
        // 缺失时失败闭合。
        .ok_or_else(worker_protocol_error)?;
    // 成功 data 必须恰好七个字段。
    if data.len() != 7 {
        // 禁止路径、PID 或 provider 输出混入。
        return Err(worker_protocol_error());
    }
    // 验证固定安全布尔事实。
    for (field, expected) in [
        // 候选已完整写入。
        ("candidateWritten", true),
        // profile 为本次调用独占。
        ("isolatedProfile", true),
        // worker 内前景保持不变。
        ("foregroundUnchanged", true),
    ] {
        // 每个字段必须精确匹配。
        if data.get(field).and_then(Value::as_bool) != Some(expected) {
            // 安全证明缺失时拒绝。
            return Err(worker_protocol_error());
        }
    }
    // 读取并限制 PNG 字节数。
    let bytes = data
        // 访问 bytes。
        .get("pngBytes")
        // 只接受无符号整数。
        .and_then(Value::as_u64)
        // 应用公开上限。
        .filter(|value| (24..=MAXIMUM_PNG_BYTES).contains(value))
        // 缺失或越界失败闭合。
        .ok_or_else(worker_protocol_error)?;
    // 读取宽度。
    let width = protocol_u32(data.get("width"), 10_000)?;
    // 读取高度。
    let height = protocol_u32(data.get("height"), 10_000)?;
    // 读取固定十六位摘要。
    let png_digest = data
        // 访问摘要字段。
        .get("pngDigest")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 验证固定小写十六进制语法。
        .filter(|value| {
            // 长度必须恰好十六。
            value.len() == 16
                // 每字节必须是小写十六进制。
                && value
                    // 遍历字节。
                    .bytes()
                    // 验证字符集合。
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        // 漂移时失败闭合。
        .ok_or_else(worker_protocol_error)?
        // 冻结为领域事实。
        .to_owned();
    // 返回严格候选事实。
    Ok(WorkerScreenshot {
        // 保存字节数。
        bytes,
        // 保存宽度。
        width,
        // 保存高度。
        height,
        // 保存摘要。
        png_digest,
    })
}

// 读取正数协议 u32。
fn protocol_u32(value: Option<&Value>, maximum: u32) -> AppResult<u32> {
    // 读取并转换无符号整数。
    let value = value
        // 只接受 JSON 无符号整数。
        .and_then(Value::as_u64)
        // 无损转换为 u32。
        .and_then(|value| u32::try_from(value).ok())
        // 缺失或溢出按协议错误处理。
        .ok_or_else(worker_protocol_error)?;
    // 应用正数与上限边界。
    if !(1..=maximum).contains(&value) {
        // 越界时失败闭合。
        return Err(worker_protocol_error());
    }
    // 返回有界值。
    Ok(value)
}

// 把 worker 错误码限制在公开白名单。
fn worker_error_code(code: &str) -> AppResult<BrowserScreenshotErrorCode> {
    // 未知错误拒绝转发并收敛为协议违规。
    BrowserScreenshotErrorCode::from_worker(code).ok_or_else(worker_protocol_error)
}

// 把 worker process 错误映射到浏览器领域。
fn map_worker_process_error(error: AppControlError) -> AppControlError {
    // 外层 watchdog 超时保持浏览器公开错误码。
    if error.code == BrowserScreenshotErrorCode::WorkerProcessTimeout.as_str() {
        // 返回已经整树回收的浏览器超时。
        return BrowserScreenshotErrorCode::BrowserTimeout.error(
            // 说明隔离进程树已回收。
            "The isolated browser exceeded its deadline and was reaped.",
        );
    }
    // 其他 worker 生命周期错误保持原分类。
    error
}

// 把结果投影为公开 browser screenshot schema。
fn render(input: &ScreenshotInput, capture: &WorkerScreenshot, replaced_existing: bool) -> Value {
    // 覆盖发生必须已有独立许可。
    debug_assert!(!replaced_existing || input.overwrite);
    // worker 返回的尺寸必须匹配冻结输入。
    debug_assert_eq!(capture.width, input.width);
    // worker 返回的尺寸必须匹配冻结输入。
    debug_assert_eq!(capture.height, input.height);
    // 摘要只作为内部候选一致性证据，不进入旧公开 schema。
    debug_assert_eq!(capture.png_digest.len(), 16);
    // 返回恰好匹配兼容 schema 的字段。
    json!({
        // 标记操作成功。
        "ok": true,
        // 回显固定 app。
        "app": "browser",
        // 回显固定 operation。
        "operation": "screenshot",
        // 输出稳定 capability ID。
        "capability": capabilities::BROWSER_SCREENSHOT,
        // 回显最终路径。
        "path": input.path,
        // 输出实际 PNG 字节数。
        "bytes": capture.bytes,
        // 输出实际宽度。
        "width": capture.width,
        // 输出实际高度。
        "height": capture.height,
        // 声明 profile 独占。
        "isolatedProfile": true,
        // 声明同目录原子提交。
        "atomicOutput": true,
        // 回显独立覆盖许可。
        "overwriteConfirmed": input.overwrite,
        // 返回无原生标识的前景证明。
        "foreground": {
            // 主进程与 worker 均证明未变化。
            "unchanged": true,
            // 明确不泄漏 HWND。
            "nativeIdentifiersExposed": false,
        },
        // 明确不公开 Chromium 路径。
        "runtimePathExposed": false,
        // 明确不公开 PID、HWND 或 profile identity。
        "nativeIdentifiersExposed": false,
        // 固定兼容结果形状。
        "compatibilityShape": "isolated-headless-browser-v1",
    })
}

// 构造稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回不含调用方内容的参数错误。
    BrowserScreenshotErrorCode::InvalidArgument.error(message)
}

// 构造稳定输出路径错误。
fn invalid_output_path() -> AppControlError {
    // 不公开具体路径或底层 I/O 文本。
    BrowserScreenshotErrorCode::InvalidOutputPath.error(
        // 说明固定 PNG 与父目录要求。
        "Browser screenshot requires a safe .png path in an existing directory.",
    )
}

// 构造内部 profile 边界错误。
fn profile_path_error() -> AppControlError {
    // 不公开平台路径编码。
    BrowserScreenshotErrorCode::TempProfileFailed.error(
        // 说明无法跨协议边界。
        "The isolated browser profile cannot cross the worker boundary.",
    )
}

// 构造截图候选缺失错误。
fn screenshot_missing() -> AppControlError {
    // 返回稳定候选错误。
    BrowserScreenshotErrorCode::ScreenshotMissing.error(
        // 不公开 staging 路径。
        "The isolated browser did not produce a valid PNG candidate.",
    )
}

// 构造前景变化错误。
fn foreground_changed() -> AppControlError {
    // 返回稳定前景干扰码。
    BrowserScreenshotErrorCode::ForegroundChanged.error(
        // 说明结果已拒绝。
        "The isolated browser unexpectedly changed the foreground window; output was rejected.",
    )
}

// 构造统一 worker 协议错误。
fn worker_protocol_error() -> AppControlError {
    // 返回不含 worker 输出的安全诊断。
    BrowserScreenshotErrorCode::WorkerProtocolViolation.error(
        // 不回显 provider 数据。
        "The browser worker returned an invalid screenshot protocol envelope.",
    )
}

// 映射统一输出门禁错误。
fn output_guard_error(error: OutputGuardError) -> AppControlError {
    // 保持覆盖许可与路径分类稳定。
    match error {
        // 既有普通文件需要独立覆盖许可。
        OutputGuardError::ConfirmationRequired => {
            // 通过封闭类型构造覆盖许可错误。
            BrowserScreenshotErrorCode::OverwriteConfirmationRequired.error(
                // 不公开路径。
                "Existing browser screenshot output requires overwrite confirmation.",
            )
        }
        // 其他目标类型或检查失败统一按不安全路径拒绝。
        OutputGuardError::InspectionFailed | OutputGuardError::InvalidTargetType => {
            // 返回稳定路径错误。
            invalid_output_path()
        }
    }
}

// 映射原子提交错误。
fn atomic_file_error(error: AtomicFileError) -> AppControlError {
    // 保持竞态与写入失败分类稳定。
    match error {
        // 目标在提交竞态中出现时重新要求覆盖许可。
        AtomicFileError::TargetExists => {
            // 通过封闭类型保持覆盖竞态分类。
            BrowserScreenshotErrorCode::OverwriteConfirmationRequired.error(
            // 不公开路径。
            "Existing browser screenshot output requires overwrite confirmation.",
            )
        }
        // 无效目标保持路径错误。
        AtomicFileError::InvalidDestination => invalid_output_path(),
        // 其余生命周期失败统一为候选写入失败。
        AtomicFileError::StagingCreationFailed
        // 合并无效 staging。
        | AtomicFileError::InvalidStaging
        // 合并持久化失败。
        | AtomicFileError::SyncFailed
        // 合并原子提交失败。
        | AtomicFileError::CommitFailed => screenshot_missing(),
    }
}

// 声明纯输入、协议与投影测试。
#[cfg(test)]
// 保持测试靠近 Module 私有契约。
mod tests {
    // 导入父模块纯函数与事实。
    use super::{WorkerScreenshot, parse_input, parse_worker_envelope, render};
    // 导入 JSON Map 与构造器。
    use serde_json::{Map, json};

    // 验证固定参数默认值与未知字段拒绝。
    #[test]
    // 不触碰 runtime 或文件系统。
    fn input_defaults_are_bounded_and_closed() {
        // 构造唯一合法目标。
        let target = Map::from_iter([("url".to_owned(), json!("https://example.invalid/"))]);
        // 构造父目录真实存在的仓库内 PNG 路径。
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("browser.png");
        // 使用只解析但不会写入的合法绝对路径。
        let args = Map::from_iter([(
            // 设置固定 path 字段。
            "path".to_owned(),
            // 序列化平台路径文本。
            json!(output.display().to_string()),
        )]);
        // 解析固定输入。
        let Ok(input) = parse_input(&target, &args) else {
            // 合法输入意外失败时终止。
            panic!("browser screenshot input should pass");
        };
        // 核对默认宽度。
        assert_eq!(input.width, 1_280);
        // 核对默认高度。
        assert_eq!(input.height, 720);
        // 核对默认 deadline。
        assert_eq!(input.timeout_ms, 30_000);
        // 构造任意 Chromium 参数注入。
        let mut injected = args.clone();
        // 增加未认证 argv。
        injected.insert("argv".to_owned(), json!(["--disable-web-security"]));
        // 未知字段必须失败。
        assert!(parse_input(&target, &injected).is_err());
    }

    // 验证严格 worker 成功协议。
    #[test]
    // 只验证语言中立 envelope。
    fn worker_envelope_requires_exact_candidate_proof() {
        // 构造固定合法 envelope。
        let envelope = json!({
            // 标记成功。
            "ok": true,
            // 固定协议版本。
            "contractVersion": "act/browser-screenshot-worker/v1",
            // 提供精确七字段 data。
            "data": {
                // 提供 PNG 字节数。
                "pngBytes": 1024,
                // 提供宽度。
                "width": 640,
                // 提供高度。
                "height": 360,
                // 提供摘要。
                "pngDigest": "0123456789abcdef",
                // 声明候选已写入。
                "candidateWritten": true,
                // 声明 profile 隔离。
                "isolatedProfile": true,
                // 声明前景不变。
                "foregroundUnchanged": true,
            },
        });
        // 合法 envelope 必须通过。
        let Ok(capture) = parse_worker_envelope(0, &envelope) else {
            // 合法协议意外失败时终止。
            panic!("browser worker envelope should pass");
        };
        // 核对维度。
        assert_eq!((capture.width, capture.height), (640, 360));
    }

    // 验证公开结果不包含 runtime、profile 或原生标识。
    #[test]
    // 使用纯投影输入。
    fn render_exposes_only_compatibility_schema() {
        // 构造固定输入。
        let input = super::ScreenshotInput {
            // 设置 URL。
            url: "https://example.invalid/".to_owned(),
            // 设置输出路径。
            path: "browser.png".to_owned(),
            // 设置宽度。
            width: 640,
            // 设置高度。
            height: 360,
            // 设置 deadline。
            timeout_ms: 30_000,
            // 禁止覆盖。
            overwrite: false,
        };
        // 构造固定候选事实。
        let capture = WorkerScreenshot {
            // 设置字节数。
            bytes: 1024,
            // 设置宽度。
            width: 640,
            // 设置高度。
            height: 360,
            // 设置摘要。
            png_digest: "fedcba9876543210".to_owned(),
        };
        // 投影公开结果。
        let value = render(&input, &capture, false);
        // 核对 capability。
        assert_eq!(value["capability"], "browser.screenshot@1");
        // 序列化后检查敏感字段缺失。
        let text = value.to_string();
        // 禁止 profile 路径。
        assert!(!text.contains("profilePath"));
        // 禁止 runtime 路径。
        assert!(!text.contains("runtimePath\""));
        // 禁止 PID。
        assert!(!text.contains("pid"));
    }
}
