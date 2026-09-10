//! 定义显式浏览器会话 worker 的封闭打开握手协议。

// 导入标准错误展示接口。
use std::error::Error;
// 导入安全错误格式化接口。
use std::fmt::{self, Display, Formatter};

// 导入严格 JSON 序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 固定浏览器会话 worker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/browser-session-worker/v1";
// 固定一次性 nonce 的小写十六进制长度。
const NONCE_LENGTH: usize = 32;
// 固定授权 endpoint opaque ID 前缀。
const ENDPOINT_PREFIX: &str = "bse1:";
// 固定浏览器会话 opaque ID 前缀。
const SESSION_PREFIX: &str = "s2:bs:";
// 固定单行输入的 UTF-8 字节上限。
pub(crate) const MAXIMUM_INPUT_BYTES: usize = 8 * 1024;
// 固定 worker stdout 的 UTF-8 字节上限。
pub(crate) const MAXIMUM_OUTPUT_BYTES: usize = 64 * 1024;
// 固定打开握手的最小 deadline。
const MINIMUM_TIMEOUT_MS: u32 = 1;
// 固定打开握手的最大 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

// 表示协议边界允许产生的封闭失败类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionProtocolErrorCode {
    // 表示请求字段或身份不合法。
    InvalidArgument,
    // 表示输入超过固定资源上限。
    RequestTooLarge,
    // 表示输出超过固定资源上限。
    OutputTooLarge,
    // 表示关联、顺序或状态漂移。
    ProtocolFailed,
}

// 为封闭协议失败提供稳定文本。
impl BrowserSessionProtocolErrorCode {
    // 返回 worker 与 Module 共用的唯一错误码文本。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举全部协议失败类别。
        match self {
            // 映射普通参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射请求资源拒绝。
            Self::RequestTooLarge => "WORKER_REQUEST_TOO_LARGE",
            // 映射输出资源拒绝。
            Self::OutputTooLarge => "WORKER_OUTPUT_TOO_LARGE",
            // 映射协议状态拒绝。
            Self::ProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }
}

// 保存不回显协议负载的解析失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionProtocolFailure {
    // 保存唯一封闭失败类别。
    code: BrowserSessionProtocolErrorCode,
}

// 为协议失败提供受控构造与只读投影。
impl BrowserSessionProtocolFailure {
    // 构造不携带输入值的协议失败。
    const fn new(code: BrowserSessionProtocolErrorCode) -> Self {
        // 只保存封闭错误类别。
        Self { code }
    }

    // 返回稳定封闭失败类别。
    pub(crate) const fn code(self) -> BrowserSessionProtocolErrorCode {
        // 复制无状态枚举。
        self.code
    }
}

// 为测试与受控调用方提供不含负载的错误展示。
impl Display for BrowserSessionProtocolFailure {
    // 只展示封闭错误码，避免泄漏不可信输入。
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // 写入稳定错误码文本。
        formatter.write_str(self.code.as_str())
    }
}

// 标记协议失败可通过标准错误链传播。
impl Error for BrowserSessionProtocolFailure {}

// 表示打开请求允许选择的两种封闭来源。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用 kind 内部标签并拒绝任意扩展字段。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BrowserSessionSource {
    // 表示私有 registry 签发的显式授权端点。
    AuthorizedEndpoint {
        // 保存不泄漏地址或端口的 endpoint ID。
        #[serde(rename = "endpointId")]
        endpoint_id: String,
        // 保存本次打开独占的授权 nonce。
        #[serde(rename = "authorizationNonce")]
        authorization_nonce: String,
    },
    // 表示完全由工具拥有且不接收调用方字段的空隔离 profile。
    IsolatedProfile {},
}

// 表示 host 发送给 worker 的打开或取消输入。
#[derive(Debug, Deserialize, Serialize)]
// 使用 kind 内部标签并拒绝任意扩展字段。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BrowserSessionWorkerInput {
    // 表示唯一打开请求。
    Open {
        // 保存固定协议版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存一次性请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 保存不重置的总 deadline。
        #[serde(rename = "timeoutMs")]
        timeout_ms: u32,
        // 保存封闭会话来源。
        source: BrowserSessionSource,
    },
    // 表示关联到同一打开请求的幂等取消。
    Cancel {
        // 保存固定协议版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存一次性请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
    },
}

