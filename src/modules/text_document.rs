//! 受控 UTF-8 文档创建、验证与固定 Notepad 启动领域行为。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "text_document_error.rs"]
mod error_code;

// 导入文件、路径与唯一名称工具。
use std::{
    // 导入原子新建与失败清理接口。
    fs::{self, OpenOptions},
    // 导入完整写入接口。
    io::Write,
    // 导入路径类型。
    path::{Path, PathBuf},
    // 导入唯一时间戳来源。
    time::{SystemTime, UNIX_EPOCH},
};

// 导入 JSON 构造工具。
use serde_json::{Value, json};

// 导入当前 Module 私有封闭错误码。
use error_code::TextDocumentErrorCode;

// 导入平台 Adapter、稳定错误与请求。
use crate::{
    // 使用只读进程快照与固定 Notepad 启动 Adapter。
    adapters::{text_document_windows, windows::enumerate_process_inventory},
    // 使用统一领域结果与请求。
    domain::{AppResult, CommandRequest},
};

// 文本 UTF-8 字节数上限与公开兼容合同一致。
const MAXIMUM_UTF8_BYTES: usize = 1_048_576;
// 原子文件名碰撞最多尝试固定次数。
const MAXIMUM_NAME_ATTEMPTS: u32 = 32;
// 只允许删除本 Module 创建的文件名前缀。
const ARTIFACT_PREFIX: &str = "ai-computer-toolkit-notepad-";

// 保存尚未提交给调用方的工具自有 artifact。
struct PendingArtifact {
    // 保存精确新建路径。
    path: PathBuf,
    // 标记成功路径是否已移交所有权。
    committed: bool,
}

// 提供 artifact 所有权操作。
impl PendingArtifact {
    // 接管刚由 CREATE_NEW 等价语义创建的路径。
    fn new(path: PathBuf) -> Self {
        // 初始必须在失败时清理。
        Self {
            // 保存受控路径。
            path,
            // 尚未向调用方提交。
            committed: false,
        }
    }

    // 返回只读路径引用。
    fn path(&self) -> &Path {
        // 不转移路径所有权。
        &self.path
    }

    // 成功后保留 artifact 并返回路径。
    fn commit(mut self) -> PathBuf {
        // 禁止 Drop 删除成功 artifact。
        self.committed = true;
        // 返回独立路径副本。
        self.path.clone()
    }
}

// 失败退出时只删除本 Module 拥有的新 artifact。
impl Drop for PendingArtifact {
    // 执行受限清理。
    fn drop(&mut self) {
        // 成功提交后不删除用户预期的新文档。
        if self.committed {
            // 直接结束清理。
            return;
        }
        // 只允许匹配固定前缀和 txt 扩展名。
        let owned_name = self
            // 读取文件名。
            .path
            // 不接受无文件名路径。
            .file_name()
            // 转换为无损可比较文本。
            .and_then(|name| name.to_str())
            // 验证固定前缀与扩展名。
            .is_some_and(|name| name.starts_with(ARTIFACT_PREFIX) && name.ends_with(".txt"));
        // 只有所有权证据完整时才删除。
        if owned_name {
            // 忽略清理错误，避免覆盖原始领域错误。
            let _ = fs::remove_file(&self.path);
        }
    }
}

// 返回固定系统 Notepad 路径，不接受任意 executable。
pub(crate) fn runtime_path() -> AppResult<PathBuf> {
    // 只读取 Windows 系统目录环境变量。
    let windir = std::env::var_os("WINDIR")
        // 缺失时返回稳定 runtime 错误。
        .ok_or_else(|| {
            // 由 Module 私有类型选择固定公开错误码。
            TextDocumentErrorCode::NotepadUnavailable.error("WINDIR is unavailable.")
        })?;
    // 拼接固定 System32 Notepad。
    let path = PathBuf::from(windir).join("System32").join("notepad.exe");
    // 只接受现有普通文件。
    if path.is_file() {
        // 返回私有 runtime 路径。
        Ok(path)
    } else {
        // 不回显本机路径。
        Err(TextDocumentErrorCode::NotepadUnavailable.error(
            // 保持公共消息无路径。
            "The fixed system Notepad runtime is unavailable.",
        ))
    }
}

// confirmation-first 后验证 provider 输入。
fn required_text(request: &CommandRequest) -> AppResult<&str> {
    // 缺确认必须在 runtime 探测、快照和文件写入前失败。
    if !request.confirmed {
        // 返回统一确认错误。
        return Err(TextDocumentErrorCode::ConfirmationRequired.error(
            // 与 policy 和公开兼容合同保持一致。
            // 明确本操作会创建新 artifact。
            "Creating a text document requires explicit confirmation.",
        ));
    }
    // 读取 legacy adapter 的已映射文本字段。
    let text = request
        // 访问参数映射。
        .args
        // 读取固定 text 字段。
        .get("text")
        // 只接受 UTF-8 JSON string。
        .and_then(Value::as_str)
        // 缺失或类型错误返回参数错误。
        .ok_or_else(|| {
            // 由 Module 私有类型选择参数拒绝码。
            TextDocumentErrorCode::InvalidArgument.error("args.text is required.")
        })?;
    // 按 UTF-8 bytes 而非字符数执行合同上限。
    if text.len() > MAXIMUM_UTF8_BYTES {
        // 返回稳定有界错误。
        return Err(TextDocumentErrorCode::InvalidArgument.error(
            // 不回显文本内容。
            "args.text exceeds the 1 MiB UTF-8 limit.",
        ));
    }
    // 返回已验证文本。
    Ok(text)
}

