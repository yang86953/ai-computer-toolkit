//! 纯静态后台 mutation 权限评估 Component。

// 表示进程元数据访问的封闭分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StaticMetadataAccess {
    // 表示所需元数据可读。
    Available,
    // 表示读取被系统权限明确阻止。
    PermissionBlocked,
    // 表示元数据当前不可用。
    Unavailable,
}

// 表示目标相对当前工具的完整性关系。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StaticIntegrityRelation {
    // 表示目标完整性更低。
    Lower,
    // 表示目标完整性相同。
    Same,
    // 表示目标完整性更高。
    Higher,
    // 表示完整性关系无法确定。
    Unknown,
}

// 保存不执行主动写探针的权限决策。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StaticPermissionAssessment {
    // 保存封闭决策名称。
    pub(crate) decision: &'static str,
    // 保存稳定权限关系原因。
    pub(crate) permission_relation: &'static str,
    // 标记是否可以绕过逐操作确认立即执行。
    pub(crate) safe_to_execute_now: bool,
    // 标记 mutation 是否要求确认。
    pub(crate) requires_confirmation: bool,
    // 标记是否需要前台输入。
    pub(crate) foreground_required: bool,
    // 标记是否执行过主动写探针。
    pub(crate) active_write_probe_performed: bool,
}

// 构造固定的保守权限结果。
const fn result(
    // 接收封闭决策。
    decision: &'static str,
    // 接收稳定权限关系。
    permission_relation: &'static str,
) -> StaticPermissionAssessment {
    // 返回无前台、无主动试写且仍需确认的结果。
    StaticPermissionAssessment {
        // 保存决策。
        decision,
        // 保存权限关系。
        permission_relation,
        // 静态评估永不直接授权 mutation。
        safe_to_execute_now: false,
        // 可用路径仍要求逐操作确认。
        requires_confirmation: true,
        // Standard Edit 认证路径不需要前台。
        foreground_required: false,
        // 静态评估不得试写用户目标。
        active_write_probe_performed: false,
    }
}

// 按 C++ 对照规则评估同会话后台 mutation 权限。
pub(crate) const fn assess_static_background_mutation(
    // 接收元数据访问事实。
    metadata_access: StaticMetadataAccess,
    // 接收完整性关系事实。
    integrity_relation: StaticIntegrityRelation,
) -> StaticPermissionAssessment {
    // 明确访问拒绝优先返回权限阻塞。
    if matches!(metadata_access, StaticMetadataAccess::PermissionBlocked) {
        // 保留与 C++ Component 相同的原因文本。
        return result("permission-blocked", "target-metadata-permission-blocked");
    }
    // 元数据不可用时不得推测权限成立。
    if matches!(metadata_access, StaticMetadataAccess::Unavailable) {
        // 返回未知关系。
        return result("indeterminate", "unknown");
    }
    // 更高完整性目标不能进入认证写路径。
    if matches!(integrity_relation, StaticIntegrityRelation::Higher) {
        // 返回稳定权限阻塞原因。
        return result("permission-blocked", "target-higher-integrity");
    }
    // 未知完整性关系不得通过静态门禁。
    if matches!(integrity_relation, StaticIntegrityRelation::Unknown) {
        // 返回保守未知结果。
        return result("indeterminate", "unknown");
    }
    // 同级或较低完整性只证明可在确认后尝试固定消息。
    result(
        "requires-confirmation",
        "no-static-integrity-block-observed",
    )
}

// 仅在测试构建中验证四条权限分支。
#[cfg(test)]
mod tests {
    // 导入被测纯函数与封闭输入。
    use super::*;

    // 验证同级完整性要求确认且不执行试写。
    #[test]
    fn same_integrity_requires_confirmation_without_probe() {
        // 执行纯评估。
        let assessment = assess_static_background_mutation(
            // 提供可读元数据。
            StaticMetadataAccess::Available,
            // 提供同级完整性。
            StaticIntegrityRelation::Same,
        );
        // 核对决策。
        assert_eq!(assessment.decision, "requires-confirmation");
        // 核对未授权立即执行。
        assert!(!assessment.safe_to_execute_now);
        // 核对逐操作确认要求。
        assert!(assessment.requires_confirmation);
        // 核对没有前台要求。
        assert!(!assessment.foreground_required);
        // 核对没有主动试写。
        assert!(!assessment.active_write_probe_performed);
    }

    // 验证较低完整性沿用同一确认门禁。
    #[test]
    fn lower_integrity_requires_confirmation() {
        // 执行纯评估。
        let assessment = assess_static_background_mutation(
            // 提供可读元数据。
            StaticMetadataAccess::Available,
            // 提供较低完整性。
            StaticIntegrityRelation::Lower,
        );
        // 核对决策。
        assert_eq!(assessment.decision, "requires-confirmation");
        // 核对稳定原因。
        assert_eq!(
            assessment.permission_relation,
            "no-static-integrity-block-observed"
        );
    }