// 为 worker 输入提供严格单行解析与只读投影。
impl BrowserSessionWorkerInput {
    // 解析一条有界 UTF-8 JSON Lines 输入。
    pub(crate) fn parse_line(line: &str) -> Result<Self, BrowserSessionProtocolFailure> {
        // 拒绝超过固定输入边界的请求。
        if line.len() > MAXIMUM_INPUT_BYTES {
            // 返回稳定请求过大类别。
            return Err(BrowserSessionProtocolFailure::new(
                // 使用资源边界错误。
                BrowserSessionProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 单次调用只能携带一条逻辑 JSON 行。
        if line.contains(['\r', '\n']) {
            // 拒绝行拼接与额外协议帧。
            return Err(BrowserSessionProtocolFailure::new(
                // 使用普通参数失败。
                BrowserSessionProtocolErrorCode::InvalidArgument,
            ));
        }
        // 使用严格 Serde 类型拒绝未知字段与错误变体。
        let input = serde_json::from_str::<Self>(line).map_err(|_| {
            // 不回显调用方负载。
            BrowserSessionProtocolFailure::new(BrowserSessionProtocolErrorCode::InvalidArgument)
        })?;
        // 验证跨字段身份与 deadline 约束。
        input.validate()?;
        // 返回已验证输入。
        Ok(input)
    }

    // 返回请求关联 nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 两种输入共享相同关联字段。
        match self {
            // 借用打开请求 nonce。
            Self::Open { request_nonce, .. }
            // 借用取消请求 nonce。
            | Self::Cancel { request_nonce, .. } => request_nonce,
        }
    }

    // 返回打开请求的可选来源。
    pub(crate) const fn source(&self) -> Option<&BrowserSessionSource> {
        // 只有打开请求携带来源。
        match self {
            // 借用封闭来源。
            Self::Open { source, .. } => Some(source),
            // 取消请求没有来源。
            Self::Cancel { .. } => None,
        }
    }

    // 验证输入的固定版本、关联值和来源组合。
    fn validate(&self) -> Result<(), BrowserSessionProtocolFailure> {
        // 拆出共享版本与关联字段。
        let (contract_version, request_nonce) = match self {
            // 打开请求还需验证 deadline 与来源。
            Self::Open {
                contract_version,
                request_nonce,
                timeout_ms,
                source,
            } => {
                // deadline 必须在冻结闭区间内。
                if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(timeout_ms) {
                    // 拒绝无效执行预算。
                    return Err(invalid_argument());
                }
                // 验证两种来源各自的身份约束。
                validate_source(source)?;
                // 返回共享字段继续核对。
                (contract_version, request_nonce)
            }
            // 取消请求只需验证共享字段。
            Self::Cancel {
                contract_version,
                request_nonce,
            } => (contract_version, request_nonce),
        };
        // 版本与一次性 nonce 必须 canonical。
        if contract_version != CONTRACT_VERSION || !is_canonical_nonce(request_nonce) {
            // 拒绝版本或关联值漂移。
            return Err(invalid_argument());
        }
        // 输入全部约束成立。
        Ok(())
    }
}

// 表示打开 final 允许公开的封闭结果类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用稳定 kebab-case 文本。
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrowserSessionOutcome {
    // 表示会话已确定打开。
    Ready,
    // 表示打开请求确定未 dispatch。
    NotDispatched,
    // 表示 dispatch 后确定失败。
    Failed,
    // 表示 accepted 后缺失可信终态。
    Unknown,
}

// 表示 worker stdout 允许出现的两类帧。
#[derive(Debug, Deserialize)]
// 使用 kind 内部标签并拒绝任意扩展字段。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum BrowserSessionWorkerFrame {
    // 表示 worker 即将产生会话资源。
    OpenAccepted {
        // 保存固定协议版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 固定表示 dispatch 已接受。
        #[serde(rename = "dispatchAccepted")]
        dispatch_accepted: bool,
        // 固定表示尚无最终结果。
        completed: bool,
    },
    // 表示 worker 建立了唯一打开终态。
    OpenFinal {
        // 保存固定协议版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 保存封闭打开结果。
        outcome: BrowserSessionOutcome,
        // 保存确定完成事实。
        completed: bool,
        // 保存自动重试安全事实。
        #[serde(rename = "retrySafe")]
        retry_safe: bool,
        // 保存 endpoint 或 runtime 可能已接受事实。
        #[serde(rename = "acceptedMayHaveOccurred")]
        accepted_may_have_occurred: bool,
        // 仅 ready 携带 opaque 会话 ID。
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        // 仅非 ready 携带安全错误。
        error: Option<Value>,
    },
}

// 保存从零至两帧 stdout 建立的最小可靠观察。
#[derive(Debug)]
pub(crate) struct BrowserSessionFrameObservation {
    // 保存是否看到合法 accepted 帧。
    accepted: bool,
    // 保存可选 final 观察。
    final_observation: Option<BrowserSessionFinalObservation>,
}

// 保存一个已经通过状态机核验的 final。
#[derive(Debug)]
pub(crate) struct BrowserSessionFinalObservation {
    // 保存封闭打开结果。
    outcome: BrowserSessionOutcome,
    // 保存确定完成事实。
    completed: bool,
    // 保存自动重试安全事实。
    retry_safe: bool,
    // 保存可能已接受事实。
    accepted_may_have_occurred: bool,
    // 保存可选 opaque 会话 ID。
    session_id: Option<String>,
    // 保存可选安全错误。
    error: Option<Value>,
}

// 为 stdout 观察提供只读投影。
impl BrowserSessionFrameObservation {
    // 返回是否已经建立 accepted 事实。
    pub(crate) const fn accepted(&self) -> bool {
        // 复制封闭布尔事实。
        self.accepted
    }

