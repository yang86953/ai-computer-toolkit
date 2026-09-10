//! 定义 Browser Session Module 与固定 worker 的页面命令协议。

// 导入安全错误展示接口。
use std::{
    // 导入标准错误 trait。
    error::Error,
    // 导入格式化接口。
    fmt::{self, Display, Formatter},
};

// 导入严格 JSON 派生。
use serde::{Deserialize, Serialize};
// 导入中立 JSON 值。
use serde_json::Value;

// 把复杂成功数据和安全错误验证拆到同一 Component 的窄文件。
#[path = "browser_page_protocol_result.rs"]
mod result;
// 把页面操作验证拆到同一 Component 的窄文件。
#[path = "browser_page_protocol_validation.rs"]
mod validation;
// 导入结果验证器。
use result::{is_safe_error, validate_success_data};
// 导入操作验证入口。
use validation::validate_operation;

// 固定页面命令协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/browser-page-command-worker/v1";
// 固定单行命令输入上限。
pub(crate) const MAXIMUM_INPUT_BYTES: usize = 64 * 1024;
// 固定单个命令输出上限，包含 16 MiB Base64 与 128 KiB JSON envelope。
pub(crate) const MAXIMUM_OUTPUT_BYTES: usize = (16 * 1024 * 1024) + (128 * 1024);
// 固定 nonce 长度。
const NONCE_LENGTH: usize = 32;
// 固定 session ID 前缀。
const SESSION_PREFIX: &str = "s2:bs:";
// 固定 worker 私有 page ref 前缀。
const PAGE_REF_PREFIX: &str = "w1:bp:";
// 固定 worker 私有 element ref 前缀。
const ELEMENT_REF_PREFIX: &str = "w1:be:";
// 固定命令 deadline 范围。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
// 固定 selector 文本长度。
const MAXIMUM_SELECTOR_TEXT_BYTES: usize = 1_024;
// 固定输入文本长度。
pub(crate) const MAXIMUM_TYPE_TEXT_BYTES: usize = 16 * 1024;
// 固定 URL 长度。
const MAXIMUM_URL_BYTES: usize = 8 * 1024;
// 固定查询结果数量。
const MAXIMUM_QUERY_RESULTS: u16 = 100;
// 固定截图字节上限。
pub(crate) const MAXIMUM_PNG_BYTES: u64 = 12 * 1024 * 1024;

// 表示页面协议允许产生的封闭失败类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserPageProtocolErrorCode {
    // 表示输入参数或身份不合法。
    InvalidArgument,
    // 表示请求超过资源边界。
    RequestTooLarge,
    // 表示结果超过资源边界。
    OutputTooLarge,
    // 表示关联、顺序或字段组合漂移。
    ProtocolFailed,
}

// 为协议错误码提供稳定文本。
impl BrowserPageProtocolErrorCode {
    // 返回唯一错误码文本。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举封闭集合。
        match self {
            // 映射普通参数失败。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射请求超限。
            Self::RequestTooLarge => "WORKER_REQUEST_TOO_LARGE",
            // 映射输出超限。
            Self::OutputTooLarge => "WORKER_OUTPUT_TOO_LARGE",
            // 映射协议漂移。
            Self::ProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }
}

// 保存不回显协议负载的失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserPageProtocolFailure {
    // 保存封闭错误类别。
    code: BrowserPageProtocolErrorCode,
}

// 为协议失败提供构造和只读投影。
impl BrowserPageProtocolFailure {
    // 构造封闭失败。
    const fn new(code: BrowserPageProtocolErrorCode) -> Self {
        // 只保存类别。
        Self { code }
    }

    // 返回稳定错误类别。
    pub(crate) const fn code(self) -> BrowserPageProtocolErrorCode {
        // 复制无状态枚举。
        self.code
    }
}

// 只展示稳定错误码。
impl Display for BrowserPageProtocolFailure {
    // 格式化安全文本。
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // 不回显输入。
        formatter.write_str(self.code.as_str())
    }
}

// 标记协议失败可进入标准错误链。
impl Error for BrowserPageProtocolFailure {}

// 表示 provider-neutral 元素 selector。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 拒绝 CSS、XPath、脚本和其他扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserElementSelector {
    // 保存可选语义 role。
    role: Option<String>,
    // 保存可选可访问名称。
    name: Option<String>,
    // 保存可选可见文本。
    text: Option<String>,
    // 保存是否要求逐字匹配。
    exact: bool,
}

