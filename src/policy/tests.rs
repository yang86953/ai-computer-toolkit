// 导入父 Module 的纯策略边界。
use super::*;

// 将浏览器公开路由回归定位到窄文件以保持本文件行数上限。
#[path = "browser_session_public_route_tests.rs"]
mod browser_session_public_route_tests;

// 构造不触碰 provider 的运行请求。
fn request(app: &str, operation: &str, requirement: IsolationRequirement) -> CommandRequest {
    // 从默认请求开始。
    let mut request = CommandRequest::read(Verb::Run, app);
    // 设置公开 operation。
    request.operation = Some(operation.to_owned());
    // 设置调用方隔离要求。
    request.isolation_requirement = requirement;
    // 预置逐操作确认以聚焦隔离门禁。
    request.confirmed = true;
    // 预置前台同意以证明严格模式不会复用它。
    request.foreground_consent = true;
    // 返回夹具。
    request
}

// 构造纯状态机计划。
fn plan(
    // 接收契约要求域。
    required_realm: ExecutionRealm,
    // 接收真实运行域。
    runtime_realm: ExecutionRealm,
    // 接收路由认证事实。
    isolated_route_certified: bool,
    // 接收 companion 可达事实。
    isolated_worker_available: bool,
) -> ExecutionPlan {
    // 返回严格计划。
    ExecutionPlan {
        // 保存要求域。
        required_realm,
        // 保存运行域。
        runtime_realm,
        // 使用严格主机影响策略。
        host_impact_policy: HostImpactPolicy::StrictNoInterference,
        // 使用严格隔离要求。
        isolation_requirement: IsolationRequirement::Strict,
        // 保存认证事实。
        isolated_route_certified,
        // 保存可达事实。
        isolated_worker_available,
    }
}

// 执行通用 Policy 并要求返回结构化错误。
fn validation_error(request: &CommandRequest) -> AppControlError {
    // 成功表示测试期门禁失效。
    validate(request)
        // 只保留失败分支。
        .err()
        // 使用显式 panic 保留调用上下文。
        .unwrap_or_else(|| panic!("catalog field validation must fail"))
}

// 构造具备全部必填外壳的 app.create 请求。
fn app_create_request(input: Value) -> CommandRequest {
    // 从已确认标准请求开始。
    let mut request = request("app", "create", IsolationRequirement::Standard);
    // 写入非空 opaque session ID 外壳，精确语义由后续边界负责。
    request
        // 访问目标对象。
        .target
        // 插入必填目标。
        .insert("sessionId".to_owned(), json!("s2:a:test"));
    // 写入已登记版本化 capability ID。
    request.args.insert(
        // 使用固定参数名。
        "capability".to_owned(),
        // 使用 App surface 创建 capability。
        json!(capabilities::IMAGE_CANVAS_CREATE),
    );
    // 写入调用方测试输入。
    request
        // 访问参数对象。
        .args
        // 插入必填 input。
        .insert("input".to_owned(), input);
    // 返回尚未进入 provider 的请求。
    request
}

