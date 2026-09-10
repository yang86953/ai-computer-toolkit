//! 定义 sequence step worker 的封闭双阶段 JSON Lines 协议。

// 导入严格协议序列化与反序列化派生。
use serde::{Deserialize, Serialize};

// 导入统一请求、JSON 对象与隔离要求。
use crate::domain::{CommandRequest, IsolationRequirement, JsonMap, Verb};

// 固定 sequence step worker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/sequence-step-worker/v1";
// 固定一次性请求关联值的小写十六进制长度。
const NONCE_LENGTH: usize = 32;
// 固定单个 worker 请求的 UTF-8 字节上限。
pub(crate) const MAXIMUM_REQUEST_BYTES: usize = 128 * 1024;
// 固定 target 或 args 单个 JSON 对象的紧凑字节上限。
const MAXIMUM_COMMAND_MAP_BYTES: usize = 64 * 1024;
// 固定 app 与 operation 文本的 UTF-8 字节上限。
const MAXIMUM_ROUTE_BYTES: usize = 128;
// 固定 accepted 与 final 两帧合计的 UTF-8 字节上限。
pub(crate) const MAXIMUM_OUTPUT_BYTES: usize = 16 * 1024 * 1024 + 128 * 1024;
// 固定 worker 接受的最小剩余 deadline。
const MINIMUM_TIMEOUT_MS: u32 = 1;
// 固定 worker 接受的最大剩余 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

// 表示协议边界允许产生的封闭失败类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceStepProtocolErrorCode {
    // 表示请求或 control 字段违反封闭协议。
    InvalidArgument,
    // 表示请求超过固定输入字节边界。
    RequestTooLarge,
    // 表示 worker 输出超过固定结果字节边界。
    OutputTooLarge,
    // 表示 frame 关联、顺序或状态不合法。
    ProtocolFailed,
}

// 为封闭协议失败类别提供稳定文本。
impl SequenceStepProtocolErrorCode {
    // 返回后续 worker/Workflow 错误投影使用的唯一文本。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举全部封闭失败类别。
        match self {
            // 映射普通协议参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射请求资源上限。
            Self::RequestTooLarge => "WORKER_REQUEST_TOO_LARGE",
            // 映射输出资源上限。
            Self::OutputTooLarge => "WORKER_OUTPUT_TOO_LARGE",
            // 映射关联、顺序或状态漂移。
            Self::ProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }
}

// 保存不回显协议负载的解析失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SequenceStepProtocolFailure {
    // 保存唯一封闭失败类别。
    code: SequenceStepProtocolErrorCode,
}

// 为协议失败提供只读错误类别。
impl SequenceStepProtocolFailure {
    // 构造不携带输入值的协议失败。
    const fn new(code: SequenceStepProtocolErrorCode) -> Self {
        // 只保存封闭错误类别。
        Self { code }
    }

    // 返回稳定封闭失败类别。
    pub(crate) const fn code(self) -> SequenceStepProtocolErrorCode {
        // 复制无状态枚举。
        self.code
    }
}

// 表示 worker 协议内的统一 provider-neutral CommandRequest。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceStepWorkerCommand {
    // 保存统一控制动词。
    verb: Verb,
    // 保存公开 app 或 surface ID。
    app: String,
    // 保存 run 动词的可选 operation。
    operation: Option<String>,
    // 保存 provider-neutral 精确目标对象。
    target: JsonMap,
    // 保存 provider-neutral 参数对象。
    args: JsonMap,
    // 保存调用方结果数量边界。
    max_items: usize,
    // 保存调用方层级深度边界。
    max_depth: usize,
    // 保存逐操作确认事实。
    confirmed: bool,
    // 保存受控前台影响同意。
    foreground_consent: bool,
    // 保存不可由 worker 放宽的隔离要求。
    isolation_requirement: IsolationRequirement,
}

// 实现统一请求与协议命令之间的窄转换。
impl SequenceStepWorkerCommand {
    // 从已经完成 Workflow 绑定的最终请求构造协议命令。
    pub(crate) fn from_request(
        // 取得最终统一请求所有权。
        request: CommandRequest,
    ) -> Result<Self, SequenceStepProtocolFailure> {
        // 构造不增加路由或权限字段的协议命令。
        let command = Self {
            // 保留统一动词。
            verb: request.verb,
            // 保留公开 app ID。
            app: request.app,
            // 保留可选 operation。
            operation: request.operation,
            // 转移 provider-neutral 目标。
            target: request.target,
            // 转移 provider-neutral 参数。
            args: request.args,
            // 保留结果数量边界。
            max_items: request.max_items,
            // 保留读取深度边界。
            max_depth: request.max_depth,
            // 保留逐操作确认。
            confirmed: request.confirmed,
            // 保留前台同意。
            foreground_consent: request.foreground_consent,
            // 保留不可降级隔离要求。
            isolation_requirement: request.isolation_requirement,
        };
        // 在进入 worker 前完成协议资源与路由文本验证。
        command.validate()?;
        // 返回已验证命令。
        Ok(command)
    }

