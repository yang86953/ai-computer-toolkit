//! 验证 legacy Desktop Adapter 的纯参数与路由门禁。

// 导入统一 Adapter trait、请求与动词类型。
use crate::{
    // 导入 Adapter trait 以调用被测兼容入口。
    adapters::AppAdapter,
    // 导入语言中立请求与动词。
    domain::{CommandRequest, Verb},
};
// 导入 JSON 参数值。
use serde_json::{Map, Value};

// 导入被测 Adapter 与纯辅助函数。
use super::{
    // 导入公开兼容 Adapter。
    DesktopAdapter,
    // 导入按键输入序列构造器。
    chord_inputs,
    // 导入命名按键解析器。
    named_key,
    // 导入有界按键持续时间解析器。
    optional_hold_ms,
    // 导入有界按键阶段解析器。
    optional_key_phase,
    // 导入 argv 封闭解析器。
    parse_argv,
    // 导入必需坐标解析器。
    required_i32,
    // 导入必需字符串解析器。
    required_string,
};

// 验证 operation 与确认门禁在任何 provider 或 OS 写操作前保持稳定。
#[test]
fn desktop_owned_operation_gates_fail_before_provider_or_os_access() {
    // 构造不含 operation 的基础 run 请求。
    let missing_request = CommandRequest::read(Verb::Run, "desktop");
    // 缺失 operation 必须在任何下层 Module 前失败。
    let missing_error = DesktopAdapter
        // 调用纯 Adapter 分发门禁。
        .run(&missing_request)
        // 缺失 operation 不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing Desktop operation must fail"));
    // 保持稳定参数错误码。
    assert_eq!(missing_error.code, "INVALID_ARGUMENT");
    // 保持既有 operation 缺失消息。
    assert_eq!(missing_error.message, "run 缺少 operation。");

    // 克隆请求并提供未认证 operation。
    let mut unsupported_request = missing_request;
    // 使用不会进入任何真实 provider 的合成 operation。
    unsupported_request.operation = Some("replace-desktop".to_owned());
    // 未认证 operation 必须在 OS 写操作前失败。
    let unsupported_error = DesktopAdapter
        // 调用纯 Adapter 分发门禁。
        .run(&unsupported_request)
        // 未认证 operation 不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("unsupported Desktop operation must fail"));
    // 保持稳定后台操作缺口码。
    assert_eq!(
        // 读取公开错误码。
        unsupported_error.code,
        // 对照既有稳定文本。
        "BACKGROUND_OPERATION_UNAVAILABLE"
    );
    // 保持仅回显 operation 身份的既有消息。
    assert_eq!(
        // 读取公开错误消息。
        unsupported_error.message,
        // 对照既有兼容消息。
        "desktop.replace-desktop 未认证为控制操作。"
    );

    // 固定三条 confirmation-first operation 与既有消息。
    let confirmation_cases = [
        // 录制必须在目标和文件系统访问前确认。
        (
            "record",
            "Exact window recording requires explicit confirmation.",
        ),
        // 截图必须在目标和文件系统访问前确认。
        (
            "screenshot",
            "Exact window screenshot requires explicit confirmation.",
        ),
        // 窗口关闭必须在目标解析和消息发送前确认。
        ("close", "Exact window close requires confirmation."),
    ];
    // 逐项验证未确认请求不会访问真实 provider。
    for (operation, expected_message) in confirmation_cases {
        // 构造干净的未确认 run 请求。
        let mut request = CommandRequest::read(Verb::Run, "desktop");
        // 注入当前被测 operation。
        request.operation = Some(operation.to_owned());
        // 未确认请求必须由 Adapter 自有门禁拒绝。
        let error = DesktopAdapter
            // 调用对应兼容 operation。
            .run(&request)
            // 未确认路径不得成功。
            .err()
            // 使用显式 panic 保留 operation 上下文。
            .unwrap_or_else(|| panic!("unconfirmed Desktop {operation} must fail"));
        // 保持稳定确认错误码。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        // 保持每条 operation 的既有公开消息。
        assert_eq!(error.message, expected_message);
    }
}