// 验证强类型字段仍逐字序列化既有 catalog 类型文本。
#[test]
fn catalog_field_types_preserve_public_serialization() -> AppResult<()> {
    // 读取带完整范围说明的录制 operation。
    let record = catalog::operation("desktop", "record").ok_or_else(|| {
        // 静态目录缺失属于测试夹具错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "desktop.record must exist")
    })?;
    // 找到 MP4 路径字段。
    let path = record
        // 遍历参数字段。
        .argument_fields
        // 创建迭代器。
        .iter()
        // 按稳定字段名查找。
        .find(|field| field.name == "path")
        // 缺失字段属于目录漂移。
        .ok_or_else(|| AppControlError::new("TEST_FIXTURE_MISSING", "path must exist"))?;
    // 路径展示文本必须逐字不变。
    assert_eq!(json!(path.value_type), "string (.mp4)");
    // 找到带整数范围的时长字段。
    let duration = record
        // 遍历参数字段。
        .argument_fields
        // 创建迭代器。
        .iter()
        // 按稳定字段名查找。
        .find(|field| field.name == "durationMs")
        // 缺失字段属于目录漂移。
        .ok_or_else(|| AppControlError::new("TEST_FIXTURE_MISSING", "durationMs must exist"))?;
    // 整数范围说明必须逐字不变。
    assert_eq!(
        json!(duration.value_type),
        "integer (1000..=300000; default 30000)"
    );
    // 找到带数值范围的变化阈值字段。
    let threshold = record
        // 遍历参数字段。
        .argument_fields
        // 创建迭代器。
        .iter()
        // 按稳定字段名查找。
        .find(|field| field.name == "changeThreshold")
        // 缺失字段属于目录漂移。
        .ok_or_else(|| {
            AppControlError::new("TEST_FIXTURE_MISSING", "changeThreshold must exist")
        })?;
    // 数值范围说明必须逐字不变。
    assert_eq!(
        json!(threshold.value_type),
        "number (0.005..=0.5; default 0.035)"
    );
    // 读取 App generic operation 的语义字段类型。
    let create = catalog::operation("app", "create").ok_or_else(|| {
        // 静态目录缺失属于测试夹具错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "app.create must exist")
    })?;
    // 语义 ID 与对象文本也必须保持不变。
    assert_eq!(
        // 序列化全部参数字段便于锁定顺序和值。
        json!(create.argument_fields),
        // 使用迁移前相同 JSON 形状。
        json!([
            {
                "name": "capability",
                "value_type": "versioned-capability-id",
                "required": true
            },
            {
                "name": "input",
                "value_type": "object",
                "required": true
            }
        ])
    );
    // 返回兼容验证成功。
    Ok(())
}

// 验证 app.run 必填 input 的错误 JSON 类型在 provider 前失败。
#[test]
fn catalog_rejects_non_object_app_input() {
    // 使用字符串冒充 capability input 对象。
    let request = app_create_request(json!("not-an-object"));
    // 执行通用 Policy 类型门禁。
    let error = validation_error(&request);
    // 使用稳定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 只输出字段路径和公开类型，不回显值。
    assert_eq!(error.message, "args.input 必须是 object。");
    // 不附加调用值详情。
    assert_eq!(error.details, Value::Null);
}

// 验证缺失和空白必填值保持既有错误兼容。
#[test]
fn catalog_preserves_required_field_errors() {
    // 构造合法输入后删除精确目标。
    let mut missing = app_create_request(json!({}));
    // 移除整个目标字段。
    missing.target.remove("sessionId");
    // 执行缺失字段门禁。
    let missing_error = validation_error(&missing);
    // 保持既有错误码。
    assert_eq!(missing_error.code, "INVALID_ARGUMENT");
    // 保持既有必填消息。
    assert_eq!(missing_error.message, "target.sessionId 是必填字段。");
    // 构造空白字符串目标。
    let mut blank = app_create_request(json!({}));
    // 用空白值替换 session ID。
    blank
        // 访问目标对象。
        .target
        // 写入空白字符串。
        .insert("sessionId".to_owned(), json!("   "));
    // 执行空白字段门禁。
    let blank_error = validation_error(&blank);
    // 空白值继续视为缺失。
    assert_eq!(blank_error.message, "target.sessionId 是必填字段。");
}