    // 把已验证协议命令恢复为统一 System 请求。
    pub(crate) fn into_request(self) -> CommandRequest {
        // 逐字段恢复，不创建第二种权限或路由语义。
        CommandRequest {
            // 恢复统一动词。
            verb: self.verb,
            // 恢复公开 app ID。
            app: self.app,
            // 恢复可选 operation。
            operation: self.operation,
            // 恢复 provider-neutral 目标。
            target: self.target,
            // 恢复 provider-neutral 参数。
            args: self.args,
            // 恢复结果数量边界。
            max_items: self.max_items,
            // 恢复读取深度边界。
            max_depth: self.max_depth,
            // 恢复逐操作确认。
            confirmed: self.confirmed,
            // 恢复前台同意。
            foreground_consent: self.foreground_consent,
            // 恢复不可降级隔离要求。
            isolation_requirement: self.isolation_requirement,
        }
    }

    // 验证协议命令自身的资源边界。
    fn validate(&self) -> Result<(), SequenceStepProtocolFailure> {
        // app 必须非空且不超过固定 UTF-8 字节边界。
        if self.app.is_empty() || self.app.len() > MAXIMUM_ROUTE_BYTES {
            // 拒绝无路由或无界路由文本。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数失败。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // 可选 operation 也必须非空且有界。
        if self.operation.as_ref().is_some_and(|operation| {
            // 同时检查空文本与 UTF-8 字节长度。
            operation.is_empty() || operation.len() > MAXIMUM_ROUTE_BYTES
        }) {
            // 拒绝宽松或无界 operation。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数失败。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // target 与 args 分别保持固定紧凑 JSON 字节边界。
        if compact_map_bytes(&self.target) > MAXIMUM_COMMAND_MAP_BYTES
            // 同时检查参数对象。
            || compact_map_bytes(&self.args) > MAXIMUM_COMMAND_MAP_BYTES
        {
            // 拒绝进入 worker 的无界拥有型负载。
            return Err(SequenceStepProtocolFailure::new(
                // 使用请求过大类别。
                SequenceStepProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 命令资源边界全部成立。
        Ok(())
    }
}

// 表示 host 发送给 worker 的一次性执行请求。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceStepWorkerRequest {
    // 保存固定协议版本。
    contract_version: String,
    // 保存不含目标事实的一次性请求 nonce。
    request_nonce: String,
    // 保存总预算与逐步预算取较早者后的剩余 deadline。
    timeout_ms: u32,
    // 保存最终 provider-neutral System 请求。
    command: SequenceStepWorkerCommand,
}

// 实现一次性 worker 请求的构造、解析与投影。
impl SequenceStepWorkerRequest {
    // 构造已经通过 Workflow 输入与绑定验证的 worker 请求。
    pub(crate) fn new(
        // 接收 canonical 一次性关联值。
        request_nonce: String,
        // 接收较早有效 deadline 的剩余毫秒数。
        timeout_ms: u32,
        // 接收已经验证的协议命令。
        command: SequenceStepWorkerCommand,
    ) -> Result<Self, SequenceStepProtocolFailure> {
        // 构造固定版本请求。
        let request = Self {
            // 禁止协议版本由调用方选择。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 保存一次性关联值。
            request_nonce,
            // 保存唯一剩余 deadline。
            timeout_ms,
            // 保存最终命令。
            command,
        };
        // 新建请求与解析请求共享全部边界。
        request.validate()?;
        // 返回已验证请求。
        Ok(request)
    }

    // 从单个有界 JSON 行严格解析 worker 请求。
    pub(crate) fn parse(text: &str) -> Result<Self, SequenceStepProtocolFailure> {
        // 在 JSON 解析前拒绝超过完整请求字节边界的输入。
        if text.len() > MAXIMUM_REQUEST_BYTES {
            // 返回请求过大且不解析任何字段。
            return Err(SequenceStepProtocolFailure::new(
                // 使用封闭资源错误。
                SequenceStepProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 兼容标准输入首个 BOM 并只去除外围空白。
        let normalized = text.trim_start_matches('\u{feff}').trim();
        // JSON Lines 请求不得包含第二行或空输入。
        if normalized.is_empty() || normalized.contains(['\r', '\n']) {
            // 拒绝多帧请求与空输入。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数错误。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // 严格解析封闭对象。
        let request = serde_json::from_str::<Self>(normalized).map_err(|_| {
            // 不回显任何潜在目标或参数。
            SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::InvalidArgument)
        })?;
        // 解析后验证版本、nonce、deadline 和命令资源。
        request.validate()?;
        // 返回未触碰 provider 的请求。
        Ok(request)
    }

    // 把请求序列化为单个有界 JSON 行。
    pub(crate) fn to_line(&self) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
        // 先确认内存请求没有绕过边界。
        self.validate()?;
        // 序列化固定封闭请求。
        let mut bytes = serde_json::to_vec(self).map_err(|_| {
            // 结构化派生失败视为协议错误。
            SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::ProtocolFailed)
        })?;
        // 序列化结果必须仍满足完整请求边界。
        if bytes.len() > MAXIMUM_REQUEST_BYTES {
            // 返回请求过大。
            return Err(SequenceStepProtocolFailure::new(
                // 使用封闭资源错误。
                SequenceStepProtocolErrorCode::RequestTooLarge,
            ));
        }
        // JSON Lines framing 固定追加一个换行。
        bytes.push(b'\n');
        // 返回可直接写入 stdin 的唯一请求帧。
        Ok(bytes)
    }

    // 返回已经验证的一次性关联值。
    pub(crate) fn request_nonce(&self) -> &str {
        // 只公开不含目标事实的关联值。
        &self.request_nonce
    }

    // 返回已经验证的剩余 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 返回 1..30000 范围内的毫秒数。
        self.timeout_ms
    }

    // 取得最终命令所有权供 worker 进入统一 System。
    pub(crate) fn into_command(self) -> SequenceStepWorkerCommand {
        // 转移 provider-neutral 命令。
        self.command
    }

    // 验证完整请求不变量。
    fn validate(&self) -> Result<(), SequenceStepProtocolFailure> {
        // 协议版本必须逐字匹配且不协商。
        if self.contract_version != CONTRACT_VERSION
            // nonce 必须是 canonical 128 位小写十六进制。
            || !is_canonical_nonce(&self.request_nonce)
            // deadline 必须保持公开逐步边界。
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&self.timeout_ms)
        {
            // 拒绝版本、关联或 deadline 漂移。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数错误。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // 命令继续保持自己的资源边界。
        self.command.validate()
    }
}

// 表示 host 在请求帧后最多发送一次的封闭控制类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用唯一稳定小写文本。
#[serde(rename_all = "kebab-case")]
enum SequenceStepControlKind {
    // 表示调用方请求取消当前步骤。
    Cancel,
}

// 表示请求帧后的唯一取消控制帧。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceStepWorkerControl {
    // 保存固定协议版本。
    contract_version: String,
    // 保存必须与请求一致的关联值。
    request_nonce: String,
    // 保存唯一取消控制类别。
    kind: SequenceStepControlKind,
}

// 实现取消帧构造、解析与序列化。
impl SequenceStepWorkerControl {
    // 构造绑定到当前请求的唯一取消帧。
    pub(crate) fn cancel(
        // 接收已经验证的请求关联值。
        request_nonce: &str,
    ) -> Result<Self, SequenceStepProtocolFailure> {
        // nonce 必须保持 canonical。
        if !is_canonical_nonce(request_nonce) {
            // 拒绝无法关联的控制帧。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数错误。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // 构造固定版本取消帧。
        Ok(Self {
            // 禁止协议版本由调用方选择。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 复制很小的一次性关联值。
            request_nonce: request_nonce.to_owned(),
            // 固定为取消控制。
            kind: SequenceStepControlKind::Cancel,
        })
    }

    // 严格解析并核对当前请求关联值。
    pub(crate) fn parse(
        // 接收单个 control JSON 行。
        text: &str,
        // 接收当前 worker 请求 nonce。
        expected_nonce: &str,
    ) -> Result<Self, SequenceStepProtocolFailure> {
        // control 与请求共享同一个输入硬上限。
        if text.len() > MAXIMUM_REQUEST_BYTES {
            // 在 JSON 解析前拒绝无界输入。
            return Err(SequenceStepProtocolFailure::new(
                // 使用请求过大错误。
                SequenceStepProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 只允许一个非空 JSON 行。
        let normalized = text.trim();
        // 拒绝空值或多行控制。
        if normalized.is_empty() || normalized.contains(['\r', '\n']) {
            // 返回协议参数错误。
            return Err(SequenceStepProtocolFailure::new(
                // 使用普通参数错误。
                SequenceStepProtocolErrorCode::InvalidArgument,
            ));
        }
        // 严格解析封闭 control 对象。
        let control = serde_json::from_str::<Self>(normalized).map_err(|_| {
            // 不回显原始 control。
            SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::InvalidArgument)
        })?;
        // 版本、canonical nonce 与当前请求必须完全一致。
        if control.contract_version != CONTRACT_VERSION
            // 核对自身 nonce 形状。
            || !is_canonical_nonce(&control.request_nonce)
            // 核对当前请求关联。
            || control.request_nonce != expected_nonce
        {
            // 关联漂移是协议失败而非 provider 取消。
            return Err(SequenceStepProtocolFailure::new(
                // 使用协议状态错误。
                SequenceStepProtocolErrorCode::ProtocolFailed,
            ));
        }
        // 返回唯一有效取消帧。
        Ok(control)
    }

    // 把取消 control 序列化为单个 JSON 行。
    pub(crate) fn to_line(&self) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
        // 只序列化固定派生对象。
        let mut bytes = serde_json::to_vec(self).map_err(|_| {
            // 结构化派生失败视为协议错误。
            SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::ProtocolFailed)
        })?;
        // control 必须远小于共享请求边界。
        if bytes.len() > MAXIMUM_REQUEST_BYTES {
            // 返回请求过大。
            return Err(SequenceStepProtocolFailure::new(
                // 使用封闭资源错误。
                SequenceStepProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 固定追加 JSON Lines 换行。
        bytes.push(b'\n');
        // 返回唯一 control 帧。
        Ok(bytes)
    }
}

// 保存单次 worker 请求已经接受的 control 状态。
#[derive(Debug, Default)]
pub(crate) struct SequenceStepControlState {
    // 保存是否已经接受唯一 cancel 帧。
    cancelled: bool,
}

// 实现每个请求至多一次的 control 状态机。
impl SequenceStepControlState {
    // 创建尚未接受 control 的请求状态。
    pub(crate) const fn new() -> Self {
        // 初始未取消。
        Self { cancelled: false }
    }

    // 接受并消费当前请求的唯一 cancel 帧。
    pub(crate) fn accept_cancel(
        // 取得状态的唯一可变权。
        &mut self,
        // 接收单个 control JSON 行。
        text: &str,
        // 接收当前请求 nonce。
        expected_nonce: &str,
    ) -> Result<(), SequenceStepProtocolFailure> {
        // 重复 control 不得重新触发 Module 取消。
        if self.cancelled {
            // 返回协议状态错误。
            return Err(SequenceStepProtocolFailure::new(
                // 使用协议失败类别。
                SequenceStepProtocolErrorCode::ProtocolFailed,
            ));
        }
        // 严格解析版本、nonce 与唯一 cancel 类别。
        let _ = SequenceStepWorkerControl::parse(text, expected_nonce)?;
        // 只在完整验证后提交取消状态。
        self.cancelled = true;
        // 返回唯一 control 已接受。
        Ok(())
    }

    // 返回当前请求是否已经接受 cancel。
    #[cfg(test)]
    pub(crate) const fn is_cancelled(&self) -> bool {
        // 复制封闭布尔事实。
        self.cancelled
    }
}

// 注册 accepted 与 final 双阶段 stdout 帧状态机。
#[path = "sequence_step_protocol_frames.rs"]
pub(crate) mod frames;
// 验证 canonical 一次性请求 nonce。
fn validate_nonce(value: &str) -> Result<(), SequenceStepProtocolFailure> {
    // 只接受固定长度小写十六进制。
    if !is_canonical_nonce(value) {
        // 拒绝可变长度或宽松关联值。
        return Err(SequenceStepProtocolFailure::new(
            // 使用普通参数错误。
            SequenceStepProtocolErrorCode::InvalidArgument,
        ));
    }
    // nonce 形状合法。
    Ok(())
}

// 判断文本是否为 canonical 128 位小写十六进制 nonce。
fn is_canonical_nonce(value: &str) -> bool {
    // 长度必须固定且每字节为小写十六进制。
    value.len() == NONCE_LENGTH
        // 遍历 ASCII 字节。
        && value
            // 读取原始字节。
            .bytes()
            // 只接受数字与小写 a-f。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 测量 JSON 对象的紧凑 UTF-8 字节数。
fn compact_map_bytes(value: &JsonMap) -> usize {
    // 直接序列化借用对象以避免复制拥有型参数。
    serde_json::to_vec(value)
        // 返回紧凑 JSON 字节数。
        .map(|bytes| bytes.len())
        // 理论序列化失败按无界负载拒绝。
        .unwrap_or(usize::MAX)
}

// 编译双阶段协议的独立纯回归。
#[cfg(test)]
#[path = "sequence_step_protocol_tests.rs"]
mod tests;
