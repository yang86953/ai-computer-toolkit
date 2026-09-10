//! 定义长操作 broker 的封闭、版本化 JSON 协议。

// 导入协议序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 canonical opaque 目标原语。
use super::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

// 固定同会话长操作 broker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/long-operation-broker/v1";
// 固定请求关联 nonce 的小写十六进制长度。
const REQUEST_NONCE_LENGTH: usize = 32;
// 冻结首个可提交的长操作 capability。
const WINDOW_RECORD_CAPABILITY: &str = "window.record@1";

// 表示 broker 接受的封闭 Command 与 Query 集合。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用 kebab-case 对齐 JSON 协议。
#[serde(rename_all = "kebab-case")]
pub(crate) enum LongOperationBrokerAction {
    // 提交一个经过确认的长操作 Command。
    Submit,
    // 查询一个既有任务句柄的当前事实。
    Status,
    // 请求取消一个既有任务句柄。
    Cancel,
}

// 表示协议解析阶段的封闭失败码。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongOperationProtocolErrorCode {
    // 表示 JSON、版本、nonce 或字段组合不合法。
    InvalidArgument,
    // 表示提交命令尚未经过显式确认。
    ConfirmationRequired,
}

// 为协议失败码提供稳定公开文本。
impl LongOperationProtocolErrorCode {
    // 返回版本化稳定错误码。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举封闭协议错误集合。
        match self {
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射确认拒绝。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
        }
    }
}

// 表示协议边界拒绝一条 frame 的安全事实。
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LongOperationProtocolFailure {
    // 保存封闭失败码。
    code: LongOperationProtocolErrorCode,
    // 区分固定 envelope 是否已经接受。
    transport_accepted: bool,
    // 只在固定 envelope 已接受后保存已经验证的请求 nonce。
    request_nonce: Option<String>,
}

// 为协议失败提供只读投影。
impl LongOperationProtocolFailure {
    // 构造尚未接受固定 envelope 的失败。
    const fn envelope(code: LongOperationProtocolErrorCode) -> Self {
        // 返回不回显任何不可信输入的失败。
        Self {
            // 保存封闭失败码。
            code,
            // 标记 frame 未被接受。
            transport_accepted: false,
            // 未接受 envelope 时禁止回显任何关联值。
            request_nonce: None,
        }
    }

    // 构造 envelope 已接受后的语义拒绝。
    const fn accepted(code: LongOperationProtocolErrorCode) -> Self {
        // 返回不携带目标、路径或 payload 的失败。
        Self {
            // 保存封闭失败码。
            code,
            // 标记固定 transport frame 已接受。
            transport_accepted: true,
            // 由顶层 parser 在 envelope 验证后绑定 nonce。
            request_nonce: None,
        }
    }

    // 返回封闭失败码。
    pub(crate) const fn code(&self) -> LongOperationProtocolErrorCode {
        // 复制无状态枚举值。
        self.code
    }

    // 返回 transport frame 接受事实。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 只公开封闭布尔值。
        self.transport_accepted
    }

    // 返回仅在 transport 已接受后可回显的 canonical nonce。
    pub(crate) fn request_nonce(&self) -> Option<&str> {
        // 借用已经验证的关联值。
        self.request_nonce.as_deref()
    }
}

// 表示 broker 接受的一条严格请求。
#[derive(Debug, Deserialize, Serialize)]
// 固定 camelCase 字段并拒绝任意 envelope 扩展。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LongOperationBrokerRequest {
    // 保存固定协议版本。
    contract_version: String,
    // 保存一次性请求关联值。
    request_nonce: String,
    // 保存封闭 Command 或 Query。
    action: LongOperationBrokerAction,
    // status 与 cancel 使用 canonical operation handle。
    operation_id: Option<String>,
    // submit 使用固定 capability ID。
    capability_id: Option<String>,
    // submit 使用 provider-neutral 精确目标对象。
    target: Option<Value>,
    // submit 使用 capability 自有的封闭输入对象。
    input: Option<Value>,
    // submit 必须携带显式确认。
    confirmed: Option<bool>,
}