// 验证已提供的可选整数与布尔字段由 catalog 同源校验。
#[test]
fn catalog_rejects_wrong_optional_scalar_types() {
    // 构造完整桌面截图请求。
    let mut screenshot = request("desktop", "screenshot", IsolationRequirement::Standard);
    // 写入非空精确目标外壳。
    screenshot
        // 访问目标对象。
        .target
        // 插入必填 session ID。
        .insert("sessionId".to_owned(), json!("s2:w:test"));
    // 写入不会在类型门禁前访问的 PNG 路径。
    screenshot
        // 访问参数对象。
        .args
        // 插入必填路径。
        .insert("path".to_owned(), json!("C:\\catalog-field-test.png"));
    // 使用字符串冒充整数 timeout。
    screenshot
        // 访问参数对象。
        .args
        // 插入错误类型。
        .insert("timeoutMs".to_owned(), json!("250"));
    // 类型错误必须早于路径和 provider 行为。
    let integer_error = validation_error(&screenshot);
    // 锁定稳定类型消息。
    assert_eq!(integer_error.message, "args.timeoutMs 必须是 integer。");
    // 把错误类型替换为超过窗口截图上界的整数。
    screenshot
        // 访问参数对象。
        .args
        // 插入基础类型正确但范围错误的值。
        .insert("timeoutMs".to_owned(), json!(30_001));
    // Catalog 范围门禁必须替代原 operation 手写分支。
    let range_error = validation_error(&screenshot);
    // 锁定由 catalog 生成的公开范围。
    assert_eq!(
        range_error.message,
        "args.timeoutMs 必须满足 integer (250..=30000)。"
    );
    // 移除错误范围以测试下一个可选字段。
    screenshot.args.remove("timeoutMs");
    // 使用字符串冒充布尔覆盖许可。
    screenshot
        // 访问参数对象。
        .args
        // 插入错误类型。
        .insert("overwrite".to_owned(), json!("true"));
    // 类型错误必须早于覆盖检查。
    let boolean_error = validation_error(&screenshot);
    // 锁定稳定类型消息。
    assert_eq!(boolean_error.message, "args.overwrite 必须是 boolean。");
}

// 验证 catalog 数值范围在专属 Module 和 provider 前统一失败。
#[test]
fn catalog_rejects_out_of_range_numeric_arguments() {
    // 构造不涉及文件系统的 Standard Edit 请求。
    let mut set_text = request(
        // 使用已认证后台文本 surface。
        "win32-control",
        // 使用公开文本 operation。
        "set-text",
        // 使用标准隔离要求。
        IsolationRequirement::Standard,
    );
    // 写入精确控件目标外壳。
    set_text
        // 访问目标对象。
        .target
        // 插入必填 session ID。
        .insert("sessionId".to_owned(), json!("s2:c:test"));
    // 写入必填文本。
    set_text
        // 访问参数对象。
        .args
        // 插入合法文本。
        .insert("text".to_owned(), json!("hello"));
    // 写入超过 catalog 上界的 deadline。
    set_text
        // 访问参数对象。
        .args
        // 插入基础类型正确但范围错误的整数。
        .insert("timeoutMs".to_owned(), json!(30_001));
    // 通用 Policy 必须在 Module 前拒绝该值。
    let integer_error = validation_error(&set_text);
    // 锁定稳定参数错误码。
    assert_eq!(integer_error.code, "INVALID_ARGUMENT");
    // 诊断必须来自同一强类型闭区间。
    assert_eq!(
        integer_error.message,
        "args.timeoutMs 必须满足 integer (1..=30000)。"
    );

    // 构造具备完整外壳的桌面录制请求。
    let mut record = request("desktop", "record", IsolationRequirement::Standard);
    // 写入精确窗口目标外壳。
    record
        // 访问目标对象。
        .target
        // 插入必填 session ID。
        .insert("sessionId".to_owned(), json!("s2:w:test"));
    // 写入只供 catalog 校验的 MP4 路径。
    record
        // 访问参数对象。
        .args
        // 插入基础类型正确的必填路径。
        .insert("path".to_owned(), json!("C:\\catalog-range-test.mp4"));
    // 写入超过 catalog 上界的变化阈值。
    record
        // 访问参数对象。
        .args
        // 插入基础类型正确但范围错误的数值。
        .insert("changeThreshold".to_owned(), json!(0.6));
    // 通用 Policy 必须早于 Recording Module 路径访问拒绝该值。
    let number_error = validation_error(&record);
    // 锁定稳定参数错误码。
    assert_eq!(number_error.code, "INVALID_ARGUMENT");
    // 诊断必须来自同一强类型闭区间。
    assert_eq!(
        number_error.message,
        "args.changeThreshold 必须满足 number (0.005..=0.5)。"
    );

    // 构造浏览器截图请求以覆盖原手写 timeout 分支。
    let mut browser = request("browser", "screenshot", IsolationRequirement::Standard);
    // 写入合法 URL 目标外壳。
    browser
        // 访问目标对象。
        .target
        // 插入必填 URL。
        .insert("url".to_owned(), json!("https://example.test"));
    // 写入必填输出路径。
    browser
        // 访问参数对象。
        .args
        // 该路径不会在范围门禁前被访问。
        .insert("path".to_owned(), json!("C:\\catalog-browser-test.png"));
    // 写入低于 catalog 下界的 timeout。
    browser
        // 访问参数对象。
        .args
        // 插入基础类型正确但范围错误的整数。
        .insert("timeoutMs".to_owned(), json!(999));
    // 通用 Policy 必须替代原 operation 手写范围判断。
    let browser_error = validation_error(&browser);
    // 锁定由 catalog 生成的公开范围。
    assert_eq!(
        browser_error.message,
        "args.timeoutMs 必须满足 integer (1000..=300000)。"
    );
}

