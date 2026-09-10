//! 实现 sequence step worker accepted 与 final 帧状态机。

// 导入严格帧序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入父协议拥有的版本、边界、失败和 nonce 原语。
use super::{
    // 导入固定协议版本。
    CONTRACT_VERSION,
    // 导入 stdout 总字节上限。
    MAXIMUM_OUTPUT_BYTES,
    // 导入协议错误类别。
    SequenceStepProtocolErrorCode,
    // 导入无负载协议失败。
    SequenceStepProtocolFailure,
    // 导入 canonical nonce 判断。
    is_canonical_nonce,
    // 导入 canonical nonce 验证。
    validate_nonce,
};

// 表示 final 帧允许公开的封闭步骤结果类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用稳定 kebab-case 文本。
#[serde(rename_all = "kebab-case")]
pub(crate) enum SequenceStepWorkerOutcome {
    // 表示 provider 尚未 dispatch。
    NotDispatched,
    // 表示 provider 已返回确定成功。
    Completed,
    // 表示 provider 已返回确定失败。
    Failed,
    // 表示 mutation 可能已接受但终态不可靠。
    Unknown,
}

// 表示 worker stdout 允许出现的两类帧。
#[derive(Debug, Deserialize, Serialize)]
// 使用 kind 内部标签并拒绝未知字段。
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SequenceStepWorkerFrame {
    // 表示 System 已完成门禁且即将调用 provider。
    DispatchAccepted {
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
    // 表示 worker 建立了唯一最终业务事实。
    Final {
        // 保存固定协议版本。
        #[serde(rename = "contractVersion")]
        contract_version: String,
        // 保存请求关联值。
        #[serde(rename = "requestNonce")]
        request_nonce: String,
        // 区分 provider 是否已经 dispatch。
        #[serde(rename = "dispatchAccepted")]
        dispatch_accepted: bool,
        // 区分是否建立确定终态。
        completed: bool,
        // 保存封闭结果类别。
        outcome: SequenceStepWorkerOutcome,
        // 明确禁止未知或已接受 mutation 自动重试。
        #[serde(rename = "retrySafe")]
        retry_safe: bool,
        // 明确 dispatch 后 provider 可能已经接受操作。
        #[serde(rename = "acceptedMayHaveOccurred")]
        accepted_may_have_occurred: bool,
        // 仅确定成功携带完整结果。
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        // 拒绝、失败或未知结果携带稳定错误对象。
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
    },
}

// 保存从零至两帧 stdout 建立的最小可靠观察。
#[derive(Debug)]
pub(crate) struct SequenceStepFrameObservation {
    // 保存是否看到合法 accepted 帧。
    dispatch_accepted: bool,
    // 保存可选最终结果。
    final_observation: Option<SequenceStepFinalObservation>,
}

// 保存一个已经通过帧状态机核验的 final 结果。
#[derive(Debug)]
pub(crate) struct SequenceStepFinalObservation {
    // 保存封闭步骤结果类别。
    outcome: SequenceStepWorkerOutcome,
    // 保存确定终态事实。
    completed: bool,
    // 保存协议证明的重试安全性。
    retry_safe: bool,
    // 保存 provider 可能已经接受操作的事实。
    accepted_may_have_occurred: bool,
    // 保存可选完整结果。
    result: Option<Value>,
    // 保存可选稳定错误对象。
    error: Option<Value>,
}

// 为 stdout 观察提供只读投影。
impl SequenceStepFrameObservation {
    // 返回是否已经建立 dispatch accepted 事实。
    pub(crate) const fn dispatch_accepted(&self) -> bool {
        // 复制封闭布尔事实。
        self.dispatch_accepted
    }

    // 返回可选最终观察。
    pub(crate) const fn final_observation(&self) -> Option<&SequenceStepFinalObservation> {
        // 借用已经验证的 final 结果。
        self.final_observation.as_ref()
    }
}

// 为 final 观察提供只读投影。
impl SequenceStepFinalObservation {
    // 返回封闭结果类别。
    pub(crate) const fn outcome(&self) -> SequenceStepWorkerOutcome {
        // 复制无状态枚举。
        self.outcome
    }

    // 返回是否建立确定终态。
    pub(crate) const fn completed(&self) -> bool {
        // 复制封闭布尔事实。
        self.completed
    }

    // 返回调用方是否可以自动重试。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制协议证明的重试事实。
        self.retry_safe
    }

    // 返回 provider 是否可能已经接受操作。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制封闭接受事实。
        self.accepted_may_have_occurred
    }

    // 返回可选完整结果。
    pub(crate) const fn result(&self) -> Option<&Value> {
        // 借用 provider-neutral 结果。
        self.result.as_ref()
    }

    // 返回可选稳定错误对象。
    pub(crate) const fn error(&self) -> Option<&Value> {
        // 借用 provider-neutral 错误。
        self.error.as_ref()
    }
}