// 为 provider-neutral selector 提供只读匹配事实。
impl BrowserElementSelector {
    // 从 Module provider-neutral 字段建立待统一验证的 selector。
    pub(crate) fn new(
        // 接收可选 role。
        role: Option<String>,
        // 接收可选名称。
        name: Option<String>,
        // 接收可选文本。
        text: Option<String>,
        // 接收逐字匹配要求。
        exact: bool,
    ) -> Self {
        // 只构造强类型值，边界仍由 operation 唯一验证器决定。
        Self {
            // 保存 role。
            role,
            // 保存名称。
            name,
            // 保存文本。
            text,
            // 保存匹配方式。
            exact,
        }
    }

    // 返回可选语义 role。
    pub(crate) fn role(&self) -> Option<&str> {
        // 借用已验证 role。
        self.role.as_deref()
    }

    // 返回可选可访问名称。
    pub(crate) fn name(&self) -> Option<&str> {
        // 借用已验证名称。
        self.name.as_deref()
    }

    // 返回可选可见文本。
    pub(crate) fn text(&self) -> Option<&str> {
        // 借用已验证文本。
        self.text.as_deref()
    }

    // 返回是否要求逐字匹配。
    pub(crate) const fn exact(&self) -> bool {
        // 复制布尔值。
        self.exact
    }
}

// 表示 wait 允许的封闭条件。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 kind 标签并拒绝扩展。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BrowserWaitCondition {
    // 等待文档 readyState 完成。
    DocumentReady {},
    // 等待 provider-neutral selector 至少一个命中。
    ElementPresent {
        // 保存强类型 selector。
        selector: BrowserElementSelector,
    },
    // 等待有界可见文本出现。
    TextPresent {
        // 保存待匹配文本。
        text: String,
        // 保存是否逐字匹配。
        exact: bool,
    },
}

// 表示页面命令允许的封闭操作。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 operation 内部标签并拒绝任意扩展。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BrowserPageOperation {
    // 导航到显式 http/https URL。
    Navigate {
        // 保存有界 URL。
        url: String,
    },
    // 等待封闭页面条件。
    Wait {
        // 保存 wait 条件。
        condition: BrowserWaitCondition,
    },
    // 查询 provider-neutral 元素。
    Query {
        // 保存 selector。
        selector: BrowserElementSelector,
        // 保存结果上限。
        #[serde(rename = "maxResults")]
        max_results: u16,
    },
    // 点击已重解析的 element ref。
    Click {
        // worker 必须独立重验显式确认。
        confirmed: bool,
        // 保存 worker 私有 element ref。
        #[serde(rename = "elementRef")]
        element_ref: String,
    },
    // 向已重解析元素输入文本。
    Type {
        // worker 必须独立重验显式确认。
        confirmed: bool,
        // 保存 worker 私有 element ref。
        #[serde(rename = "elementRef")]
        element_ref: String,
        // 保存有界 UTF-8 文本。
        text: String,
        // 保存是否替换现有值。
        replace: bool,
    },
    // 捕获当前页面 PNG。
    Screenshot {},
}

// 为操作提供稳定种类。
impl BrowserPageOperation {
    // 在任何 worker I/O 前验证操作自有边界。
    pub(crate) fn validate(&self) -> Result<(), BrowserPageProtocolFailure> {
        // 复用协议唯一验证器，避免 Module 复制规则。
        validate_operation(self)
    }

    // 返回帧结果关联的固定 kind。
    pub(crate) const fn kind(&self) -> BrowserPageOperationKind {
        // 穷举全部操作。
        match self {
            // 映射导航。
            Self::Navigate { .. } => BrowserPageOperationKind::Navigate,
            // 映射等待。
            Self::Wait { .. } => BrowserPageOperationKind::Wait,
            // 映射查询。
            Self::Query { .. } => BrowserPageOperationKind::Query,
            // 映射点击。
            Self::Click { .. } => BrowserPageOperationKind::Click,
            // 映射输入。
            Self::Type { .. } => BrowserPageOperationKind::Type,
            // 映射截图。
            Self::Screenshot {} => BrowserPageOperationKind::Screenshot,
        }
    }
}