// 验证字符串数组不得混入其他 JSON 类型。
#[test]
fn catalog_rejects_mixed_string_array() {
    // 构造完整桌面启动请求。
    let mut launch = request("desktop", "launch", IsolationRequirement::Standard);
    // 写入固定可执行路径外壳。
    launch
        // 访问参数对象。
        .args
        // 插入必填字符串。
        .insert("path".to_owned(), json!("C:\\fixed.exe"));
    // 在 argv 中混入数字。
    launch
        // 访问参数对象。
        .args
        // 插入错误数组。
        .insert("argv".to_owned(), json!(["--safe", 1]));
    // 通用 Policy 必须拒绝混合数组。
    let error = validation_error(&launch);
    // 使用稳定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 输出公开字符串数组类型。
    assert_eq!(error.message, "args.argv 必须是 array<string>。");
}

// 验证正确 JSON 类型继续通过通用 Policy。
#[test]
fn catalog_accepts_valid_app_field_types() {
    // 使用包含 capability 私有字段的对象证明外层 Policy 不穿透。
    let request = app_create_request(json!({ "moduleOwnedField": true }));
    // 通用 Policy 不应替代后续 capability 领域语义。
    assert!(validate(&request).is_ok());
}

// 验证 catalog 补齐实现已读取的三个可选字段契约。
#[test]
fn catalog_advertises_existing_optional_runtime_fields() -> AppResult<()> {
    // 读取 Standard Edit 兼容 operation。
    let set_text = catalog::operation("win32-control", "set-text").ok_or_else(|| {
        // 静态目录缺失属于测试夹具错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "win32-control.set-text must exist")
    })?;
    // 序列化完整参数字段并锁定新增公开说明。
    assert_eq!(
        // 使用正式 catalog 序列化路径。
        json!(set_text.argument_fields),
        // text 与实现已读取的 timeout 必须同时公开。
        json!([
            {
                "name": "text",
                "value_type": "string",
                "required": true
            },
            {
                "name": "timeoutMs",
                "value_type": "integer (1..=30000; default 2000)",
                "required": false
            }
        ])
    );
    // 读取复用同一 Standard Edit 契约的桌面 operation。
    let type_text = catalog::operation("desktop", "type-text").ok_or_else(|| {
        // 静态目录缺失属于测试夹具错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "desktop.type-text must exist")
    })?;
    // 两条兼容 surface 必须投影同一完整字段集合。
    assert_eq!(
        // 序列化桌面兼容参数。
        json!(type_text.argument_fields),
        // 核对 Standard Edit 公共参数。
        json!(set_text.argument_fields)
    );
    // 读取前台按键 operation。
    let press_key = catalog::operation("desktop", "press-key").ok_or_else(|| {
        // 静态目录缺失属于测试夹具错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "desktop.press-key must exist")
    })?;
    // 锁定按键完整参数集合及约束文本。
    assert_eq!(
        // 使用正式 catalog 序列化路径。
        json!(press_key.argument_fields),
        // 输出实现已经认证的三个参数。
        json!([
            {
                "name": "key",
                "value_type": "string",
                "required": true
            },
            {
                "name": "holdMs",
                "value_type": "integer (0..=5000; default 0)",
                "required": false
            },
            {
                "name": "phase",
                "value_type": "string (press|down|up; default press)",
                "required": false
            }
        ])
    );
    // 返回 catalog 覆盖成功。
    Ok(())
}