// 聚合 final 构造与验证所需的封闭字段。
struct SequenceStepFinalFields {
    // 保存 dispatch 事实。
    dispatch_accepted: bool,
    // 保存确定完成事实。
    completed: bool,
    // 保存封闭结果类别。
    outcome: SequenceStepWorkerOutcome,
    // 保存重试安全事实。
    retry_safe: bool,
    // 保存可能已经接受事实。
    accepted_may_have_occurred: bool,
    // 保存可选完整结果。
    result: Option<Value>,
    // 保存可选稳定错误。
    error: Option<Value>,
}

// 构造 System 门禁完成后的 accepted 帧。
pub(crate) fn dispatch_accepted_frame(
    // 接收已验证请求 nonce。
    request_nonce: &str,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 拒绝无法关联的 accepted 帧。
    validate_nonce(request_nonce)?;
    // 构造固定 accepted 事实。
    frame_to_line(&SequenceStepWorkerFrame::DispatchAccepted {
        // 固定协议版本。
        contract_version: CONTRACT_VERSION.to_owned(),
        // 复制很小的关联值。
        request_nonce: request_nonce.to_owned(),
        // accepted 帧必须声明已接受。
        dispatch_accepted: true,
        // accepted 帧不得宣称完成。
        completed: false,
    })
}

// 构造 provider dispatch 前的最终拒绝帧。
pub(crate) fn not_dispatched_frame(
    // 接收已验证请求 nonce。
    request_nonce: &str,
    // 接收稳定公开错误对象。
    error: Value,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 构造唯一 dispatch 前 final 状态。
    final_frame(
        // 保留请求关联。
        request_nonce,
        // 聚合唯一 dispatch 前状态。
        SequenceStepFinalFields {
            // provider 未接受。
            dispatch_accepted: false,
            // provider 步骤没有开始或完成。
            completed: false,
            // 使用未 dispatch 类别。
            outcome: SequenceStepWorkerOutcome::NotDispatched,
            // 修正请求后可以重试。
            retry_safe: true,
            // provider 不可能已经接受。
            accepted_may_have_occurred: false,
            // 拒绝不携带成功结果。
            result: None,
            // 携带稳定错误。
            error: Some(error),
        },
    )
}

// 构造 provider 确定成功的 final 帧。
pub(crate) fn completed_frame(
    // 接收已验证请求 nonce。
    request_nonce: &str,
    // 接收经过 System 证明的完整结果。
    result: Value,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 构造唯一成功状态。
    final_frame(
        // 保留请求关联。
        request_nonce,
        // 聚合唯一确定成功状态。
        SequenceStepFinalFields {
            // provider 已接受。
            dispatch_accepted: true,
            // 已建立确定成功。
            completed: true,
            // 使用完成类别。
            outcome: SequenceStepWorkerOutcome::Completed,
            // 已完成操作不应自动重复。
            retry_safe: false,
            // provider 已经接受。
            accepted_may_have_occurred: true,
            // 携带完整结果。
            result: Some(result),
            // 成功不携带错误。
            error: None,
        },
    )
}

// 构造 provider 已 dispatch 后的确定失败或未知 final 帧。
pub(crate) fn failed_frame(
    // 接收已验证请求 nonce。
    request_nonce: &str,
    // 接收稳定公开错误对象。
    error: Value,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 从统一错误对象读取稳定错误码。
    let outcome = if error
        // 只读取公开错误码。
        .get("code")
        // 只接受字符串。
        .and_then(Value::as_str)
        // OutcomeUnknown 保持独立类别。
        == Some("OUTCOME_UNKNOWN")
    {
        // 保留未知终态。
        SequenceStepWorkerOutcome::Unknown
    } else {
        // 其他 provider 错误是确定失败。
        SequenceStepWorkerOutcome::Failed
    };
    // 未知结果不宣称已经建立完成事实。
    let completed = outcome != SequenceStepWorkerOutcome::Unknown;
    // 构造 dispatch 后 final 状态。
    final_frame(
        // 保留请求关联。
        request_nonce,
        // 聚合 dispatch 后失败状态。
        SequenceStepFinalFields {
            // provider 已接受。
            dispatch_accepted: true,
            // 按错误类别保存确定性。
            completed,
            // 保存确定失败或未知结果。
            outcome,
            // generic dispatch 后错误一律禁止自动重试。
            retry_safe: false,
            // provider 可能已经接受 mutation。
            accepted_may_have_occurred: true,
            // 失败不携带成功结果。
            result: None,
            // 携带稳定错误。
            error: Some(error),
        },
    )
}