// 在写入前拒绝已有 Notepad 会话或不完整快照。
fn ensure_no_existing_notepad() -> AppResult<()> {
    // 完整枚举当前进程快照。
    let inventory = enumerate_process_inventory(usize::MAX)?;
    // 不完整快照不能证明不会附着既有实例。
    if !inventory.complete {
        // 关闭执行路径。
        return Err(
            TextDocumentErrorCode::BackgroundOperationUnavailable.with_details(
                // 明确安全判断无法完成。
                "The process snapshot could not certify an isolated Notepad launch.",
                // 输出稳定、无 PID 的重试语义。
                json!({
                    "reason": "process-snapshot-incomplete",
                    "artifactCreated": false,
                    "safeToRetryAutomatically": false,
                }),
            ),
        );
    }
    // Windows 文件名比较不区分大小写。
    let existing = inventory
        // 遍历只读进程事实。
        .records
        // 检查固定进程名。
        .iter()
        // 不公开匹配进程身份。
        .any(|process| process.process_name.eq_ignore_ascii_case("notepad.exe"));
    // 已有实例可能吸收新文件，必须在 artifact 创建前拒绝。
    if existing {
        // 返回合同规定的结构化拒绝。
        return Err(
            TextDocumentErrorCode::BackgroundOperationUnavailable.with_details(
                // 保持原因稳定且不提取用户文档。
                "An existing Notepad session prevents a certified isolated launch.",
                // 明确无写入及不可自动重试。
                json!({
                    "reason": "existing-application-session-attachment-not-certified",
                    "artifactCreated": false,
                    "safeToRetryAutomatically": false,
                }),
            ),
        );
    }
    // 安全前置条件成立。
    Ok(())
}

// 原子新建、持久 flush 并逐字节回读 UTF-8 artifact。
fn create_verified_artifact(text: &str) -> AppResult<(PendingArtifact, String)> {
    // 获取系统临时目录。
    let directory = std::env::temp_dir();
    // 使用当前时间与进程 ID 生成私有候选。
    let stamp = SystemTime::now()
        // 转换为 UNIX duration。
        .duration_since(UNIX_EPOCH)
        // 时钟异常时结构化失败。
        .map_err(|_| {
            // 不暴露系统时间细节。
            TextDocumentErrorCode::DocumentCreateFailed
                // 保持既有公开时钟错误消息。
                .error("The artifact clock is unavailable.")
        })?
        // 使用纳秒降低同进程碰撞概率。
        .as_nanos();
    // 在固定次数内处理真实 CREATE_NEW 碰撞。
    for attempt in 0..MAXIMUM_NAME_ATTEMPTS {
        // 组合工具自有文件名。
        let path = directory.join(format!(
            // 固定前缀、进程、时间与尝试序号。
            "{ARTIFACT_PREFIX}{}-{stamp}-{attempt}.txt",
            // 当前工具进程 ID 只进入私有路径。
            std::process::id(),
        ));
        // 使用 create_new 提供 CREATE_NEW 等价语义。
        let mut file = match OpenOptions::new()
            // 只申请写入。
            .write(true)
            // 禁止覆盖既有路径。
            .create_new(true)
            // 打开候选路径。
            .open(&path)
        {
            // 接收新文件。
            Ok(file) => file,
            // 碰撞时尝试下一个受控名称。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // 其他写入错误结构化失败。
            Err(_) => {
                // 不回显系统路径或原生错误。
                return Err(TextDocumentErrorCode::DocumentCreateFailed.error(
                    // 固定错误消息。
                    "The new text artifact could not be created atomically.",
                ));
            }
        };
        // 从此处开始所有失败都由 guard 清理。
        let artifact = PendingArtifact::new(path);
        // 写入全部 UTF-8 bytes。
        file.write_all(text.as_bytes()).map_err(|_| {
            // 返回稳定写入错误。
            TextDocumentErrorCode::DocumentCreateFailed
                // 保持既有公开写入错误消息。
                .error("The new text artifact could not be written completely.")
        })?;
        // flush 用户态缓冲。
        file.flush().map_err(|_| {
            // 返回稳定 flush 错误。
            TextDocumentErrorCode::DocumentCreateFailed
                // 保持既有公开 flush 错误消息。
                .error("The new text artifact could not be flushed.")
        })?;
        // sync_all 对齐公开兼容合同的 durable gate。
        file.sync_all().map_err(|_| {
            // 返回稳定持久化错误。
            TextDocumentErrorCode::DocumentCreateFailed
                // 保持既有公开持久化错误消息。
                .error("The new text artifact could not be synchronized.")
        })?;
        // 关闭写 handle 后再独立回读。
        drop(file);
        // 读取全部 bytes；前置上限保证最多 1 MiB。
        let bytes = fs::read(artifact.path()).map_err(|_| {
            // 返回稳定回读错误。
            TextDocumentErrorCode::TextVerificationFailed
                // 保持既有公开回读错误消息。
                .error("The new text artifact could not be read back.")
        })?;
        // 要求逐字节等价。
        if bytes != text.as_bytes() {
            // guard 在返回时删除不一致 artifact。
            return Err(TextDocumentErrorCode::TextVerificationFailed.error(
                // 固定错误消息。
                "The new text artifact did not match the requested UTF-8.",
            ));
        }
        // JSON 输入已保证 UTF-8，逐字节相同可安全复制原字符串。
        return Ok((artifact, text.to_owned()));
    }
    // 所有候选均碰撞时关闭失败。
    Err(TextDocumentErrorCode::DocumentCreateFailed.error(
        // 不回显候选路径。
        "No collision-free temporary artifact name was available.",
    ))
}

