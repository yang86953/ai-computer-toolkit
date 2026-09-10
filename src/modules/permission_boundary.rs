//! 受保护上下文与活动交互会话 Permission Assessment Module。

// 导入安全证据 JSON 构造器。
use serde_json::json;

// 导入统一结果类型。
use crate::domain::AppResult;

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "permission_boundary_error.rs"]
mod error_code;
// 导入当前 Module 私有封闭错误码。
use error_code::PermissionBoundaryErrorCode;

// 表示 System 即将执行的精确目标访问类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetAccessKind {
    // 表示读取、观察或捕获一个精确目标。
    Read,
    // 表示可能改变目标或外部状态的操作。
    Mutation,
}

// 为访问类别提供稳定公开证据文本。
impl TargetAccessKind {
    // 返回契约规定的 kebab-case 值。
    const fn as_str(self) -> &'static str {
        // 穷举两种访问类别。
        match self {
            // 映射目标读取。
            Self::Read => "target-read",
            // 映射目标 mutation。
            Self::Mutation => "target-mutation",
        }
    }
}

// 表示私有 Windows 当前会话探针的封闭事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionSecurityState {
    // 表示活动的非零交互会话。
    ActiveInteractive,
    // 表示 Windows Session 0。
    SessionZero,
    // 表示当前会话已断开或不是活动状态。
    NotActive,
    // 表示平台无法可靠取得会话状态。
    Indeterminate,
}

// 表示私有 Windows 桌面探针的封闭事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSecurityState {
    // 表示当前进程位于活动输入 Default 桌面。
    ActiveDefaultInput,
    // 表示受保护、不可访问或非输入桌面。
    ProtectedOrNonInput,
    // 表示平台无法可靠比较桌面身份。
    Indeterminate,
}

// 聚合平台 Adapter 允许交给 Module 的最小无敏感事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HostSecurityFacts {
    // 保存当前进程会话分类。
    pub(crate) session: SessionSecurityState,
    // 保存当前进程桌面分类。
    pub(crate) desktop: DesktopSecurityState,
}

// 定义 Permission Assessment 所需的只读平台端口。
pub(crate) trait SecurityContextProbe: Send + Sync {
    // 返回不含 SID、token、handle、名称或路径的封闭事实。
    fn probe(&self) -> HostSecurityFacts;
}