// 严格解析 worker 的完整或部分 stdout 帧序列。
pub(crate) fn parse_frame_log(
    // 接收零至两行 worker stdout。
    text: &str,
    // 接收当前请求 nonce。
    expected_nonce: &str,
    // 允许 Job 终止后只有 accepted 或零帧。
    allow_partial: bool,
) -> Result<SequenceStepFrameObservation, SequenceStepProtocolFailure> {
    // 在解析任何字段前拒绝无界 worker 输出。
    if text.len() > MAXIMUM_OUTPUT_BYTES {
        // 返回输出资源错误。
        return Err(SequenceStepProtocolFailure::new(
            // 使用封闭输出过大类别。
            SequenceStepProtocolErrorCode::OutputTooLarge,
        ));
    }
    // 当前请求 nonce 本身必须可信。
    validate_nonce(expected_nonce)?;
    // 只移除末尾 JSON Lines 换行。
    let normalized = text.trim_end_matches(['\r', '\n']);
    // Job 在 dispatch 前终止时允许零帧观察。
    if normalized.is_empty() {
        // 完整退出必须提供 final 帧。
        if !allow_partial {
            // 空完整输出违反 worker 协议。
            return Err(SequenceStepProtocolFailure::new(
                // 使用协议状态错误。
                SequenceStepProtocolErrorCode::ProtocolFailed,
            ));
        }
        // 返回可靠的未 accepted 且无 final 观察。
        return Ok(SequenceStepFrameObservation {
            // 没有 accepted 事实。
            dispatch_accepted: false,
            // 没有 final 事实。
            final_observation: None,
        });
    }
    // 分割最多两帧并拒绝空中间行。
    let lines = normalized.lines().collect::<Vec<_>>();
    // accepted 加 final 是唯一最大帧数。
    if lines.len() > 2 || lines.iter().any(|line| line.trim().is_empty()) {
        // 拒绝重复、空行或额外帧。
        return Err(SequenceStepProtocolFailure::new(
            // 使用协议状态错误。
            SequenceStepProtocolErrorCode::ProtocolFailed,
        ));
    }
    // 初始没有 accepted 事实。
    let mut dispatch_accepted = false;
    // 初始没有 final 事实。
    let mut final_observation = None;
    // 按输出顺序验证每一帧。
    for line in lines {
        // 严格解析单帧对象。
        let frame = serde_json::from_str::<SequenceStepWorkerFrame>(line).map_err(|_| {
            // 不回显 worker 输出。
            SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::ProtocolFailed)
        })?;
        // 按 accepted 或 final 更新封闭状态机。
        match frame {
            // 处理唯一 accepted 帧。
            SequenceStepWorkerFrame::DispatchAccepted {
                // 读取协议版本。
                contract_version,
                // 读取关联值。
                request_nonce,
                // 读取必须为真的 accepted 标志。
                dispatch_accepted: accepted,
                // 读取必须为假的完成标志。
                completed,
            } => {
                // accepted 只能是首帧且字段必须保持常量。
                if dispatch_accepted
                    // final 后也不能再 accepted。
                    || final_observation.is_some()
                    // accepted 标志不得为假。
                    || !accepted
                    // accepted 帧不得宣称完成。
                    || completed
                    // 核对协议版本与 nonce。
                    || !frame_identity_matches(
                        // 传递帧版本。
                        &contract_version,
                        // 传递帧 nonce。
                        &request_nonce,
                        // 传递当前请求 nonce。
                        expected_nonce,
                    )
                {
                    // 拒绝重复、乱序或关联漂移。
                    return Err(SequenceStepProtocolFailure::new(
                        // 使用协议状态错误。
                        SequenceStepProtocolErrorCode::ProtocolFailed,
                    ));
                }
                // 建立唯一 accepted 事实。
                dispatch_accepted = true;
            }
            // 处理唯一 final 帧。
            SequenceStepWorkerFrame::Final {
                // 读取协议版本。
                contract_version,
                // 读取关联值。
                request_nonce,
                // 读取 dispatch 事实。
                dispatch_accepted: final_accepted,
                // 读取完成事实。
                completed,
                // 读取结果类别。
                outcome,
                // 读取重试事实。
                retry_safe,
                // 读取可能接受事实。
                accepted_may_have_occurred,
                // 读取可选结果。
                result,
                // 读取可选错误。
                error,
            } => {
                // final 只能出现一次且关联必须匹配。
                if final_observation.is_some()
                    // 核对协议版本与 nonce。
                    || !frame_identity_matches(
                        // 传递帧版本。
                        &contract_version,
                        // 传递帧 nonce。
                        &request_nonce,
                        // 传递当前请求 nonce。
                        expected_nonce,
                    )
                    // final 的 accepted 事实必须与前序帧一致。
                    || final_accepted != dispatch_accepted
                    // 核对 final 字段组合。
                    || !valid_final_shape(
                        // 传递 dispatch 事实。
                        final_accepted,
                        // 传递完成事实。
                        completed,
                        // 传递结果类别。
                        outcome,
                        // 传递重试事实。
                        retry_safe,
                        // 传递可能接受事实。
                        accepted_may_have_occurred,
                        // 借用结果。
                        result.as_ref(),
                        // 借用错误。
                        error.as_ref(),
                    )
                {
                    // 拒绝状态漂移或非法结果形状。
                    return Err(SequenceStepProtocolFailure::new(
                        // 使用协议状态错误。
                        SequenceStepProtocolErrorCode::ProtocolFailed,
                    ));
                }
                // 保存已经验证的 final 观察。
                final_observation = Some(SequenceStepFinalObservation {
                    // 保存封闭结果类别。
                    outcome,
                    // 保存完成事实。
                    completed,
                    // 保存重试事实。
                    retry_safe,
                    // 保存可能接受事实。
                    accepted_may_have_occurred,
                    // 转移结果所有权。
                    result,
                    // 转移错误所有权。
                    error,
                });
            }
        }
    }
    // 完整 worker 退出必须建立 final 事实。
    if !allow_partial && final_observation.is_none() {
        // accepted-only 完整输出违反协议。
        return Err(SequenceStepProtocolFailure::new(
            // 使用协议状态错误。
            SequenceStepProtocolErrorCode::ProtocolFailed,
        ));
    }
    // 返回最后可靠的两阶段观察。
    Ok(SequenceStepFrameObservation {
        // 保存是否已 accepted。
        dispatch_accepted,
        // 保存可选 final。
        final_observation,
    })
}