    // 返回可选最终观察。
    pub(crate) const fn final_observation(&self) -> Option<&BrowserSessionFinalObservation> {
        // 借用已验证 final。
        self.final_observation.as_ref()
    }
}

// 为 final 观察提供只读投影。
impl BrowserSessionFinalObservation {
    // 返回封闭打开结果。
    pub(crate) const fn outcome(&self) -> BrowserSessionOutcome {
        // 复制无状态枚举。
        self.outcome
    }

    // 返回确定完成事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制布尔事实。
        self.completed
    }

    // 返回自动重试安全事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制布尔事实。
        self.retry_safe
    }

    // 返回 endpoint 或 runtime 是否可能已接受。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制布尔事实。
        self.accepted_may_have_occurred
    }

    // 返回可选 opaque 会话 ID。
    pub(crate) fn session_id(&self) -> Option<&str> {
        // 借用已验证身份文本。
        self.session_id.as_deref()
    }

    // 返回可选安全错误对象。
    pub(crate) const fn error(&self) -> Option<&Value> {
        // 借用 provider-neutral 错误。
        self.error.as_ref()
    }
}

// 解析并验证 worker 的全部 stdout 帧。
pub(crate) fn observe_output(
    // 接收有界 UTF-8 stdout。
    output: &str,
    // 接收打开请求的预期 nonce。
    expected_nonce: &str,
) -> Result<BrowserSessionFrameObservation, BrowserSessionProtocolFailure> {
    // 输出不得超过固定资源上限。
    if output.len() > MAXIMUM_OUTPUT_BYTES {
        // 返回稳定输出过大类别。
        return Err(BrowserSessionProtocolFailure::new(
            // 使用资源边界错误。
            BrowserSessionProtocolErrorCode::OutputTooLarge,
        ));
    }
    // 预期关联值自身必须 canonical。
    if !is_canonical_nonce(expected_nonce) {
        // 调用方协议状态无效。
        return Err(protocol_failed());
    }
    // 初始化尚未 accepted 的观察。
    let mut observation = BrowserSessionFrameObservation {
        // 初始没有 accepted。
        accepted: false,
        // 初始没有 final。
        final_observation: None,
    };
    // 按 JSON Lines 顺序解析非空帧。
    for line in output.lines() {
        // 空行属于协议噪声而不是可忽略空白。
        if line.is_empty() {
            // 拒绝额外 stdout 内容。
            return Err(protocol_failed());
        }
        // final 后任何帧都必须拒绝。
        if observation.final_observation.is_some() {
            // 拒绝重复终态或尾随噪声。
            return Err(protocol_failed());
        }
        // 严格解析当前帧。
        let frame = serde_json::from_str::<BrowserSessionWorkerFrame>(line)
            // 不回显不可信 stdout。
            .map_err(|_| protocol_failed())?;
        // 验证当前帧关联、顺序与字段组合。
        apply_frame(&mut observation, frame, expected_nonce)?;
    }
    // 返回零帧、accepted-only 或完整 final 观察。
    Ok(observation)
}