// 表示纯评估产生的封闭授权结论。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostSecurityDecision {
    // 表示允许 System 继续解析精确目标。
    Allowed,
    // 表示已确认的系统安全边界阻塞。
    Blocked(&'static str),
    // 表示无法可靠认证安全姿态。
    Indeterminate(&'static str),
}

// 按固定优先级评估会话与桌面事实。
const fn assess(facts: HostSecurityFacts) -> HostSecurityDecision {
    // 会话边界优先于桌面状态。
    match facts.session {
        // Session 0 永不进入交互控制路线。
        SessionSecurityState::SessionZero => HostSecurityDecision::Blocked("session-zero"),
        // 非活动会话不得控制精确目标。
        SessionSecurityState::NotActive => HostSecurityDecision::Blocked("session-not-active"),
        // 无法取得会话事实时必须失败闭合。
        SessionSecurityState::Indeterminate => {
            // 返回稳定不可确定原因。
            HostSecurityDecision::Indeterminate("session-state-indeterminate")
        }
        // 只有活动非零会话继续检查桌面。
        SessionSecurityState::ActiveInteractive => match facts.desktop {
            // 活动输入 Default 桌面满足主机姿态条件。
            DesktopSecurityState::ActiveDefaultInput => HostSecurityDecision::Allowed,
            // 受保护或非输入桌面明确阻塞。
            DesktopSecurityState::ProtectedOrNonInput => {
                // 使用不泄漏桌面名称的稳定原因。
                HostSecurityDecision::Blocked("protected-or-non-input-desktop")
            }
            // 桌面身份不可可靠比较时不得猜测。
            DesktopSecurityState::Indeterminate => {
                // 返回稳定不可确定原因。
                HostSecurityDecision::Indeterminate("desktop-state-indeterminate")
            }
        },
    }
}

// 构造所有前置失败共享的零目标访问证据。
fn failure_details(
    // 接收封闭决策名称。
    decision: &'static str,
    // 接收稳定 provider-neutral 原因。
    reason: &'static str,
    // 接收本次请求的访问类别。
    access_kind: TargetAccessKind,
) -> serde_json::Value {
    // 返回不含原生安全事实的固定对象。
    json!({
        // 输出 blocked 或 indeterminate。
        "decision": decision,
        // 输出稳定安全原因。
        "reason": reason,
        // 输出目标读取或 mutation 分类。
        "accessKind": access_kind.as_str(),
        // 证明门禁发生在 provider 与目标访问前。
        "evaluatedBeforeTargetAccess": true,
        // 证明没有读取精确目标。
        "targetReadAttempted": false,
        // 证明没有写入精确目标。
        "targetWriteAttempted": false,
        // 证明没有请求或尝试提权。
        "elevationAttempted": false,
        // 证明没有尝试注入。
        "injectionAttempted": false,
        // 证明没有尝试备用 provider 或前台降级。
        "fallbackAttempted": false,
        // 前置失败没有产生目标 mutation。
        "retrySafe": true,
        // 安全状态变化必须由调用方显式重新发起并重新评估。
        "automaticRetryProhibited": true,
    })
}

// 在 System 解析 provider 或目标前授权一次精确访问。
pub(crate) fn authorize(
    // 借用只读平台探针端口。
    probe: &dyn SecurityContextProbe,
    // 接收目标访问类别。
    access_kind: TargetAccessKind,
) -> AppResult<()> {
    // 每次请求只捕获一次封闭安全姿态快照。
    let decision = assess(probe.probe());
    // 将纯结论映射为稳定公开结果。
    match decision {
        // 允许 System 继续协调 provider。
        HostSecurityDecision::Allowed => Ok(()),
        // 明确阻塞返回统一权限错误。
        HostSecurityDecision::Blocked(reason) => {
            // 构造零目标访问的公开失败。
            Err(PermissionBoundaryErrorCode::PermissionDenied.with_details(
                // 消息不公开具体原生桌面或会话值。
                "The current Windows security context does not permit exact target access.",
                // 附加稳定且最小的失败证据。
                failure_details("blocked", reason, access_kind),
            ))
        }
        // 不可确定结论返回 assessment 缺口。
        HostSecurityDecision::Indeterminate(reason) => {
            // 构造零目标访问的不可确定失败。
            Err(
                PermissionBoundaryErrorCode::CapabilityAssessmentUnavailable.with_details(
                    // 消息仅说明无法认证，不泄漏探针错误。
                    "The current Windows security context cannot be certified for exact target access.",
                    // 附加稳定且最小的失败证据。
                    failure_details("indeterminate", reason, access_kind),
                ),
            )
        }
    }
}

// 声明纯矩阵与端口调用的回归测试。
#[cfg(test)]
mod tests {
    // 导入原子计数器验证探针只调用一次。
    use std::sync::atomic::{AtomicUsize, Ordering};

    // 导入被测封闭类型与函数。
    use super::*;

    // 实现可重复的合成安全姿态探针。
    struct SyntheticProbe {
        // 保存固定事实。
        facts: HostSecurityFacts,
        // 记录探针调用次数。
        calls: AtomicUsize,
    }

    // 为合成探针实现只读端口。
    impl SecurityContextProbe for SyntheticProbe {
        // 返回固定事实并记录唯一调用。
        fn probe(&self) -> HostSecurityFacts {
            // 使用顺序无关计数覆盖单次调用不变量。
            self.calls.fetch_add(1, Ordering::Relaxed);
            // 返回复制的封闭事实。
            self.facts
        }
    }

    // 构造合成探针并执行一次授权。
    fn authorize_facts(
        // 接收会话状态。
        session: SessionSecurityState,
        // 接收桌面状态。
        desktop: DesktopSecurityState,
        // 接收访问类别。
        access_kind: TargetAccessKind,
    ) -> (AppResult<()>, usize) {
        // 构造零调用探针。
        let probe = SyntheticProbe {
            // 保存合成事实。
            facts: HostSecurityFacts { session, desktop },
            // 初始化调用计数。
            calls: AtomicUsize::new(0),
        };
        // 执行生产授权函数。
        let result = authorize(&probe, access_kind);
        // 读取最终调用次数。
        let calls = probe.calls.load(Ordering::Relaxed);
        // 返回结果与计数。
        (result, calls)
    }

    // 验证唯一允许的活动 Default 输入桌面组合。
    #[test]
    fn active_default_input_desktop_is_allowed_once() {
        // 执行活动会话读取授权。
        let (result, calls) = authorize_facts(
            // 提供活动交互会话。
            SessionSecurityState::ActiveInteractive,
            // 提供活动 Default 输入桌面。
            DesktopSecurityState::ActiveDefaultInput,
            // 使用目标读取类别。
            TargetAccessKind::Read,
        );
        // 唯一认证组合必须通过。
        assert!(result.is_ok());
        // 每个请求只允许一次平台快照。
        assert_eq!(calls, 1);
    }

    // 验证三个明确阻塞状态共享稳定错误与零写证据。
    #[test]
    fn protected_session_states_fail_before_target_access() {
        // 枚举 Session 0、非活动会话和非输入桌面。
        let cases = [
            // Session 0 优先于任意桌面事实。
            (
                SessionSecurityState::SessionZero,
                DesktopSecurityState::Indeterminate,
                "session-zero",
            ),
            // 非活动会话优先于桌面事实。
            (
                SessionSecurityState::NotActive,
                DesktopSecurityState::ActiveDefaultInput,
                "session-not-active",
            ),
            // 活动会话中的受保护桌面明确阻塞。
            (
                SessionSecurityState::ActiveInteractive,
                DesktopSecurityState::ProtectedOrNonInput,
                "protected-or-non-input-desktop",
            ),
        ];
        // 逐项覆盖稳定安全原因。
        for (session, desktop, reason) in cases {
            // 执行 mutation 授权。
            let (result, calls) = authorize_facts(session, desktop, TargetAccessKind::Mutation);
            // 明确阻塞不得成功。
            let error = result
                // 取得预期错误。
                .err()
                // 不可达成功需要明确测试失败。
                .unwrap_or_else(|| panic!("protected context must fail"));
            // 使用统一权限拒绝码。
            assert_eq!(error.code, "PERMISSION_DENIED");
            // 核对稳定原因。
            assert_eq!(error.details["reason"], reason);
            // 核对 mutation 分类。
            assert_eq!(error.details["accessKind"], "target-mutation");
            // 门禁必须早于任何目标读取。
            assert_eq!(error.details["targetReadAttempted"], false);
            // 门禁必须早于任何目标写入。
            assert_eq!(error.details["targetWriteAttempted"], false);
            // 不得尝试 fallback。
            assert_eq!(error.details["fallbackAttempted"], false);
            // 每个请求只探测一次。
            assert_eq!(calls, 1);
        }
    }

    // 验证不可确定会话和桌面保持独立稳定原因。
    #[test]
    fn indeterminate_facts_never_authorize_target_reads() {
        // 枚举两种不可确定来源。
        let cases = [
            // 会话状态不可确定。
            (
                SessionSecurityState::Indeterminate,
                DesktopSecurityState::ActiveDefaultInput,
                "session-state-indeterminate",
            ),
            // 桌面状态不可确定。
            (
                SessionSecurityState::ActiveInteractive,
                DesktopSecurityState::Indeterminate,
                "desktop-state-indeterminate",
            ),
        ];
        // 逐项核对失败闭合结果。
        for (session, desktop, reason) in cases {
            // 执行目标读取授权。
            let (result, calls) = authorize_facts(session, desktop, TargetAccessKind::Read);
            // 不可确定事实不得成功。
            let error = result
                // 取得预期错误。
                .err()
                // 不可达成功需要明确测试失败。
                .unwrap_or_else(|| panic!("indeterminate context must fail"));
            // 使用 assessment 不可用错误。
            assert_eq!(error.code, "CAPABILITY_ASSESSMENT_UNAVAILABLE");
            // 核对稳定原因。
            assert_eq!(error.details["reason"], reason);
            // 核对只读访问分类。
            assert_eq!(error.details["accessKind"], "target-read");
            // 明确禁止自动重试。
            assert_eq!(error.details["automaticRetryProhibited"], true);
            // 每个请求只探测一次。
            assert_eq!(calls, 1);
        }
    }
}
