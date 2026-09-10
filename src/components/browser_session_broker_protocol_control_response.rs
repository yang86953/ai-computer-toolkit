//! 定义 cancel receipt 与 cancel-rejected 的严格响应投影。

// 导入严格 cancel request 与 identity 长度。
use super::super::{BrowserSessionBrokerCancellationRequest, NONCE_LENGTH};
// 导入当前 live broker epoch。
use super::super::state::BrowserSessionBrokerEpoch;
// 导入封闭 cancel receipt 状态。
use super::super::state::BrowserSessionBrokerCancelStatus;

// 保存 cancel receipt。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerCancelReceipt {
    // 保存 cancel request nonce。
    cancel_nonce: String,
    // 保存目标 request nonce。
    request_nonce: String,
    // 保存当前 broker epoch。
    epoch: String,
    // 保存单调 cancel revision。
    revision: u64,
    // 保存封闭四态结果。
    status: BrowserSessionBrokerCancelStatus,
}

// 为 receipt 提供构造和访问。
impl BrowserSessionBrokerCancelReceipt {
    // 用当前 epoch 回显 cancel 处理结果。
    pub(crate) fn new(
        // 绑定严格 cancel request。
        cancel: &BrowserSessionBrokerCancellationRequest,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收当前 revision。
        revision: u64,
        // 接收封闭状态。
        status: BrowserSessionBrokerCancelStatus,
    ) -> Self {
        // 从严格输入派生全部关联字段。
        Self {
            // 复制 cancel nonce。
            cancel_nonce: cancel.cancel_request_nonce().to_owned(),
            // 复制目标 nonce。
            request_nonce: cancel.request_nonce().to_owned(),
            // 复制当前 epoch。
            epoch: epoch.as_str().to_owned(),
            // 保存 revision。
            revision,
            // 保存状态。
            status,
        }
    }

    // 从严格 wire 字段恢复 cancel receipt。
    pub(in super::super) fn from_wire(
        // 接收 canonical cancel nonce。
        cancel_nonce: String,
        // 接收 canonical 目标 nonce。
        request_nonce: String,
        // 接收 canonical broker epoch。
        epoch: String,
        // 接收单调 revision。
        revision: u64,
        // 接收封闭状态。
        status: BrowserSessionBrokerCancelStatus,
    ) -> Option<Self> {
        // 所有关联 identity 必须先通过 canonical 形状校验。
        (canonical_hex(&cancel_nonce)
            // target nonce 也必须 canonical。
            && canonical_hex(&request_nonce)
            // epoch 也必须 canonical。
            && canonical_hex(&epoch))
        // 只在全部字段安全时恢复投影。
        .then_some(Self {
            // 保存 canonical cancel nonce。
            cancel_nonce,
            // 保存 canonical 目标 nonce。
            request_nonce,
            // 保存 canonical epoch。
            epoch,
            // 保存 revision。
            revision,
            // 保存封闭状态。
            status,
        })
    }

    // 返回当前 epoch。
    pub(crate) fn broker_epoch(&self) -> &str {
        // 借用 epoch。
        &self.epoch
    }

    // 返回四态取消结果。
    pub(crate) const fn status(&self) -> BrowserSessionBrokerCancelStatus {
        // 复制状态。
        self.status
    }

    // 返回 cancel nonce。
    pub(crate) fn cancel_request_nonce(&self) -> &str {
        // 借用 cancel nonce。
        &self.cancel_nonce
    }

    // 返回目标 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用目标 nonce。
        &self.request_nonce
    }

    // 返回单调 revision。
    pub(crate) const fn cancel_revision(&self) -> u64 {
        // 复制 revision。
        self.revision
    }

    // 判断 receipt revision/status 是否属于冻结 wire 矩阵。
    pub(crate) const fn valid_wire_revision(&self) -> bool {
        // rev0 允许四种初态，rev1 只允许两个业务终态。
        self.revision == 0
            // rev1 只能表达 cancelled 或 too-late。
            || (self.revision == 1
                // 检查封闭 terminal status。
                && matches!(
                    // 读取 receipt status。
                    self.status,
                    // 允许 cancelled。
                    BrowserSessionBrokerCancelStatus::Cancelled
                        // 允许 too-late。
                        | BrowserSessionBrokerCancelStatus::TooLate
                ))
    }
}

// 保存 cancel business-before 拒绝投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerCancelRejected {
    // 保存 cancel request nonce。
    cancel_nonce: String,
    // 保存目标 request nonce。
    request_nonce: String,
    // 保存当前 broker epoch。
    epoch: String,
    // 保存封闭错误码。
    error_code: String,
    // 保存已经验证的安全错误说明。
    error_message: String,
}