// 验证每个 operation 的 target 与 args 都形成无重复字段集合。
#[test]
fn catalog_operation_field_sets_are_unique() {
    // 遍历全部公开 surface。
    for app in catalog::apps() {
        // 遍历 surface 的全部 run operation。
        for operation in app.operations {
            // 分别核对 target 与 args，允许两组之间复用语义名称。
            for (group, fields) in [
                // 核对目标集合。
                ("target", operation.target_fields),
                // 核对参数集合。
                ("args", operation.argument_fields),
            ] {
                // 保存当前组已见字段名。
                let mut names = std::collections::BTreeSet::new();
                // 逐项核对稳定名称。
                for field in fields {
                    // 字段名不得为空。
                    assert!(!field.name.is_empty());
                    // 同组重复会使 allowedFields 和校验顺序产生歧义。
                    assert!(
                        names.insert(field.name),
                        "{}.{} contains duplicate {group} field {}",
                        app.id,
                        operation.operation,
                        field.name
                    );
                }
            }
        }
    }
}

// 验证补齐的可选字段继续通过通用 Policy。
#[test]
fn catalog_accepts_existing_optional_runtime_fields() {
    // 构造 Standard Edit 兼容请求。
    let mut set_text = request(
        // 使用固定消息兼容 surface。
        "win32-control",
        // 使用唯一公开 operation。
        "set-text",
        // 使用标准后台优先要求。
        IsolationRequirement::Standard,
    );
    // 写入精确目标外壳。
    set_text
        // 访问目标对象。
        .target
        // 插入必填 session ID。
        .insert("sessionId".to_owned(), json!("s2:c:test"));
    // 写入必填文本。
    set_text
        // 访问参数对象。
        .args
        // 插入文本。
        .insert("text".to_owned(), json!("hello"));
    // 写入实现已读取的可选 deadline。
    set_text
        // 访问参数对象。
        .args
        // 插入合法整数。
        .insert("timeoutMs".to_owned(), json!(2_000));
    // Catalog 补齐后该合法调用必须继续通过。
    assert!(validate(&set_text).is_ok());
    // 构造显式前台按键请求。
    let mut press_key = request(
        // 使用通用桌面 surface。
        "desktop",
        // 使用前台按键 operation。
        "press-key",
        // 使用标准要求。
        IsolationRequirement::Standard,
    );
    // 写入精确窗口目标外壳。
    press_key
        // 访问目标对象。
        .target
        // 插入必填 session ID。
        .insert("sessionId".to_owned(), json!("s2:w:test"));
    // 写入必填按键。
    press_key
        // 访问参数对象。
        .args
        // 插入按键值。
        .insert("key".to_owned(), json!("ctrl+s"));
    // 写入合法按住时长。
    press_key
        // 访问参数对象。
        .args
        // 插入有界整数。
        .insert("holdMs".to_owned(), json!(10));
    // 写入与按住时长兼容的完整按键阶段。
    press_key
        // 访问参数对象。
        .args
        // 插入封闭枚举文本。
        .insert("phase".to_owned(), json!("press"));
    // Catalog 补齐后合法调用必须继续通过。
    assert!(validate(&press_key).is_ok());
}