// 为 broker 请求提供严格解析和只读事实。
impl LongOperationBrokerRequest {
    // 构造经过字段门禁的窗口录制 submit Command。
    pub(crate) fn submit_window_record(
        // 接收系统随机 canonical nonce。
        request_nonce: String,
        // 接收 canonical opaque 窗口目标。
        session_id: String,
        // 接收完整 provider-neutral 录制输入。
        input: Value,
        // 接收逐操作显式确认。
        confirmed: bool,
    ) -> Option<Self> {
        // 构造字段封闭请求。
        let request = Self {
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 保存一次性关联值。
            request_nonce,
            // 固定选择 submit Command。
            action: LongOperationBrokerAction::Submit,
            // 新提交不得预先携带 operation handle。
            operation_id: None,
            // 固定首个异步 capability。
            capability_id: Some(WINDOW_RECORD_CAPABILITY.to_owned()),
            // 只构造单字段 canonical 窗口目标。
            target: Some(serde_json::json!({"sessionId": session_id})),
            // 保存完整领域输入对象。
            input: Some(input),
            // 保存逐操作确认事实。
            confirmed: Some(confirmed),
        };
        // 生产 client 必须通过与 broker 相同的字段门禁。
        request.validate_action_fields().ok().map(|()| request)
    }

    // 构造只包含 canonical operation handle 的 status Query。
    pub(crate) fn status(request_nonce: String, operation_id: String) -> Option<Self> {
        // 复用同一个封闭 handle-only 构造入口。
        Self::handle_request(
            request_nonce,
            LongOperationBrokerAction::Status,
            operation_id,
        )
    }

    // 构造只包含 canonical operation handle 的 cancel Command。
    pub(crate) fn cancel(request_nonce: String, operation_id: String) -> Option<Self> {
        // 复用同一个封闭 handle-only 构造入口。
        Self::handle_request(
            request_nonce,
            LongOperationBrokerAction::Cancel,
            operation_id,
        )
    }

    // 构造并再次验证客户端生成的 handle-only 请求。
    fn handle_request(
        // 接收系统随机 canonical nonce。
        request_nonce: String,
        // 接收封闭 action。
        action: LongOperationBrokerAction,
        // 接收公开 opaque operation handle。
        operation_id: String,
    ) -> Option<Self> {
        // 构造字段封闭请求。
        let request = Self {
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 保存一次性关联值。
            request_nonce,
            // 保存 status 或 cancel。
            action,
            // 保存唯一 handle。
            operation_id: Some(operation_id),
            // handle-only 请求不携带 capability。
            capability_id: None,
            // handle-only 请求不携带目标。
            target: None,
            // handle-only 请求不携带输入。
            input: None,
            // handle-only 请求不携带确认。
            confirmed: None,
        };
        // 生产 client 也必须通过与 broker 相同的字段门禁。
        request.validate_action_fields().ok().map(|()| request)
    }

    // 严格解析一条完整 JSON frame。
    pub(crate) fn parse(text: &str) -> Result<Self, LongOperationProtocolFailure> {
        // 兼容唯一 UTF-8 BOM 并去除外围空白。
        let normalized = text.trim_start_matches('\u{feff}').trim();
        // 只接受字段封闭的单个 JSON 对象。
        let request = serde_json::from_str::<Self>(normalized).map_err(|_| {
            // JSON 或字段形状失败时不得回显输入。
            LongOperationProtocolFailure::envelope(
                // 使用稳定参数错误。
                LongOperationProtocolErrorCode::InvalidArgument,
            )
        })?;
        // 固定版本不得协商或猜测。
        if request.contract_version != CONTRACT_VERSION
            // 请求 nonce 必须 canonical。
            || !is_canonical_request_nonce(&request.request_nonce)
        {
            // envelope 未完成时不接受关联值。
            return Err(LongOperationProtocolFailure::envelope(
                // 使用稳定参数错误。
                LongOperationProtocolErrorCode::InvalidArgument,
            ));
        }
        // envelope 接受后验证 action 专属字段。
        if let Err(mut failure) = request.validate_action_fields() {
            // 只有 transport 已接受的失败可以绑定 canonical nonce。
            failure.request_nonce = Some(request.request_nonce.clone());
            // 返回不携带 target 或 input 的失败。
            return Err(failure);
        }
        // 返回尚未触碰 registry、目标或 worker 的请求。
        Ok(request)
    }

    // 验证 action 与可选字段形成唯一合法组合。
    fn validate_action_fields(&self) -> Result<(), LongOperationProtocolFailure> {
        // 按封闭 action 选择互斥字段规则。
        match self.action {
            // submit 必须先验证确认，再解析目标与输入。
            LongOperationBrokerAction::Submit => self.validate_submit(),
            // status 只允许 operation handle。
            LongOperationBrokerAction::Status => self.validate_handle_query(),
            // cancel 复用同一 handle-only 约束。
            LongOperationBrokerAction::Cancel => self.validate_handle_query(),
        }
    }