// 验证纯参数辅助函数保持既有错误 envelope 与消息。
#[test]
fn desktop_owned_parameter_gates_keep_stable_errors() {
    // 构造空参数对象。
    let args = Map::new();
    // 缺失字符串必须稳定拒绝。
    let string_error = required_string(&args, "path")
        // 缺失值不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing Desktop string must fail"));
    // 保持通用参数错误码。
    assert_eq!(string_error.code, "INVALID_ARGUMENT");
    // 保持字段名限定消息。
    assert_eq!(string_error.message, "args.path 是必填字符串。");

    // 缺失坐标必须稳定拒绝。
    let coordinate_error = required_i32(&args, "x")
        // 缺失坐标不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing Desktop coordinate must fail"));
    // 保持通用参数错误码。
    assert_eq!(coordinate_error.code, "INVALID_ARGUMENT");
    // 保持坐标类型消息。
    assert_eq!(coordinate_error.message, "args.x 必须是 i32 坐标。");

    // 非数组 argv 必须稳定拒绝。
    let argv_error = parse_argv(&Value::String("--unsafe".to_owned()))
        // 任意字符串不得冒充 argv 数组。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("non-array Desktop argv must fail"));
    // 保持通用参数错误码。
    assert_eq!(argv_error.code, "INVALID_ARGUMENT");
    // 保持 argv 外壳消息。
    assert_eq!(argv_error.message, "args.argv 必须是字符串数组。");
}

// 验证按键持续时间硬边界。
#[test]
fn key_hold_duration_is_bounded() -> crate::domain::AppResult<()> {
    // 缺失持续时间使用零默认值。
    assert_eq!(optional_hold_ms(None)?, 0);
    // 合法持续时间保持原值。
    assert_eq!(optional_hold_ms(Some(&Value::from(1_250)))?, 1_250);
    // 提取超过上界的稳定错误。
    let error = optional_hold_ms(Some(&Value::from(5_001)))
        // 超限值不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("oversized Desktop hold must fail"));
    // 保持通用参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 保持既有持续时间消息。
    assert_eq!(
        // 读取公开错误消息。
        error.message,
        // 对照既有范围说明。
        "args.holdMs must be an integer between 0 and 5000."
    );
    // 负持续时间必须拒绝。
    assert!(optional_hold_ms(Some(&Value::from(-1))).is_err());
    // 返回测试成功。
    Ok(())
}

// 验证 F4 保持可用于标准窗口关闭组合键。
#[test]
fn f4_is_available_for_standard_window_close_chords() {
    // 命名按键解析必须接受 F4。
    assert!(named_key("F4").is_ok());
}

// 验证同时按键生成按下与释放序列。
#[test]
fn simultaneous_ascii_keys_form_a_press_release_chord() -> crate::domain::AppResult<()> {
    // 两个按键应产生四个输入事件。
    assert_eq!(chord_inputs("w+d")?.len(), 4);
    // 返回测试成功。
    Ok(())
}

// 验证按键阶段默认值与拒绝消息。
#[test]
fn key_phase_defaults_to_press_and_rejects_unknown_values() -> crate::domain::AppResult<()> {
    // 缺失阶段使用 press。
    assert_eq!(optional_key_phase(None)?, "press");
    // 合法 down 阶段保持原值。
    assert_eq!(optional_key_phase(Some(&Value::from("down")))?, "down");
    // 提取未知阶段错误。
    let error = optional_key_phase(Some(&Value::from("hold")))
        // 未知阶段不得成功。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("unknown Desktop key phase must fail"));
    // 保持通用参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 保持既有封闭阶段消息。
    assert_eq!(
        error.message,
        "args.phase must be 'press', 'down', or 'up'."
    );
    // 返回测试成功。
    Ok(())
}
