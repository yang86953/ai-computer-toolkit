//! 验证 observation worker 的协议、cache request 与隐私边界。

// 导入父模块的私有被测项。
use super::*;

// 构造 canonical 窗口目标。
fn target() -> String {
    // 使用稳定私有测试身份。
    OpaqueTargetId::new(OpaqueTargetKind::Window, "worker-test-window").to_string()
}

// 验证树请求允许 depth 0 并强制数量与 view 边界。
#[test]
fn tree_request_bounds_are_strict() {
    // 构造最小合法树请求。
    let valid = json!({
        // 指定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 指定树 operation。
        "operation": "accessibility-tree",
        // 指定 opaque 目标。
        "sessionId": target(),
        // depth 0 只读取 root。
        "maximumDepth": 0,
        // 至少读取一个节点。
        "maximumItems": 1,
        // 使用 control view。
        "view": "control",
    });
    // 合法边界必须解析成功。
    assert!(parse_request(&valid.to_string()).is_ok());
    // 把数量上限改为零。
    let mut invalid = valid;
    // 覆盖 maximumItems。
    invalid["maximumItems"] = Value::from(0);
    // 零数量必须拒绝。
    assert_eq!(
        parse_request(&invalid.to_string())
            // 取错误以检查稳定码。
            .err()
            // 测试夹具保证错误存在。
            .map(|error| error.code),
        // 对比参数错误码。
        Some("INVALID_ARGUMENT")
    );
}

// 验证 worker 拒绝旧版本和非窗口 opaque ID。
#[test]
fn request_rejects_non_window_or_native_targets() {
    // 构造无效目标集合。
    let invalid_targets = [
        // 拒绝 native HWND。
        "1234".to_owned(),
        // 拒绝旧版本目标。
        "s1:w:0000000000000000".to_owned(),
        // 拒绝进程目标。
        OpaqueTargetId::new(OpaqueTargetKind::Process, "process").to_string(),
    ];
    // 逐个验证拒绝。
    for session_id in invalid_targets {
        // 构造 root 请求。
        let request = json!({
            // 指定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 指定 root operation。
            "operation": "accessibility-root",
            // 注入无效目标。
            "sessionId": session_id,
        });
        // 无效目标必须拒绝。
        assert!(parse_request(&request.to_string()).is_err());
    }
}

// 验证节点 JSON 不包含禁止字段。
#[test]
fn error_envelope_never_adds_native_fields() {
    // 构造安全错误。
    let envelope = error_envelope(&ObservationWorkerErrorCode::StaleSession.error("stale"));
    // 固定完整 v1 失败 envelope。
    assert_eq!(
        envelope,
        // 期望值只包含版本、稳定码与安全消息。
        json!({
            // 标记失败。
            "ok": false,
            // 保持 observation worker v1 协议。
            "contractVersion": "act/observation-worker/v1",
            // 保持最小错误对象。
            "error": {
                // 保持目标过期码。
                "code": "STALE_SESSION",
                // 保持安全消息。
                "message": "stale",
            },
        })
    );
    // 序列化以递归检查字段文本。
    let text = envelope.to_string();
    // 禁止 HWND。
    assert!(!text.contains("hwnd"));
    // 禁止 PID。
    assert!(!text.contains("processId"));
    // 禁止 bounds。
    assert!(!text.contains("bounds"));
    // 禁止 Value 内容。
    assert!(!text.contains("valueContent"));
}

// 验证 UIA HRESULT 分类仍选择既有三种公开错误码。
#[test]
fn accessibility_hresult_classification_keeps_stable_error_codes() {
    // 固定权限、过期与其他 provider 失败夹具。
    let cases = [
        // 权限拒绝必须单独分类。
        (E_ACCESSDENIED, "PERMISSION_DENIED"),
        // UIA element 不可用必须分类为 stale。
        (
            windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32),
            "STALE_SESSION",
        ),
        // 其他 HRESULT 统一分类为可访问性不可用。
        (
            windows::core::HRESULT(0x8000_4005_u32 as i32),
            "ACCESSIBILITY_UNAVAILABLE",
        ),
    ];
    // 逐项验证平台状态到封闭错误类别的选择。
    for (status, expected) in cases {
        // 调用 Worker 私有 HRESULT 分类器。
        let error = accessibility_status_error("fixture", status);
        // 公开错误码必须逐字保持稳定。
        assert_eq!(error.code, expected);
        // 平台事实只允许安全 HRESULT 文本。
        assert_eq!(
            error.message,
            // 按既有小写十六进制格式构造期望消息。
            format!("fixture failed (HRESULT 0x{:08x}).", status.0 as u32)
        );
        // 分类器不得制造额外详情。
        assert!(error.details.is_null());
    }
}

