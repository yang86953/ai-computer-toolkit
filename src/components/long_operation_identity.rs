//! 生成不携带进程、时间或目标身份的长操作 handle。

// 导入统一结果。
use crate::domain::AppResult;

// 导入系统随机 nonce 与共享 opaque ID 原语。
use super::{
    // 构造 provider-neutral operation handle。
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    // 生成不可预测身份材料。
    secure_nonce_windows::random_nonce,
};

// 生成 canonical operation handle。
pub(crate) fn new_operation_id() -> AppResult<String> {
    // 使用系统随机材料作为私有身份。
    let nonce = random_nonce()?;
    // 通过统一 opaque ID 原语隐藏随机材料。
    Ok(OpaqueTargetId::new(OpaqueTargetKind::Operation, &nonce).to_string())
}

// 验证生成结果保持 canonical 且不重复。
#[cfg(test)]
mod tests {
    // 导入被测身份入口与 opaque parser。
    use super::{OpaqueTargetId, OpaqueTargetKind, new_operation_id};

    // 验证两条随机 handle 都保持 operation 类别。
    #[test]
    fn generated_operation_ids_are_canonical_and_distinct() {
        // 生成第一条 handle。
        let first = new_operation_id()
            // 当前 Windows 测试环境必须提供系统随机源。
            .unwrap_or_else(|error| panic!("first operation ID failed: {error}"));
        // 生成第二条 handle。
        let second = new_operation_id()
            // 当前 Windows 测试环境必须提供系统随机源。
            .unwrap_or_else(|error| panic!("second operation ID failed: {error}"));
        // 第一条必须严格解析为 operation 类别。
        assert_eq!(
            // 解析 canonical opaque 外壳。
            OpaqueTargetId::parse(&first).map(OpaqueTargetId::kind),
            // 核对固定类别。
            Some(OpaqueTargetKind::Operation)
        );
        // 两条系统随机身份不得重复。
        assert_ne!(first, second);
    }
}