// 执行完整受控文档新建流程并返回 legacy 内部结果。
pub(crate) fn create_and_open(request: &CommandRequest) -> AppResult<Value> {
    // confirmation-first 并验证输入上限。
    let text = required_text(request)?;
    // 只解析固定系统 runtime。
    let executable = runtime_path()?;
    // 在任何 artifact 写入前拒绝既有 Notepad。
    ensure_no_existing_notepad()?;
    // 创建并回读本次工具自有 artifact。
    let (artifact, verified_text) = create_verified_artifact(text)?;
    // 在 Job 回滚边界内请求 no-activate 启动。
    let launch = text_document_windows::launch_no_activate(&executable, artifact.path())?;
    // 只有完整成功后才保留 artifact。
    let path = artifact.commit();
    // 返回 legacy adapter 兼容形状；facade 会进一步净化。
    Ok(json!({
        // 标记内部成功。
        "ok": true,
        // 保留 legacy app 字段。
        "app": "notepad",
        // 保留 legacy operation 字段。
        "operation": "open-and-write-text",
        // 返回本次新 artifact 路径。
        "path": path,
        // legacy 形状保留启动器 PID，facade 不透出。
        "launcherProcessId": launch.process_id,
        // 返回逐字节回读文本。
        "text": verified_text,
        // 说明没有前台输入机制。
        "inputMethod": "atomic UTF-8 artifact + fixed no-activate Notepad launch",
        // 仅输出前台不变布尔值。
        "foreground": { "unchanged": launch.foreground_unchanged },
    }))
}

// 声明纯验证与临时 artifact 单元测试。
#[cfg(test)]
mod tests {
    // 导入当前 Module 私有函数。
    use super::*;
    // 导入请求构造所需 verb。
    use crate::domain::Verb;

    // 构造不触发 runtime 的确认门禁请求。
    fn request(text: &str, confirmed: bool) -> CommandRequest {
        // 建立 legacy run 请求。
        let mut request = CommandRequest::read(Verb::Run, "notepad");
        // 设置确认状态。
        request.confirmed = confirmed;
        // 写入固定文本参数。
        request.args.insert("text".to_owned(), json!(text));
        // 返回请求。
        request
    }

    // 确认必须先于 runtime 与文件系统。
    #[test]
    fn confirmation_is_required_before_runtime_probe() {
        // 缺确认时只运行纯输入门禁。
        let error = required_text(&request("hello", false)).err();
        // 验证稳定确认错误。
        assert_eq!(error.map(|value| value.code), Some("CONFIRMATION_REQUIRED"));
    }

    // UTF-8 上限按 bytes 而非字符计数。
    #[test]
    fn utf8_limit_counts_bytes() {
        // 多字节字符构成超过上限的输入。
        let text = "你".repeat((MAXIMUM_UTF8_BYTES / 3) + 1);
        // 只运行纯输入门禁。
        let error = required_text(&request(&text, true)).err();
        // 验证参数错误。
        assert_eq!(error.map(|value| value.code), Some("INVALID_ARGUMENT"));
    }

    // 原子 artifact 必须逐字节回读并由测试清理。
    #[test]
    fn artifact_is_durable_and_byte_exact() -> AppResult<()> {
        // 使用 ASCII 与多字节 UTF-8 混合夹具。
        let text = "Rust 文本文档 fixture\r\n第二行";
        // 创建并验证 artifact。
        let (artifact, verified) = create_verified_artifact(text)?;
        // 核对回读文本。
        assert_eq!(verified, text);
        // 核对磁盘 bytes。
        assert_eq!(
            fs::read(artifact.path()).ok().as_deref(),
            Some(text.as_bytes())
        );
        // 保存路径用于 drop 后验证。
        let path = artifact.path().to_path_buf();
        // 未提交 guard 必须清理。
        drop(artifact);
        // 验证文件已删除。
        assert!(!path.exists());
        // 返回成功。
        Ok(())
    }
}