// 构造并序列化一个 final 帧。
fn final_frame(
    // 接收请求关联值。
    request_nonce: &str,
    // 取得全部 final 字段所有权。
    fields: SequenceStepFinalFields,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 请求关联值必须保持 canonical。
    validate_nonce(request_nonce)?;
    // 字段组合必须满足唯一状态表。
    if !valid_final_shape(
        // 核对 dispatch 事实。
        fields.dispatch_accepted,
        // 核对完成事实。
        fields.completed,
        // 核对结果类别。
        fields.outcome,
        // 核对重试事实。
        fields.retry_safe,
        // 核对可能接受事实。
        fields.accepted_may_have_occurred,
        // 借用结果。
        fields.result.as_ref(),
        // 借用错误。
        fields.error.as_ref(),
    ) {
        // 构造点漂移也必须失败闭合。
        return Err(SequenceStepProtocolFailure::new(
            // 使用协议状态错误。
            SequenceStepProtocolErrorCode::ProtocolFailed,
        ));
    }
    // 构造固定版本 final 帧。
    frame_to_line(&SequenceStepWorkerFrame::Final {
        // 固定协议版本。
        contract_version: CONTRACT_VERSION.to_owned(),
        // 复制很小的请求 nonce。
        request_nonce: request_nonce.to_owned(),
        // 保存 dispatch 事实。
        dispatch_accepted: fields.dispatch_accepted,
        // 保存完成事实。
        completed: fields.completed,
        // 保存封闭结果类别。
        outcome: fields.outcome,
        // 保存重试事实。
        retry_safe: fields.retry_safe,
        // 保存可能接受事实。
        accepted_may_have_occurred: fields.accepted_may_have_occurred,
        // 转移结果。
        result: fields.result,
        // 转移错误。
        error: fields.error,
    })
}