// 验证有界树读取只使用单次发现调用携带的缓存属性。
#[test]
fn bounded_tree_uses_cache_without_current_property_fallback() {
    // 将当前实现作为架构门禁输入；生产文件在测试目录的父级。
    let source = include_str!("../observation_worker.rs");
    // 从 cache request helper 开始隔离源码。
    let Some((_, after_cache_request)) = source.split_once("fn create_tree_cache_request(") else {
        // 测试源码结构变化时立即失败。
        panic!("tree cache request function must exist");
    };
    // 在协议解析实现前停止。
    let Some((cache_request, _)) = after_cache_request.split_once("fn parse_request(") else {
        // 测试源码结构变化时立即失败。
        panic!("request parser function must exist");
    };
    // 名称必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_NamePropertyId"));
    // automation ID 必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_AutomationIdPropertyId"));
    // 类名必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_ClassNamePropertyId"));
    // framework ID 必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_FrameworkIdPropertyId"));
    // control type 必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_ControlTypePropertyId"));
    // enabled 必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_IsEnabledPropertyId"));
    // offscreen 必须纳入同一个 cache request。
    assert!(cache_request.contains("UIA_IsOffscreenPropertyId"));
    // request 必须保持严格的 7 项允许属性集合。
    assert_eq!(cache_request.matches("PropertyId,").count(), 7);
    // 从节点投影函数开始隔离源码。
    let Some((_, after_node_projection)) = source.split_once("fn node_json(") else {
        // 测试源码结构变化时立即失败。
        panic!("node projection function must exist");
    };
    // 在 root-only 实现前停止。
    let Some((node_projection, _)) = after_node_projection.split_once("fn observe_root(") else {
        // 测试源码结构变化时立即失败。
        panic!("root observation function must exist");
    };
    // 所有公开字符串属性必须来自 cache。
    assert!(node_projection.contains("CachedName"));
    // automation ID 必须来自 cache。
    assert!(node_projection.contains("CachedAutomationId"));
    // 类名必须来自 cache。
    assert!(node_projection.contains("CachedClassName"));
    // framework ID 必须来自 cache。
    assert!(node_projection.contains("CachedFrameworkId"));
    // control type 必须来自 cache。
    assert!(node_projection.contains("CachedControlType"));
    // enabled 必须来自 cache。
    assert!(node_projection.contains("CachedIsEnabled"));
    // offscreen 必须来自 cache。
    assert!(node_projection.contains("CachedIsOffscreen"));
    // 禁止缓存缺失时静默退回跨 provider 的 Current* 调用。
    assert!(!node_projection.contains("Current"));
    // 节点 identity 必须来自 UIA RuntimeId 窄组件。
    assert!(node_projection.contains("runtime_element_id"));
    // 禁止恢复易随 sibling 排序漂移的树路径 identity。
    assert!(!node_projection.contains("path"));
    // 从树实现开始隔离源码。
    let Some((_, after_tree_observation)) = source.split_once("fn observe_tree(") else {
        // 测试源码结构变化时立即失败。
        panic!("tree observation function must exist");
    };
    // 在协议执行函数前停止。
    let Some((tree_observation, _)) = after_tree_observation.split_once("fn execute_request(")
    else {
        // 测试源码结构变化时立即失败。
        panic!("request execution function must exist");
    };
    // root 发现必须同时构建缓存。
    assert!(tree_observation.contains("ElementFromHandleBuildCache"));
    // child 发现必须同时构建缓存。
    assert!(tree_observation.contains("GetFirstChildElementBuildCache"));
    // sibling 发现必须同时构建缓存。
    assert!(tree_observation.contains("GetNextSiblingElementBuildCache"));
    // 禁止 root 退回不带缓存的发现调用。
    assert!(!tree_observation.contains("ElementFromHandle("));
    // 禁止 child 退回不带缓存的 walker 调用。
    assert!(!tree_observation.contains("GetFirstChildElement("));
    // 禁止 sibling 退回不带缓存的 walker 调用。
    assert!(!tree_observation.contains("GetNextSiblingElement("));
}