// 表示 host 向 worker 发送的命令或取消。
#[derive(Debug, Deserialize, Serialize)]
// 使用 kind 标签并拒绝扩展字段。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum BrowserPageWorkerInput {
    // 执行一项页面命令。
    Command {
        // 保存固定版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存当前会话身份。
        #[serde(rename = "sessionId")]
        session_id: String,
        // 保存命令随机关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 保存不重置总 deadline。
        #[serde(rename = "timeoutMs")]
        timeout_ms: u32,
        // 保存可选 worker 私有 page ref。
        #[serde(rename = "pageRef")]
        page_ref: Option<String>,
        // 保存 Module 观察的导航代际。
        #[serde(rename = "navigationGeneration")]
        navigation_generation: u64,
        // 保存强类型操作。
        operation: BrowserPageOperation,
    },
    // 幂等取消同一命令。
    CancelCommand {
        // 保存固定版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存当前会话身份。
        #[serde(rename = "sessionId")]
        session_id: String,
        // 关联命令随机值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
    },
}

// 为输入提供严格解析和只读投影。
impl BrowserPageWorkerInput {
    // 解析一条有界 JSON Lines 输入。
    pub(crate) fn parse_line(line: &str) -> Result<Self, BrowserPageProtocolFailure> {
        // 拒绝资源超限。
        if line.len() > MAXIMUM_INPUT_BYTES {
            // 返回请求过大。
            return Err(BrowserPageProtocolFailure::new(
                // 使用固定类别。
                BrowserPageProtocolErrorCode::RequestTooLarge,
            ));
        }
        // 一次只允许一条逻辑行。
        if line.contains(['\r', '\n']) {
            // 拒绝帧拼接。
            return Err(invalid_argument());
        }
        // 严格反序列化。
        let input = serde_json::from_str::<Self>(line)
            // 不回显不可信负载。
            .map_err(|_| invalid_argument())?;
        // 验证跨字段约束。
        input.validate()?;
        // 返回已验证输入。
        Ok(input)
    }

    // 返回请求 nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 两种输入共享关联字段。
        match self {
            // 借用 command nonce。
            Self::Command { request_nonce, .. }
            // 借用 cancel nonce。
            | Self::CancelCommand { request_nonce, .. } => request_nonce,
        }
    }

    // 返回关联会话身份。
    pub(crate) fn session_id(&self) -> &str {
        // 两种输入共享会话字段。
        match self {
            // 借用 command 会话。
            Self::Command { session_id, .. }
            // 借用 cancel 会话。
            | Self::CancelCommand { session_id, .. } => session_id,
        }
    }

    // 返回可选操作种类。
    pub(crate) const fn operation_kind(&self) -> Option<BrowserPageOperationKind> {
        // 只有 command 携带操作。
        match self {
            // 返回操作种类。
            Self::Command { operation, .. } => Some(operation.kind()),
            // cancel 不携带操作。
            Self::CancelCommand { .. } => None,
        }
    }

    // 验证身份、deadline、代际和操作组合。
    pub(crate) fn validate(&self) -> Result<(), BrowserPageProtocolFailure> {
        // 拆出共享字段。
        let (contract_version, session_id, request_nonce) = match self {
            // command 还需验证页面与操作。
            Self::Command {
                contract_version,
                session_id,
                request_nonce,
                timeout_ms,
                page_ref,
                navigation_generation,
                operation,
            } => {
                // deadline 必须位于冻结范围。
                if !(1..=MAXIMUM_TIMEOUT_MS).contains(timeout_ms) {
                    // 拒绝无效预算。
                    return Err(invalid_argument());
                }
                // 导航代际必须保留零作为尚未导航初态。
                if *navigation_generation > u64::from(u32::MAX) {
                    // 拒绝无界代际。
                    return Err(invalid_argument());
                }
                // 非导航操作必须绑定 canonical page ref。
                if !matches!(operation, BrowserPageOperation::Navigate { .. })
                    // 其他操作必须有 page ref。
                    && !page_ref.as_deref().is_some_and(is_page_ref)
                {
                    // 拒绝未绑定页面。
                    return Err(invalid_argument());
                }
                // 可选 page ref 自身必须 canonical。
                if page_ref.as_deref().is_some_and(|value| !is_page_ref(value)) {
                    // 拒绝原生或错误身份。
                    return Err(invalid_argument());
                }
                // 验证操作自有边界。
                validate_operation(operation)?;
                // 返回共享字段。
                (contract_version, session_id, request_nonce)
            }
            // cancel 只验证共享身份。
            Self::CancelCommand {
                contract_version,
                session_id,
                request_nonce,
            } => (contract_version, session_id, request_nonce),
        };
        // 版本、session 与 nonce 必须 canonical。
        if contract_version != CONTRACT_VERSION
            // 核对 session ID。
            || !is_prefixed_nonce(session_id, SESSION_PREFIX)
            // 核对 request nonce。
            || !is_nonce(request_nonce)
        {
            // 拒绝共享身份漂移。
            return Err(invalid_argument());
        }
        // 输入全部约束成立。
        Ok(())
    }
}