// 验证未知 args 在 provider 前失败且不泄漏调用方字段或值。
#[test]
fn catalog_rejects_unknown_args_without_echoing_input() {
    // 从完整合法 App 调用开始。
    let mut request = app_create_request(json!({}));
    // 注入不在外层 catalog 中的敏感命名字段。
    request
        // 访问参数对象。
        .args
        // 写入测试期敏感值。
        .insert("privateToken".to_owned(), json!("must-not-leak"));
    // 执行通用 Policy 封闭门禁。
    let error = validation_error(&request);
    // 使用稳定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 只指出 args 外层包含未知字段。
    assert_eq!(
        error.message,
        "args 包含不在该 operation 公开 catalog 中的字段。"
    );
    // 只输出公开字段组和允许集合。
    assert_eq!(
        error.details,
        json!({
            "fieldGroup": "args",
            "allowedFields": ["capability", "input"]
        })
    );
    // 错误 envelope 不得回显未知字段名。
    assert!(!error.details.to_string().contains("privateToken"));
    // 错误 envelope 不得回显未知值。
    assert!(!error.details.to_string().contains("must-not-leak"));
}

// 验证未知 target 与无目标 operation 都使用 catalog 允许集合。
#[test]
fn catalog_rejects_unknown_target_for_declared_and_empty_sets() {
    // 从完整合法 App 调用开始。
    let mut app = app_create_request(json!({}));
    // 注入未声明的模糊标题目标。
    app.target
        .insert("title".to_owned(), json!("private-title"));
    // 执行通用 Policy 封闭门禁。
    let app_error = validation_error(&app);
    // 只输出精确 session 与可选独立会话 endpoint 字段。
    assert_eq!(
        app_error.details,
        json!({
            "fieldGroup": "target",
            "allowedFields": ["sessionId", "interactiveSessionId"]
        })
    );
    // 构造不接受任何 target 的 Notepad 创建请求。
    let mut notepad = request(
        // 使用固定 Notepad surface。
        "notepad",
        // 使用唯一公开 operation。
        "open-and-write-text",
        // 使用标准要求。
        IsolationRequirement::Standard,
    );
    // 写入必填文本参数。
    notepad
        // 访问参数对象。
        .args
        // 插入合法文本。
        .insert("text".to_owned(), json!("hello"));
    // 注入不允许的 target。
    notepad
        // 访问目标对象。
        .target
        // 写入测试值。
        .insert("sessionId".to_owned(), json!("not-allowed"));
    // 无字段集合也必须失败闭合。
    let notepad_error = validation_error(&notepad);
    // 公开允许集合必须明确为空。
    assert_eq!(
        notepad_error.details,
        json!({
            "fieldGroup": "target",
            "allowedFields": []
        })
    );
}

// 验证必填和类型错误继续优先于未知字段门禁。
#[test]
fn catalog_preserves_declared_field_failure_order() {
    // 构造缺失必填 session 且含未知 target 的请求。
    let mut missing = app_create_request(json!({}));
    // 移除必填精确目标。
    missing.target.remove("sessionId");
    // 添加未知字段。
    missing.target.insert("title".to_owned(), json!("ignored"));
    // 必填错误必须保持优先。
    let missing_error = validation_error(&missing);
    // 保持既有必填消息。
    assert_eq!(missing_error.message, "target.sessionId 是必填字段。");
    // 构造已声明字段类型错误且含未知 args 的请求。
    let mut wrong_type = app_create_request(json!("not-object"));
    // 添加未知外层字段。
    wrong_type.args.insert("extra".to_owned(), json!(true));
    // 类型错误必须保持优先。
    let type_error = validation_error(&wrong_type);
    // 保持既有同源类型消息。
    assert_eq!(type_error.message, "args.input 必须是 object。");
}