    // 验证确认后的固定窗口录制提交形状。
    fn validate_submit(&self) -> Result<(), LongOperationProtocolFailure> {
        // 确认必须先于 target、input 与路径语义解析。
        if self.confirmed != Some(true) {
            // 缺少确认时返回独立稳定拒绝。
            return Err(LongOperationProtocolFailure::accepted(
                // 使用确认错误码。
                LongOperationProtocolErrorCode::ConfirmationRequired,
            ));
        }
        // submit 不得携带既有 operation handle。
        if self.operation_id.is_some()
            // 首个协议版本只允许窗口录制。
            || self.capability_id.as_deref() != Some(WINDOW_RECORD_CAPABILITY)
            // target 必须是单字段 canonical 窗口对象。
            || !is_canonical_window_target(self.target.as_ref())
            // input 必须保留为领域 Module 后续验证的对象。
            || !matches!(self.input.as_ref(), Some(Value::Object(_)))
        {
            // 拒绝宽松字段组合。
            return Err(LongOperationProtocolFailure::accepted(
                // 使用稳定参数错误。
                LongOperationProtocolErrorCode::InvalidArgument,
            ));
        }
        // 返回不启动 worker 的纯协议成功。
        Ok(())
    }

    // 验证 status 与 cancel 的 handle-only 形状。
    fn validate_handle_query(&self) -> Result<(), LongOperationProtocolFailure> {
        // 解析 canonical operation handle。
        let operation = self
            // 借用请求中的 handle。
            .operation_id
            // 缺失 handle 时拒绝。
            .as_deref()
            // 严格解析 s2 外壳。
            .and_then(OpaqueTargetId::parse);
        // 只接受 operation 类别且拒绝 submit 专属字段。
        if operation.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Operation)
            // Query 不得携带 capability。
            || self.capability_id.is_some()
            // Query 不得携带目标。
            || self.target.is_some()
            // Query 不得携带输入。
            || self.input.is_some()
            // Query 不得携带确认。
            || self.confirmed.is_some()
        {
            // 返回 envelope 已接受后的参数拒绝。
            return Err(LongOperationProtocolFailure::accepted(
                // 使用稳定参数错误。
                LongOperationProtocolErrorCode::InvalidArgument,
            ));
        }
        // 返回纯 handle Query 成功。
        Ok(())
    }

    // 返回固定协议版本。
    pub(crate) fn contract_version(&self) -> &str {
        // 借用已验证版本文本。
        &self.contract_version
    }

    // 返回已验证请求关联值。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用 canonical nonce。
        &self.request_nonce
    }

    // 返回封闭 action。
    pub(crate) const fn action(&self) -> LongOperationBrokerAction {
        // 复制无状态枚举值。
        self.action
    }

    // 返回可选 operation handle。
    pub(crate) fn operation_id(&self) -> Option<&str> {
        // 仅投影已经验证的 canonical handle。
        self.operation_id.as_deref()
    }

    // 返回 submit capability。
    pub(crate) fn capability_id(&self) -> Option<&str> {
        // 仅投影冻结 capability。
        self.capability_id.as_deref()
    }

    // 返回 submit 精确目标对象。
    pub(crate) const fn target(&self) -> Option<&Value> {
        // 借用已经验证的目标对象。
        self.target.as_ref()
    }

    // 返回 submit 领域输入对象。
    pub(crate) const fn input(&self) -> Option<&Value> {
        // 借用尚未执行领域解析的输入。
        self.input.as_ref()
    }
}

// 验证不携带平台事实的一次性请求关联值。
fn is_canonical_request_nonce(value: &str) -> bool {
    // 固定长度并只接受小写十六进制。
    value.len() == REQUEST_NONCE_LENGTH
        // 检查每个 ASCII 字节。
        && value
            // 遍历关联值字节。
            .bytes()
            // 拒绝大写、非十六进制与多字节字符。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 验证 submit 使用单字段 canonical 窗口目标。
fn is_canonical_window_target(target: Option<&Value>) -> bool {
    // 目标必须是 JSON 对象。
    let Some(object) = target.and_then(Value::as_object) else {
        // 拒绝缺失或非对象目标。
        return false;
    };
    // 只允许 sessionId 字段。
    if object.len() != 1 {
        // 拒绝 native handle、PID 或 provider 扩展。
        return false;
    }
    // 严格解析 canonical target。
    let target = object
        // 读取唯一公开目标字段。
        .get("sessionId")
        // 目标必须是字符串。
        .and_then(Value::as_str)
        // 解析版本化 opaque ID。
        .and_then(OpaqueTargetId::parse);
    // 只接受窗口类别。
    target.map(OpaqueTargetId::kind) == Some(OpaqueTargetKind::Window)
}

// 声明纯协议 Component 的回归测试。
#[cfg(test)]
// 将协议矩阵保存在独立文件以控制生产文件规模。
#[path = "long_operation_protocol_tests.rs"]
mod tests;