// 表示页面操作固定种类。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用稳定 kebab-case 文本。
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrowserPageOperationKind {
    // 导航。
    Navigate,
    // 等待。
    Wait,
    // 查询。
    Query,
    // 点击。
    Click,
    // 输入。
    Type,
    // 截图。
    Screenshot,
}

// 表示 command final 的封闭 outcome。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用稳定 kebab-case 文本。
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrowserPageOutcome {
    // 命令确定完成。
    Completed,
    // 命令确定未派发。
    NotDispatched,
    // 派发后确定失败。
    Failed,
    // accepted 后终态未知。
    Unknown,
}

// 表示 worker stdout 两类页面命令帧。
#[derive(Debug, Deserialize)]
// 使用 kind 标签并拒绝扩展。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum BrowserPageWorkerFrame {
    // 表示命令即将 dispatch。
    CommandAccepted {
        // 保存固定版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 保存操作种类。
        operation: BrowserPageOperationKind,
        // 固定接受事实。
        #[serde(rename = "dispatchAccepted")]
        dispatch_accepted: bool,
        // accepted 尚未完成。
        completed: bool,
    },
    // 表示唯一命令终态。
    CommandFinal {
        // 保存固定版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 保存操作种类。
        operation: BrowserPageOperationKind,
        // 保存封闭 outcome。
        outcome: BrowserPageOutcome,
        // 保存完成事实。
        completed: bool,
        // 保存重试事实。
        #[serde(rename = "retrySafe")]
        retry_safe: bool,
        // 保存可能接受事实。
        #[serde(rename = "acceptedMayHaveOccurred")]
        accepted_may_have_occurred: bool,
        // 保存导航代际。
        #[serde(rename = "navigationGeneration")]
        navigation_generation: u64,
        // 保存可选成功数据。
        data: Option<Value>,
        // 保存可选安全错误。
        error: Option<Value>,
    },
}

// 保存零至两帧的可靠观察。
#[derive(Debug)]
pub(crate) struct BrowserPageFrameObservation {
    // 保存是否观察 accepted。
    accepted: bool,
    // 保存可选 final。
    final_observation: Option<BrowserPageFinalObservation>,
}

// 保存已验证 command final。
#[derive(Debug)]
pub(crate) struct BrowserPageFinalObservation {
    // 保存 outcome。
    outcome: BrowserPageOutcome,
    // 保存导航代际。
    navigation_generation: u64,
    // 保存可选成功数据。
    data: Option<Value>,
    // 保存可选安全错误。
    error: Option<Value>,
}

// 为帧观察提供只读投影。
impl BrowserPageFrameObservation {
    // 返回 accepted 事实。
    pub(crate) const fn accepted(&self) -> bool {
        // 复制布尔值。
        self.accepted
    }

    // 返回可选 final。
    pub(crate) const fn final_observation(&self) -> Option<&BrowserPageFinalObservation> {
        // 借用 final。
        self.final_observation.as_ref()
    }
}

// 为 final 提供只读投影。
impl BrowserPageFinalObservation {
    // 返回 outcome。
    pub(crate) const fn outcome(&self) -> BrowserPageOutcome {
        // 复制枚举。
        self.outcome
    }

    // 返回导航代际。
    pub(crate) const fn navigation_generation(&self) -> u64 {
        // 复制代际。
        self.navigation_generation
    }

