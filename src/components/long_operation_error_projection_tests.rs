//! 验证长操作业务接受前错误不会泄漏动态上下文。

// 导入被测投影函数。
use super::pre_acceptance;
// 导入统一错误构造器。
use crate::domain::AppControlError;

// 验证登记错误保留分类且未知错误失败闭合。
#[test]
fn pre_acceptance_projection_is_closed_and_static() {
    // 构造全部受支持输入与预期输出码。
    let fixtures = [
        // 参数错误保持参数分类。
        ("INVALID_ARGUMENT", "INVALID_ARGUMENT"),
        // 确认错误保持确认分类。
        ("CONFIRMATION_REQUIRED", "CONFIRMATION_REQUIRED"),
        // 权限错误保持权限分类。
        ("PERMISSION_DENIED", "PERMISSION_DENIED"),
        // 后台缺口收敛为 capability 缺口。
        ("BACKGROUND_OPERATION_UNAVAILABLE", "CAPABILITY_GAP"),
        // 未登记错误失败闭合。
        ("UNEXPECTED_INTERNAL_ERROR", "BROKER_UNAVAILABLE"),
    ];
    // 逐项验证动态消息不进入公开投影。
    for (input_code, expected_code) in fixtures {
        // 构造包含敏感样例路径的动态错误。
        let error = AppControlError::new(input_code, "C:\\sensitive\\capture.mp4");
        // 执行纯投影。
        let (code, message) = pre_acceptance(&error);
        // 核对封闭错误码。
        assert_eq!(code, expected_code);
        // 禁止动态路径进入安全消息。
        assert!(!message.contains("sensitive"));
    }
}