// 严格模式必须在 target 或 provider 解析前拒绝同会话写。
#[test]
fn strict_mode_rejects_same_session_route_before_target_resolution() {
    // 构造缺少 target 的确认请求。
    let request = request(
        // 使用固定消息兼容 surface。
        "win32-control",
        // 使用固定消息 operation。
        "set-text",
        // 要求严格隔离。
        IsolationRequirement::Strict,
    );
    // 执行纯策略校验。
    let error = validate(&request).err();
    // 必须优先返回严格隔离错误而非 target 或前台错误。
    assert_eq!(
        error.as_ref().map(|value| value.code),
        Some("ISOLATION_REQUIRED")
    );
    // 证据必须明确忽略已经提供的前台同意。
    assert_eq!(
        error.map(|value| value.details["foregroundConsentIgnored"].clone()),
        Some(Value::Bool(true))
    );
}

// 严格模式不得因已有前台同意而允许原生输入。
#[test]
fn strict_mode_never_reuses_foreground_consent() {
    // 构造已确认且已同意前台的按键请求。
    let request = request(
        // 使用通用桌面 surface。
        "desktop",
        // 使用前台按键 operation。
        "press-key",
        // 要求严格隔离。
        IsolationRequirement::Strict,
    );
    // 执行纯策略校验。
    let error = validate(&request).err();
    // 必须返回隔离要求而不是继续到输入路径。
    assert_eq!(error.map(|value| value.code), Some("ISOLATION_REQUIRED"));
}

// 精确窗口录制必须使用固定认证 worker 计划。
#[test]
fn recording_uses_certified_isolated_worker_route() {
    // 构造已经迁入认证 worker 的录制请求。
    let request = request(
        // 使用通用桌面 surface。
        "desktop",
        // 使用录制 operation。
        "record",
        // 要求严格隔离。
        IsolationRequirement::Strict,
    );
    // 路由必须固定到录制 companion，且不依赖当前构建产物是否存在。
    assert_eq!(
        // 读取纯路由决策。
        isolated_worker_file_name(&request),
        // 核对固定 Rust worker 文件名。
        Some(RECORDING_WORKER_FILE_NAME)
    );
}

// 完整认证且可达的隔离 worker 状态允许严格执行。
#[test]
fn strict_state_accepts_certified_available_worker() {
    // 构造不访问文件系统的认证计划。
    let plan = plan(
        // 要求隔离 worker。
        ExecutionRealm::IsolatedWorker,
        // 实际也进入隔离 worker。
        ExecutionRealm::IsolatedWorker,
        // 路由已认证。
        true,
        // companion 可达。
        true,
    );
    // 使用不会进入 provider 的请求外壳。
    let request = request("app", "read", IsolationRequirement::Strict);
    // 状态机应允许继续。
    assert!(enforce_strict_isolation(&request, plan).is_ok());
}

// 已认证的隔离路由在 companion 缺失时仍必须失败闭合。
#[test]
fn strict_state_rejects_missing_certified_worker() {
    // 构造路由已认证但 companion 不可达的计划。
    let plan = plan(
        // 要求隔离 worker。
        ExecutionRealm::IsolatedWorker,
        // 计划仍指向隔离 worker。
        ExecutionRealm::IsolatedWorker,
        // 路由已通过认证。
        true,
        // companion 当前缺失。
        false,
    );
    // 使用纯请求外壳。
    let request = request("app", "read", IsolationRequirement::Strict);
    // 执行纯状态机门禁。
    let error = enforce_strict_isolation(&request, plan).err();
    // 必须返回 worker 不可用而不是继续执行。
    assert_eq!(
        error.map(|value| value.code),
        Some("ISOLATED_WORKER_UNAVAILABLE")
    );
}