// 为 cancel-rejected 提供封闭构造。
impl BrowserSessionBrokerCancelRejected {
    // 只构造 schema 允许的三种拒绝。
    pub(crate) fn new(
        // 绑定严格 cancel request。
        cancel: &BrowserSessionBrokerCancellationRequest,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收封闭拒绝码。
        error_code: &str,
    ) -> Option<Self> {
        // stale rejection 必须回显另一个 current epoch。
        let epoch_valid = if error_code == "STALE_BROKER_EPOCH" {
            // current epoch 必须不同于旧 expected epoch。
            epoch.as_str() != cancel.expected_broker_epoch()
        } else {
            // 其余拒绝必须留在 cancel expected epoch。
            epoch.as_str() == cancel.expected_broker_epoch()
        };
        // 拒绝 epoch 语义不成立的投影。
        if !epoch_valid {
            // 不允许构造会串 epoch 的 control response。
            return None;
        }
        // 使用安全固定说明构造拒绝。
        Self::from_wire(
            // 复制 cancel nonce。
            cancel.cancel_request_nonce().to_owned(),
            // 复制目标 nonce。
            cancel.request_nonce().to_owned(),
            // 复制当前 epoch。
            epoch.as_str().to_owned(),
            // 复制稳定错误码。
            error_code.to_owned(),
            // 生成不泄漏 provider 信息的默认说明。
            default_error_message(error_code),
        )
    }

    // 从严格 wire 字段恢复 cancel-rejected。
    pub(in super::super) fn from_wire(
        // 接收 canonical cancel nonce。
        cancel_nonce: String,
        // 接收 canonical 目标 nonce。
        request_nonce: String,
        // 接收 canonical broker epoch。
        epoch: String,
        // 接收封闭拒绝码。
        error_code: String,
        // 接收经解码器验证的安全说明。
        error_message: String,
    ) -> Option<Self> {
        // 拒绝码必须属于 cancel 专用白名单。
        let allowed = matches!(
            // 读取错误码文本。
            error_code.as_str(),
            // 允许 stale epoch。
            "STALE_BROKER_EPOCH"
                // 允许 nonce semantic conflict。
                | "NONCE_SEMANTIC_CONFLICT"
                // 允许 request ledger full。
                | "BROKER_REQUEST_LEDGER_FULL"
        );
        // 关联 identity、错误码和说明必须全部 canonical。
        (canonical_hex(&cancel_nonce)
            // target nonce 也必须 canonical。
            && canonical_hex(&request_nonce)
            // epoch 也必须 canonical。
            && canonical_hex(&epoch)
            // error code 必须在白名单。
            && allowed
            // safeError 字段必须通过边界验证。
            && safe_error(&error_code, &error_message))
        // 只在全部不变量成立时恢复拒绝。
        .then_some(Self {
            // 保存 cancel nonce。
            cancel_nonce,
            // 保存目标 nonce。
            request_nonce,
            // 保存 epoch。
            epoch,
            // 保存错误码。
            error_code,
            // 保存安全说明。
            error_message,
        })
    }

    // 返回 cancel nonce。
    pub(crate) fn cancel_request_nonce(&self) -> &str {
        // 借用 cancel nonce。
        &self.cancel_nonce
    }

    // 返回目标 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用目标 nonce。
        &self.request_nonce
    }

    // 返回当前 epoch。
    pub(crate) fn broker_epoch(&self) -> &str {
        // 借用 epoch。
        &self.epoch
    }

    // 返回封闭错误码。
    pub(crate) fn error_code(&self) -> &str {
        // 借用错误码。
        &self.error_code
    }

    // 返回安全错误说明。
    pub(crate) fn error_message(&self) -> &str {
        // 借用错误说明。
        &self.error_message
    }
}

// 验证 nonce 或 epoch 的固定小写十六进制形状。
fn canonical_hex(value: &str) -> bool {
    // 同时校验精确长度与字符集合。
    value.len() == NONCE_LENGTH
        // 逐字节拒绝大写或非十六进制字符。
        && value.bytes().all(|byte| {
            // 允许十进制数字。
            byte.is_ascii_digit()
                // 允许小写 a 到 f。
                || (b'a'..=b'f').contains(&byte)
        })
}

// 验证 safeError 的冻结字段边界。
fn safe_error(code: &str, message: &str) -> bool {
    // 错误码必须为一到六十四字节。
    let code_length_valid = (1..=64).contains(&code.len());
    // 首字符必须是大写 ASCII 字母。
    let first_valid = code.as_bytes().first().is_some_and(u8::is_ascii_uppercase);
    // 其余字符只允许大写字母、数字或下划线。
    let tail_valid = code
        // 跳过已单独检查的首字符。
        .bytes()
        // 从第二个字节开始检查。
        .skip(1)
        // 拒绝任何非 canonical 字节。
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    // JSON Schema 的长度按 Unicode scalar 数量近似执行。
    let message_length_valid = (1..=512).contains(&message.chars().count());
    // 同时满足错误码与说明边界。
    code_length_valid && first_valid && tail_valid && message_length_valid
}

// 生成不泄漏 target/provider 信息的固定说明。
fn default_error_message(error_code: &str) -> String {
    // 仅回显已经封闭的稳定错误码。
    format!("Browser session broker rejected the request with {error_code}.")
}
