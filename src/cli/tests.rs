// 导入文件系统以构造和清理 CLI 输入 fixture。
use std::fs;

// 导入所有父模块私有解析入口。
use super::*;
// 导入统一 JSON 输入硬上限。
use crate::components::bounded_json_input::MAX_JSON_INPUT_BYTES;
// 显式导入测试夹具自有错误构造，不依赖父 CLI 生产导入。
use crate::domain::AppControlError;

#[test]
fn textual_arguments_preserve_numeric_literals_as_strings() -> AppResult<()> {
    let options = parse_options(&[
        "--arg".to_owned(),
        "text=42".to_owned(),
        "--arg".to_owned(),
        "key=4".to_owned(),
    ])?;

    assert_eq!(
        options.args.get("text"),
        Some(&Value::String("42".to_owned()))
    );
    assert_eq!(
        options.args.get("key"),
        Some(&Value::String("4".to_owned()))
    );
    Ok(())
}

#[test]
fn non_text_arguments_keep_scalar_types() -> AppResult<()> {
    let options = parse_options(&[
        "--arg".to_owned(),
        "x=42".to_owned(),
        "--arg".to_owned(),
        "overwrite=true".to_owned(),
    ])?;

    assert_eq!(options.args.get("x"), Some(&Value::from(42)));
    assert_eq!(options.args.get("overwrite"), Some(&Value::Bool(true)));
    Ok(())
}

#[test]
fn stdin_dash_is_accepted_as_input_value() -> AppResult<()> {
    let tokens = ["--input".to_owned(), "-".to_owned()];
    assert_eq!(required_option_value(&tokens, 1, "--input")?, "-");
    Ok(())
}

// 验证 application.discover 三类边界由 CLI 独立解析。
#[test]
fn discovery_limits_are_parsed_independently() -> AppResult<()> {
    // 构造三类不同边界。
    let options = parse_options(&[
        // 应用选项。
        "--max-applications".to_owned(),
        // 应用值。
        "11".to_owned(),
        // 进程选项。
        "--max-processes".to_owned(),
        // 进程值。
        "22".to_owned(),
        // 窗口选项。
        "--max-windows".to_owned(),
        // 窗口值。
        "33".to_owned(),
    ])?;
    // 验证应用边界。
    assert_eq!(options.max_applications, 11);
    // 验证进程边界。
    assert_eq!(options.max_processes, 22);
    // 验证窗口边界。
    assert_eq!(options.max_windows, 33);
    // 完成解析测试。
    Ok(())
}