// 把单帧应用到浏览器打开状态机。
fn apply_frame(
    // 可变借用当前观察。
    observation: &mut BrowserSessionFrameObservation,
    // 取得严格解析帧。
    frame: BrowserSessionWorkerFrame,
    // 借用预期请求 nonce。
    expected_nonce: &str,
) -> Result<(), BrowserSessionProtocolFailure> {
    // 按封闭帧种类推进状态。
    match frame {
        // 处理 accepted 帧。
        BrowserSessionWorkerFrame::OpenAccepted {
            contract_version,
            request_nonce,
            dispatch_accepted,
            completed,
        } => {
            // accepted 只能出现一次且必须携带固定事实。
            if observation.accepted
                // 核对版本和关联值。
                || !correlates(&contract_version, &request_nonce, expected_nonce)
                // accepted 字段必须逐字成立。
                || !dispatch_accepted
                // accepted 尚未完成。
                || completed
            {
                // 拒绝重复或矛盾 accepted。
                return Err(protocol_failed());
            }
            // 建立 accepted 事实。
            observation.accepted = true;
        }
        // 处理唯一 final 帧。
        BrowserSessionWorkerFrame::OpenFinal {
            contract_version,
            request_nonce,
            outcome,
            completed,
            retry_safe,
            accepted_may_have_occurred,
            session_id,
            error,
        } => {
            // final 必须关联同一请求。
            if !correlates(&contract_version, &request_nonce, expected_nonce) {
                // 拒绝关联漂移。
                return Err(protocol_failed());
            }
            // 验证 outcome、accepted 和负载字段一致。
            validate_final(
                // 传递当前 accepted 事实。
                observation.accepted,
                // 传递封闭结果。
                outcome,
                // 传递确定完成事实。
                completed,
                // 传递重试事实。
                retry_safe,
                // 传递可能接受事实。
                accepted_may_have_occurred,
                // 借用可选会话 ID。
                session_id.as_deref(),
                // 借用可选错误。
                error.as_ref(),
            )?;
            // 保存已验证 final。
            observation.final_observation = Some(BrowserSessionFinalObservation {
                // 保存封闭结果。
                outcome,
                // 保存确定完成事实。
                completed,
                // 保存重试事实。
                retry_safe,
                // 保存可能接受事实。
                accepted_may_have_occurred,
                // 保存 opaque 会话 ID。
                session_id,
                // 保存安全错误。
                error,
            });
        }
    }
    // 当前帧合法应用。
    Ok(())
}

// 验证 final 的跨字段与负载不变量。
#[allow(clippy::too_many_arguments)]
fn validate_final(
    // 接收前序 accepted 事实。
    accepted: bool,
    // 接收封闭结果。
    outcome: BrowserSessionOutcome,
    // 接收确定完成事实。
    completed: bool,
    // 接收重试事实。
    retry_safe: bool,
    // 接收可能接受事实。
    accepted_may_have_occurred: bool,
    // 借用可选会话 ID。
    session_id: Option<&str>,
    // 借用可选错误。
    error: Option<&Value>,
) -> Result<(), BrowserSessionProtocolFailure> {
    // 按封闭 outcome 核对唯一合法组合。
    let valid = match outcome {
        // ready 必须在 accepted 后完成且只携带 canonical session ID。
        BrowserSessionOutcome::Ready => {
            accepted
                && completed
                && !retry_safe
                && accepted_may_have_occurred
                && session_id.is_some_and(is_canonical_session_id)
                && error.is_none()
        }
        // 未 dispatch 必须无 accepted、可安全重试且只携带安全错误。
        BrowserSessionOutcome::NotDispatched => {
            !accepted
                && completed
                && retry_safe
                && !accepted_may_have_occurred
                && session_id.is_none()
                && error.is_some_and(is_safe_error)
        }
        // 确定失败必须在 accepted 后完成且只携带安全错误。
        BrowserSessionOutcome::Failed => {
            accepted
                && completed
                && !retry_safe
                && accepted_may_have_occurred
                && session_id.is_none()
                && error.is_some_and(is_safe_error)
        }
        // 未知必须在 accepted 后保持未完成且使用固定错误码。
        BrowserSessionOutcome::Unknown => {
            accepted
                && !completed
                && !retry_safe
                && accepted_may_have_occurred
                && session_id.is_none()
                && error.is_some_and(|value| {
                    // 先验证安全错误形状。
                    is_safe_error(value)
                        // 再核对唯一未知结果码。
                        && value.get("code").and_then(Value::as_str) == Some("OUTCOME_UNKNOWN")
                })
        }
    };
    // 非法组合失败闭合。
    if !valid {
        // 返回协议状态失败。
        return Err(protocol_failed());
    }
    // final 全部不变量成立。
    Ok(())
}