// 把单个 worker frame 序列化为 JSON Lines 字节。
fn frame_to_line(
    // 借用固定封闭 frame。
    frame: &SequenceStepWorkerFrame,
) -> Result<Vec<u8>, SequenceStepProtocolFailure> {
    // 序列化不包含平台类型的 frame。
    let mut bytes = serde_json::to_vec(frame).map_err(|_| {
        // 结构化派生失败视为协议错误。
        SequenceStepProtocolFailure::new(SequenceStepProtocolErrorCode::ProtocolFailed)
    })?;
    // 单帧也不得超过全部 stdout 边界。
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        // 返回输出过大。
        return Err(SequenceStepProtocolFailure::new(
            // 使用封闭资源错误。
            SequenceStepProtocolErrorCode::OutputTooLarge,
        ));
    }
    // 固定追加 JSON Lines 换行。
    bytes.push(b'\n');
    // 返回单帧字节。
    Ok(bytes)
}

// 判断 final 字段组合是否属于封闭状态表。
fn valid_final_shape(
    // 接收 dispatch 事实。
    dispatch_accepted: bool,
    // 接收完成事实。
    completed: bool,
    // 接收封闭结果类别。
    outcome: SequenceStepWorkerOutcome,
    // 接收重试事实。
    retry_safe: bool,
    // 接收可能接受事实。
    accepted_may_have_occurred: bool,
    // 借用可选结果。
    result: Option<&Value>,
    // 借用可选错误。
    error: Option<&Value>,
) -> bool {
    // 所有错误负载必须包含稳定非空错误码。
    let error_code = error
        // 只处理对象错误的 code 字段。
        .and_then(|value| value.get("code"))
        // 只接受字符串错误码。
        .and_then(Value::as_str)
        // 拒绝空错误码。
        .filter(|code| !code.is_empty());
    // 按结果类别核对全部布尔与负载字段。
    match outcome {
        // dispatch 前拒绝可安全修正后重试。
        SequenceStepWorkerOutcome::NotDispatched => {
            // provider 未接受。
            !dispatch_accepted
                // provider 步骤没有完成。
                && !completed
                // 修正请求后可以重试。
                && retry_safe
                // provider 不可能已接受。
                && !accepted_may_have_occurred
                // 不得携带成功结果。
                && result.is_none()
                // 必须携带确定且非 OutcomeUnknown 的错误码。
                && error_code.is_some_and(|code| code != "OUTCOME_UNKNOWN")
        }
        // 确定成功只携带结果。
        SequenceStepWorkerOutcome::Completed => {
            // provider 已接受。
            dispatch_accepted
                // 已建立确定完成。
                && completed
                // 已完成操作不自动重试。
                && !retry_safe
                // provider 已接受。
                && accepted_may_have_occurred
                // 必须携带结果。
                && result.is_some()
                // 不得携带错误。
                && error.is_none()
        }
        // 确定失败仍禁止 generic 自动重试。
        SequenceStepWorkerOutcome::Failed => {
            // provider 已接受。
            dispatch_accepted
                // 已建立确定失败。
                && completed
                // generic 协议不证明重试安全。
                && !retry_safe
                // provider 可能已接受 mutation。
                && accepted_may_have_occurred
                // 不得携带成功结果。
                && result.is_none()
                // 必须携带确定且非 OutcomeUnknown 的错误码。
                && error_code.is_some_and(|code| code != "OUTCOME_UNKNOWN")
        }
        // 未知结果不得宣称完成或重试安全。
        SequenceStepWorkerOutcome::Unknown => {
            // provider 已接受。
            dispatch_accepted
                // 未建立确定终态。
                && !completed
                // 禁止自动重试。
                && !retry_safe
                // provider 可能已接受 mutation。
                && accepted_may_have_occurred
                // 不得携带成功结果。
                && result.is_none()
                // 必须携带统一 OutcomeUnknown 错误码。
                && error_code == Some("OUTCOME_UNKNOWN")
        }
    }
}

// 核对 frame 版本与请求关联值。
fn frame_identity_matches(
    // 借用 frame 协议版本。
    contract_version: &str,
    // 借用 frame 请求 nonce。
    request_nonce: &str,
    // 借用当前请求 nonce。
    expected_nonce: &str,
) -> bool {
    // 版本逐字匹配且 nonce 同时 canonical 与相等。
    contract_version == CONTRACT_VERSION
        // 核对 nonce 固定形状。
        && is_canonical_nonce(request_nonce)
        // 核对当前请求关联。
        && request_nonce == expected_nonce
}