    // 验证更高完整性被阻断。
    #[test]
    fn higher_integrity_is_permission_blocked() {
        // 执行纯评估。
        let assessment = assess_static_background_mutation(
            // 提供可读元数据。
            StaticMetadataAccess::Available,
            // 提供更高完整性。
            StaticIntegrityRelation::Higher,
        );
        // 核对权限阻塞。
        assert_eq!(assessment.decision, "permission-blocked");
        // 核对稳定原因。
        assert_eq!(assessment.permission_relation, "target-higher-integrity");
    }

    // 验证不可用元数据保持未知。
    #[test]
    fn unavailable_metadata_is_indeterminate() {
        // 执行纯评估。
        let assessment = assess_static_background_mutation(
            // 提供不可用元数据。
            StaticMetadataAccess::Unavailable,
            // 即使完整性看似同级也不得放宽。
            StaticIntegrityRelation::Same,
        );
        // 核对未知决策。
        assert_eq!(assessment.decision, "indeterminate");
        // 核对未知关系。
        assert_eq!(assessment.permission_relation, "unknown");
    }

    // 验证明确元数据权限拒绝优先于完整性关系。
    #[test]
    fn metadata_permission_block_is_authoritative() {
        // 执行纯评估。
        let assessment = assess_static_background_mutation(
            // 提供明确访问拒绝。
            StaticMetadataAccess::PermissionBlocked,
            // 提供未知完整性。
            StaticIntegrityRelation::Unknown,
        );
        // 核对权限阻塞。
        assert_eq!(assessment.decision, "permission-blocked");
        // 核对稳定原因。
        assert_eq!(
            assessment.permission_relation,
            "target-metadata-permission-blocked"
        );
    }

    // 穷举元数据访问与完整性关系的全部十二种组合。
    #[test]
    // 锁定优先级、决策映射和所有静态安全常量。
    fn all_static_permission_combinations_fail_closed() {
        // 枚举封闭元数据访问输入。
        let metadata_cases = [
            // 元数据可读。
            StaticMetadataAccess::Available,
            // 元数据读取被权限阻止。
            StaticMetadataAccess::PermissionBlocked,
            // 元数据不可用。
            StaticMetadataAccess::Unavailable,
        ];
        // 枚举封闭完整性关系输入。
        let integrity_cases = [
            // 目标完整性较低。
            StaticIntegrityRelation::Lower,
            // 目标完整性相同。
            StaticIntegrityRelation::Same,
            // 目标完整性较高。
            StaticIntegrityRelation::Higher,
            // 目标完整性未知。
            StaticIntegrityRelation::Unknown,
        ];
        // 逐元数据状态展开矩阵。
        for metadata in metadata_cases {
            // 逐完整性关系覆盖全部组合。
            for integrity in integrity_cases {
                // 执行无平台依赖的纯静态评估。
                let assessment = assess_static_background_mutation(metadata, integrity);
                // 按权限优先级计算唯一期望决策与原因。
                let expected = match metadata {
                    // 明确权限拒绝优先于任何完整性关系。
                    StaticMetadataAccess::PermissionBlocked => (
                        // 期望权限阻塞决策。
                        "permission-blocked",
                        // 期望元数据权限原因。
                        "target-metadata-permission-blocked",
                    ),
                    // 元数据不可用时不得使用完整性输入猜测。
                    StaticMetadataAccess::Unavailable => (
                        // 期望保守未知决策。
                        "indeterminate",
                        // 期望未知关系。
                        "unknown",
                    ),
                    // 元数据可用时再按完整性关系分类。
                    StaticMetadataAccess::Available => match integrity {
                        // 较低或同级目标只允许确认后尝试。
                        StaticIntegrityRelation::Lower | StaticIntegrityRelation::Same => (
                            // 期望逐操作确认。
                            "requires-confirmation",
                            // 期望没有观察到静态完整性阻塞。
                            "no-static-integrity-block-observed",
                        ),
                        // 较高完整性目标必须阻塞。
                        StaticIntegrityRelation::Higher => (
                            // 期望权限阻塞。
                            "permission-blocked",
                            // 期望较高完整性原因。
                            "target-higher-integrity",
                        ),
                        // 未知关系必须保持不确定。
                        StaticIntegrityRelation::Unknown => (
                            // 期望保守未知决策。
                            "indeterminate",
                            // 期望未知关系。
                            "unknown",
                        ),
                    },
                };
                // 核对封闭决策。
                assert_eq!(assessment.decision, expected.0);
                // 核对稳定权限原因。
                assert_eq!(assessment.permission_relation, expected.1);
                // 静态评估永不直接授权立即执行。
                assert!(!assessment.safe_to_execute_now);
                // 所有可达结果仍保留逐操作确认要求。
                assert!(assessment.requires_confirmation);
                // Standard Edit 静态路径不得要求前台输入。
                assert!(!assessment.foreground_required);
                // 任何组合都不得执行主动写探针。
                assert!(!assessment.active_write_probe_performed);
            }
        }
    }
}