// 验证封闭来源的身份字段。
fn validate_source(source: &BrowserSessionSource) -> Result<(), BrowserSessionProtocolFailure> {
    // 按来源种类验证必要字段。
    match source {
        // 授权 endpoint 必须携带两项 canonical opaque 事实。
        BrowserSessionSource::AuthorizedEndpoint {
            endpoint_id,
            authorization_nonce,
        } if is_canonical_endpoint_id(endpoint_id)
            // 授权 nonce 必须 canonical。
            && is_canonical_nonce(authorization_nonce) =>
        {
            Ok(())
        }
        // 隔离 profile 不携带任何调用方路径或参数。
        BrowserSessionSource::IsolatedProfile {} => Ok(()),
        // 其余授权来源组合都拒绝。
        BrowserSessionSource::AuthorizedEndpoint { .. } => Err(invalid_argument()),
    }
}

// 判断版本和 nonce 是否关联预期请求。
fn correlates(contract_version: &str, request_nonce: &str, expected_nonce: &str) -> bool {
    // 版本必须逐字匹配。
    contract_version == CONTRACT_VERSION
        // 关联值必须逐字匹配。
        && request_nonce == expected_nonce
        // 关联值自身必须 canonical。
        && is_canonical_nonce(request_nonce)
}

// 判断文本是否为 canonical 128-bit 小写十六进制 nonce。
fn is_canonical_nonce(value: &str) -> bool {
    // 长度必须精确为三十二字节。
    value.len() == NONCE_LENGTH
        // 每个字节只能是小写十六进制。
        && value
            // 遍历 ASCII 字节。
            .bytes()
            // 核对封闭字符集。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 判断文本是否为私有 registry 签发的 endpoint opaque ID。
fn is_canonical_endpoint_id(value: &str) -> bool {
    // 只接受固定前缀后的 canonical nonce。
    value
        // 剥离固定 endpoint 类型前缀。
        .strip_prefix(ENDPOINT_PREFIX)
        // 验证剩余随机身份。
        .is_some_and(is_canonical_nonce)
}

// 判断文本是否为 Browser Session Module 的 opaque 会话 ID。
fn is_canonical_session_id(value: &str) -> bool {
    // 只接受固定前缀后的 canonical nonce。
    value
        // 剥离固定 session 类型前缀。
        .strip_prefix(SESSION_PREFIX)
        // 验证剩余随机身份。
        .is_some_and(is_canonical_nonce)
}

// 判断错误对象是否满足安全有界形状。
fn is_safe_error(value: &Value) -> bool {
    // 错误必须是 JSON 对象。
    let Some(object) = value.as_object() else {
        // 非对象直接拒绝。
        return false;
    };
    // 只允许 code、message 与可选 details。
    if object.keys().any(|key| {
        // 未知字段不得穿透 worker。
        !matches!(key.as_str(), "code" | "message" | "details")
    }) {
        // 拒绝未知错误字段。
        return false;
    }
    // code 必须是 1..=64 的大写稳定类别。
    let code_valid = object
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(|code| {
            // 核对长度和封闭字符集。
            !code.is_empty()
            && code.len() <= 64
            // 稳定错误码必须以大写字母开始。
            && code.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            && code
                    // 遍历 ASCII 字节。
                    .bytes()
                    // 允许大写字母、数字与下划线。
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        });
    // message 必须是 1..=512 UTF-8 字节。
    let message_valid = object
        // 读取必需消息。
        .get("message")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 核对有界非空文本。
        .is_some_and(|message| !message.is_empty() && message.len() <= 512);
    // 两项必需字段都必须有效。
    code_valid && message_valid
}

// 构造不携带负载的参数失败。
const fn invalid_argument() -> BrowserSessionProtocolFailure {
    // 使用稳定普通参数类别。
    BrowserSessionProtocolFailure::new(BrowserSessionProtocolErrorCode::InvalidArgument)
}

// 构造不携带 stdout 的协议状态失败。
const fn protocol_failed() -> BrowserSessionProtocolFailure {
    // 使用稳定协议失败类别。
    BrowserSessionProtocolFailure::new(BrowserSessionProtocolErrorCode::ProtocolFailed)
}

// 仅在当前 Component 单元测试中加载协议回归。
#[cfg(test)]
#[path = "browser_session_protocol_tests.rs"]
mod tests;