    // 返回可选成功数据。
    pub(crate) const fn data(&self) -> Option<&Value> {
        // 借用数据。
        self.data.as_ref()
    }

    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&Value> {
        // 借用错误。
        self.error.as_ref()
    }
}

// 解析并验证一个命令的全部 stdout 帧。
pub(crate) fn observe_output(
    // 接收有界 JSON Lines 输出。
    output: &str,
    // 接收预期 nonce。
    expected_nonce: &str,
    // 接收预期操作。
    expected_operation: BrowserPageOperationKind,
) -> Result<BrowserPageFrameObservation, BrowserPageProtocolFailure> {
    // 拒绝输出超限。
    if output.len() > MAXIMUM_OUTPUT_BYTES {
        // 返回资源错误。
        return Err(BrowserPageProtocolFailure::new(
            // 使用输出超限类别。
            BrowserPageProtocolErrorCode::OutputTooLarge,
        ));
    }
    // 预期 nonce 自身必须 canonical。
    if !is_nonce(expected_nonce) {
        // 调用方状态错误。
        return Err(protocol_failed());
    }
    // 初始化零帧观察。
    let mut observation = BrowserPageFrameObservation {
        // 尚未 accepted。
        accepted: false,
        // 尚无 final。
        final_observation: None,
    };
    // 逐行解析非空输出。
    for line in output.lines() {
        // 空行和 final 后尾随帧都失败。
        if line.is_empty() || observation.final_observation.is_some() {
            // 返回协议失败。
            return Err(protocol_failed());
        }
        // 严格解析帧。
        let frame = serde_json::from_str::<BrowserPageWorkerFrame>(line)
            // 不回显输出。
            .map_err(|_| protocol_failed())?;
        // 应用当前帧。
        apply_frame(
            // 更新观察。
            &mut observation,
            // 传入帧。
            frame,
            // 传入关联 nonce。
            expected_nonce,
            // 传入操作种类。
            expected_operation,
        )?;
    }
    // 返回零帧、accepted-only 或 final。
    Ok(observation)
}

// 把一帧应用到严格状态机。
fn apply_frame(
    // 可变借用观察。
    observation: &mut BrowserPageFrameObservation,
    // 取得当前帧。
    frame: BrowserPageWorkerFrame,
    // 借用预期 nonce。
    expected_nonce: &str,
    // 接收预期操作。
    expected_operation: BrowserPageOperationKind,
) -> Result<(), BrowserPageProtocolFailure> {
    // 按帧种类推进。
    match frame {
        // 处理 accepted。
        BrowserPageWorkerFrame::CommandAccepted {
            contract_version,
            request_nonce,
            operation,
            dispatch_accepted,
            completed,
        } => {
            // accepted 只能出现一次并逐字关联。
            if observation.accepted
                // 核对版本和 nonce。
                || !correlates(&contract_version, &request_nonce, expected_nonce)
                // 核对操作。
                || operation != expected_operation
                // 核对接受事实。
                || !dispatch_accepted
                // accepted 尚未完成。
                || completed
            {
                // 拒绝状态漂移。
                return Err(protocol_failed());
            }
            // 建立 accepted 事实。
            observation.accepted = true;
        }
        // 处理唯一 final。
        BrowserPageWorkerFrame::CommandFinal {
            contract_version,
            request_nonce,
            operation,
            outcome,
            completed,
            retry_safe,
            accepted_may_have_occurred,
            navigation_generation,
            data,
            error,
        } => {
            // final 必须逐字关联同一命令。
            if !correlates(&contract_version, &request_nonce, expected_nonce)
                // 操作种类不得漂移。
                || operation != expected_operation
                // 代际保持有界。
                || navigation_generation > u64::from(u32::MAX)
            {
                // 拒绝关联漂移。
                return Err(protocol_failed());
            }
            // 验证 outcome 字段组合。
            validate_final(
                // 传入 accepted 事实。
                observation.accepted,
                // 传入操作。
                operation,
                // 传入 outcome。
                outcome,
                // 传入完成事实。
                completed,
                // 传入重试事实。
                retry_safe,
                // 传入接受事实。
                accepted_may_have_occurred,
                // 借用数据。
                data.as_ref(),
                // 借用错误。
                error.as_ref(),
            )?;
            // 保存已验证 final。
            observation.final_observation = Some(BrowserPageFinalObservation {
                // 保存 outcome。
                outcome,
                // 保存代际。
                navigation_generation,
                // 保存成功数据。
                data,
                // 保存安全错误。
                error,
            });
        }
    }
    // 当前帧合法。
    Ok(())
}