// 验证 discover 拒绝未知 surface。
#[test]
fn discover_rejects_unknown_surface() {
    // 执行未知 surface。
    let error = match run(vec!["discover".to_owned(), "window".to_owned()]) {
        // 成功表示错误接受未知 surface。
        Ok(_) => panic!("unknown discovery surface must fail"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 验证稳定错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

// 验证 operation 在触碰 broker 前拒绝非 canonical handle 与扩展 option。
#[test]
fn long_operation_cli_rejects_invalid_routes_before_broker_access() {
    // 非 operation opaque 类别必须失败。
    let invalid_handle = run(vec![
        // 使用长操作命令。
        "operation".to_owned(),
        // 使用 status Query。
        "status".to_owned(),
        // 注入 window 类别。
        "s2:w:0000000000000001".to_owned(),
    ]);
    // 提取预期参数错误。
    let invalid_handle = match invalid_handle {
        // 成功表示 broker 可能被错误触碰。
        Ok(_) => panic!("non-operation handle unexpectedly reached broker"),
        // 保存稳定错误。
        Err(error) => error,
    };
    // 使用普通参数错误码。
    assert_eq!(invalid_handle.code, "INVALID_ARGUMENT");

    // operation 不得接受 target/path/确认类扩展。
    let injected_option = run(vec![
        // 使用长操作命令。
        "operation".to_owned(),
        // 使用 cancel Command。
        "cancel".to_owned(),
        // 使用 canonical handle。
        "s2:o:0000000000000001".to_owned(),
        // 注入 target option。
        "--target".to_owned(),
        // 注入任意字段。
        "sessionId=s2:w:0000000000000001".to_owned(),
    ]);
    // 提取预期参数错误。
    let injected_option = match injected_option {
        // 成功表示 endpoint 注入边界失效。
        Ok(_) => panic!("long operation target injection unexpectedly succeeded"),
        // 保存稳定错误。
        Err(error) => error,
    };
    // 使用普通参数错误码。
    assert_eq!(injected_option.code, "INVALID_ARGUMENT");

    // start 缺少确认时必须先于 capability、target 与 input 失败。
    let unconfirmed_start = run(vec![
        // 使用长操作命令。
        "operation".to_owned(),
        // 使用非幂等 start Command。
        "start".to_owned(),
        // 即使 capability 合法也不得跳过确认。
        "window.record@1".to_owned(),
    ]);
    // 提取预期确认错误。
    let unconfirmed_start = match unconfirmed_start {
        // 成功表示 broker 可能被错误触碰。
        Ok(_) => panic!("unconfirmed long operation start unexpectedly succeeded"),
        // 保存稳定错误。
        Err(error) => error,
    };
    // 确认错误必须独立于参数错误。
    assert_eq!(unconfirmed_start.code, "CONFIRMATION_REQUIRED");

    // 确认后的 start 仍必须要求 target 与 input。
    let incomplete_start = run(vec![
        // 使用长操作命令。
        "operation".to_owned(),
        // 使用非幂等 start Command。
        "start".to_owned(),
        // 使用首个冻结 capability。
        "window.record@1".to_owned(),
        // 显式确认副作用。
        "--confirm".to_owned(),
    ]);
    // 提取预期参数错误。
    let incomplete_start = match incomplete_start {
        // 成功表示字段门禁失效。
        Ok(_) => panic!("incomplete long operation start unexpectedly succeeded"),
        // 保存稳定错误。
        Err(error) => error,
    };
    // 确认后缺失字段使用普通参数错误。
    assert_eq!(incomplete_start.code, "INVALID_ARGUMENT");
}

// 验证捕获预检在触碰 Windows 目标前拒绝未知 surface。
#[test]
fn capture_preflight_rejects_unknown_surface_before_resolution() {
    // 执行不支持的 window surface 别名。
    let error = match run(vec![
        // 使用预检命令。
        "preflight-capture".to_owned(),
        // 注入非 app surface。
        "window".to_owned(),
    ]) {
        // 成功表示 capability surface 被错误放宽。
        Ok(_) => panic!("capture preflight surface must fail closed"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 保持 C++ capability gap 错误语义。
    assert_eq!(error.code, "CAPABILITY_GAP");
}

// 验证捕获预检要求显式 opaque 目标。
#[test]
fn capture_preflight_requires_opaque_target_before_observation() {
    // 省略 target 执行 app surface。
    let error = match run(vec![
        // 使用预检命令。
        "preflight-capture".to_owned(),
        // 使用唯一受支持 surface。
        "app".to_owned(),
    ]) {
        // 成功表示目标门禁被绕过。
        Ok(_) => panic!("capture preflight target must be required"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 缺少目标必须是稳定参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

#[test]
fn failed_sequence_returns_non_zero_exit_code() -> AppResult<()> {
    let path = std::env::temp_dir().join(format!(
        "ai-computer-toolkit-sequence-exit-{}.json",
        std::process::id()
    ));
    fs::write(
        &path,
        r#"{"steps":[{"verb":"run","app":"unknown-app","operation":"noop","confirmed":true}]}"#,
    )
    .map_err(|error| AppControlError::new("TEST_SETUP_FAILED", error.to_string()))?;
    let output = run(vec![
        "sequence".to_owned(),
        "--input".to_owned(),
        path.to_string_lossy().into_owned(),
    ])?;
    let _ = fs::remove_file(path);
    assert_eq!(output.json["ok"], false);
    assert_eq!(output.exit_code, 2);
    Ok(())
}

// 验证 CLI 在解析和命令分发前拒绝超限 JSON 文件。
#[test]
fn oversized_json_input_is_rejected_before_command_execution()
-> Result<(), Box<dyn std::error::Error>> {
    // 使用进程 ID 构造本次测试独占路径。
    let path = std::env::temp_dir().join(format!(
        // 保持文件名可诊断且不依赖随机库。
        "ai-computer-toolkit-oversized-input-{}.json",
        // 区分并行测试进程。
        std::process::id()
    ));
    // 创建本测试拥有的临时文件。
    let file = fs::File::create(&path)?;
    // 使用稀疏文件长度构造上限加一，无需写入大正文。
    file.set_len((MAX_JSON_INPUT_BYTES + 1) as u64)?;
    // 调用 sequence 以证明共享 --input 边界发生在命令执行前。
    let result = run(vec![
        // 使用公开工作流命令。
        "sequence".to_owned(),
        // 指定 JSON 来源。
        "--input".to_owned(),
        // 传入超限临时文件路径。
        path.to_string_lossy().into_owned(),
    ]);
    // 无论断言结果如何都先清理本测试文件。
    fs::remove_file(&path)?;
    // 显式取得预期参数错误。
    let error = match result {
        // 成功表示输入硬上限被绕过。
        Ok(_) => {
            return Err(std::io::Error::other("oversized input unexpectedly succeeded").into());
        }
        // 保存公开错误。
        Err(error) => error,
    };
    // 超限属于调用方参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 公开证据必须报告统一上限。
    assert_eq!(error.details["maximumBytes"], MAX_JSON_INPUT_BYTES);
    // 文件 metadata 提供精确的上限加一长度。
    assert_eq!(
        // 读取已观察长度。
        error.details["observedBytes"],
        // 对比 fixture 的真实长度。
        MAX_JSON_INPUT_BYTES + 1
    );
    // 测试正常完成。
    Ok(())
}

// 验证 CLI 严格隔离不能被 structured JSON 放宽。
#[test]
fn strict_isolation_flag_cannot_be_downgraded_by_input() -> AppResult<()> {
    // 解析严格 CLI 标志。
    let options = parse_options(&[
        // 使用独立严格隔离标志。
        "--strict-isolation".to_owned(),
    ])?;
    // 构造尝试恢复 standard 的 structured 输入。
    let input = json!({
        // 保持 structured 请求形状。
        "target": {},
        // 保持 structured 请求形状。
        "args": {},
        // 尝试提供较弱要求。
        "isolationRequirement": "standard",
    });
    // 构造运行请求。
    let request = build_request(
        // 使用运行动词。
        Verb::Run,
        // 使用不会实际执行的测试 surface。
        "desktop",
        // 使用公开 operation。
        Some("press-key"),
        // 传入 structured JSON。
        Some(&input),
        // 传入 CLI 选项。
        &options,
    )?;
    // 较强 CLI 要求必须保留。
    assert_eq!(request.isolation_requirement, IsolationRequirement::Strict);
    // 完成测试。
    Ok(())
}

// 验证严格 CLI 在任何 target 或 adapter 解析前拒绝前台路径。
#[test]
fn strict_cli_rejects_foreground_route_before_dispatch() {
    // 执行缺少 target 的严格前台请求。
    let result = run(vec![
        // 使用 run 命令。
        "run".to_owned(),
        // 使用桌面 surface。
        "desktop".to_owned(),
        // 使用原生按键 operation。
        "press-key".to_owned(),
        // 提供 mutation 确认。
        "--confirm".to_owned(),
        // 故意提供前台同意以验证它被忽略。
        "--allow-foreground".to_owned(),
        // 要求严格零打扰。
        "--strict-isolation".to_owned(),
    ]);
    // 显式取得错误而不要求成功类型实现 Debug。
    let error = match result {
        // 成功表示策略被绕过。
        Ok(_) => panic!("strict foreground route must fail before dispatch"),
        // 保存结构化策略错误。
        Err(error) => error,
    };
    // 必须返回严格隔离错误。
    assert_eq!(error.code, "ISOLATION_REQUIRED");
}