// 主机无头域无需伪装成 worker 即可满足严格零打扰。
#[test]
fn strict_state_accepts_certified_host_headless_route() {
    // 构造主机无头计划。
    let plan = plan(
        // 要求主机无头域。
        ExecutionRealm::HostHeadless,
        // 实际进入主机无头域。
        ExecutionRealm::HostHeadless,
        // 无需 worker 路由认证。
        false,
        // 无需 companion。
        false,
    );
    // 使用纯请求外壳。
    let request = request("app", "read", IsolationRequirement::Strict);
    // 状态机应允许继续。
    assert!(enforce_strict_isolation(&request, plan).is_ok());
}

// UIA 精确 inspect 必须使用已认证 observation worker 计划。
#[test]
fn uia_inspect_uses_certified_observation_worker_plan() -> AppResult<()> {
    // 构造 UIA 精确检查请求。
    let request = CommandRequest::read(Verb::Inspect, "uia");
    // 解析公开 UIA descriptor。
    let descriptor = catalog::app("uia").ok_or_else(|| {
        // 把固定目录缺失转为结构化测试错误。
        AppControlError::new("TEST_FIXTURE_MISSING", "uia descriptor must exist")
    })?;
    // 构造不启动 companion 的读取计划。
    let plan = build_read_execution_plan(&request, descriptor);
    // 契约与真实运行域都必须是隔离 worker。
    assert_eq!(plan.required_realm, ExecutionRealm::IsolatedWorker);
    // 当前只读路线必须标记为已认证。
    assert!(plan.isolated_route_certified);
    // 返回计划一致成功。
    Ok(())
}

// 标准兼容结果必须如实区分要求域与实际域。
#[test]
fn result_attestation_exposes_uncertified_compatibility_realm() -> AppResult<()> {
    // 构造正式要求隔离但实际仍在主机后台的计划。
    let mut plan = plan(
        // 正式契约要求隔离 worker。
        ExecutionRealm::IsolatedWorker,
        // 当前兼容实现位于主机后台。
        ExecutionRealm::HostBackground,
        // 路由尚未认证。
        false,
        // companion 不可用。
        false,
    );
    // 标记该测试覆盖标准兼容请求。
    plan.isolation_requirement = IsolationRequirement::Standard;
    // 派生标准主机影响策略。
    plan.host_impact_policy = HostImpactPolicy::BackgroundPreferred;
    // 构造最小成功结果。
    let mut result = json!({ "ok": true });
    // 附加 System 证明。
    plan.attest_result(&mut result)?;
    // 实际域不得谎报为隔离 worker。
    assert_eq!(result["executionRealm"], "host-background");
    // 同时保留正式要求域。
    assert_eq!(result["requiredExecutionRealm"], "isolated-worker");
    // 未认证必须显式为 false。
    assert_eq!(result["executionRealmCertified"], false);
    // 测试成功。
    Ok(())
}

// provider 自报冲突 realm 必须使 System fail closed。
#[test]
fn result_attestation_rejects_provider_realm_conflict() {
    // 构造一致主机后台计划。
    let mut plan = plan(
        // 要求主机后台。
        ExecutionRealm::HostBackground,
        // 实际主机后台。
        ExecutionRealm::HostBackground,
        // 无隔离 worker。
        false,
        // 无 companion。
        false,
    );
    // 使用标准兼容要求。
    plan.isolation_requirement = IsolationRequirement::Standard;
    // 使用标准主机策略。
    plan.host_impact_policy = HostImpactPolicy::BackgroundPreferred;
    // 模拟 provider 错报前台域。
    let mut result = json!({ "ok": true, "executionRealm": "host-foreground" });
    // 执行 System 证明。
    let error = plan.attest_result(&mut result).err();
    // 必须拒绝域漂移。
    assert_eq!(error.map(|value| value.code), Some("OPERATION_FAILED"));
}