// 验证 final 的唯一合法组合。
#[allow(clippy::too_many_arguments)]
fn validate_final(
    // 接收 accepted 事实。
    accepted: bool,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收 outcome。
    outcome: BrowserPageOutcome,
    // 接收完成事实。
    completed: bool,
    // 接收重试事实。
    retry_safe: bool,
    // 接收接受可能。
    accepted_may_have_occurred: bool,
    // 借用成功数据。
    data: Option<&Value>,
    // 借用安全错误。
    error: Option<&Value>,
) -> Result<(), BrowserPageProtocolFailure> {
    // 按 outcome 核对字段。
    let valid = match outcome {
        // completed 必须 accepted 且携带匹配操作数据。
        BrowserPageOutcome::Completed => {
            accepted
                && completed
                && !retry_safe
                && accepted_may_have_occurred
                && data.is_some_and(|value| validate_success_data(operation, value))
                && error.is_none()
        }
        // 未派发不得 accepted 且只携带安全错误。
        BrowserPageOutcome::NotDispatched => {
            !accepted
                && completed
                && retry_safe
                && !accepted_may_have_occurred
                && data.is_none()
                && error.is_some_and(is_safe_error)
        }
        // 确定失败必须 accepted 且只携带安全错误。
        BrowserPageOutcome::Failed => {
            accepted
                && completed
                && !retry_safe
                && accepted_may_have_occurred
                && data.is_none()
                && error.is_some_and(is_safe_error)
        }
        // unknown 必须 accepted、未完成且使用固定错误码。
        BrowserPageOutcome::Unknown => {
            accepted
                && !completed
                && !retry_safe
                && accepted_may_have_occurred
                && data.is_none()
                && error.is_some_and(|value| {
                    // 验证安全错误形状与固定码。
                    is_safe_error(value)
                        && value.get("code").and_then(Value::as_str) == Some("OUTCOME_UNKNOWN")
                })
        }
    };
    // 非法组合失败闭合。
    if !valid {
        // 返回协议失败。
        return Err(protocol_failed());
    }
    // final 合法。
    Ok(())
}

// 判断字符串是否为 canonical nonce。
fn is_nonce(value: &str) -> bool {
    // 长度和小写十六进制必须逐字成立。
    value.len() == NONCE_LENGTH
        // 遍历字节。
        && value
            // 取得字节迭代器。
            .bytes()
            // 只接受小写十六进制。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 判断值是否为带固定类型前缀的 nonce。
fn is_prefixed_nonce(value: &str, prefix: &str) -> bool {
    // 移除前缀后验证 nonce。
    value.strip_prefix(prefix).is_some_and(is_nonce)
}

// 判断 worker 私有 page ref。
fn is_page_ref(value: &str) -> bool {
    // 只接受固定前缀。
    is_prefixed_nonce(value, PAGE_REF_PREFIX)
}

// 判断 worker 私有 element ref。
fn is_element_ref(value: &str) -> bool {
    // 只接受固定前缀。
    is_prefixed_nonce(value, ELEMENT_REF_PREFIX)
}

// 判断帧是否关联预期命令。
fn correlates(contract_version: &str, request_nonce: &str, expected_nonce: &str) -> bool {
    // 版本和 nonce 必须逐字匹配。
    contract_version == CONTRACT_VERSION
        // 请求 nonce 匹配。
        && request_nonce == expected_nonce
        // nonce 自身 canonical。
        && is_nonce(request_nonce)
}

// 构造参数失败。
const fn invalid_argument() -> BrowserPageProtocolFailure {
    // 使用固定类别。
    BrowserPageProtocolFailure::new(BrowserPageProtocolErrorCode::InvalidArgument)
}

// 构造协议状态失败。
const fn protocol_failed() -> BrowserPageProtocolFailure {
    // 使用固定类别。
    BrowserPageProtocolFailure::new(BrowserPageProtocolErrorCode::ProtocolFailed)
}

// 加载同 Component 的协议测试。
#[cfg(test)]
#[path = "browser_page_protocol_tests.rs"]
mod tests;
