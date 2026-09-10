use serde_json::{Map, Value, json};

// 把错误码实现保留为 CLI 适配边界的普通私有类型。
mod error_code;
// 导入当前 CLI 私有封闭错误码。
use error_code::CliErrorCode;

use crate::{
    capabilities,
    // 导入请求、结果、动词与强类型隔离要求。
    domain::{AppResult, CommandRequest, IsolationRequirement, JsonMap, Verb},
    service::{AppControlService, SequenceInput},
};

// 导入 CLI 私有的有界 JSON 来源适配器。
mod json_input;
// 只把 JSON 来源读取函数带入当前解析作用域。
use json_input::read_json;
// 注册长操作 handle-only CLI 路由。
mod long_operation;

pub struct CliOutput {
    pub json: Value,
    pub pretty: bool,
    pub exit_code: i32,
}

pub fn run(argv: Vec<String>) -> AppResult<CliOutput> {
    let (positionals, option_tokens) = split_arguments(&argv);
    let options = parse_options(option_tokens)?;
    let service = AppControlService::new();
    let command = positionals.first().map(String::as_str).unwrap_or("help");
    let result = match command {
        "help" => help_json(),
        "version" | "--version" | "-v" => {
            json!({ "ok": true, "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") })
        }
        // 路由只读 Rust 构建元数据。
        "build-info" => {
            // 禁止位置参数污染封闭命令。
            if positionals.len() != 1 {
                // 返回稳定参数错误。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 与 C++ 对照错误语义保持一致。
                    "build-info does not accept positional arguments.",
                ));
            }
            // 委托 System 协调 Module。
            service.build_info()
        }
        // 路由独立 capability 迁移元数据查询。
        "capabilities" => {
            // method 子命令查询旧 method 的 C++ 状态。
            if positionals.get(1).is_some_and(|value| value == "method") {
                // method 最多接受一个精确 ID。
                if positionals.len() > 3 {
                    // 返回与 C++ 对照一致的参数错误。
                    return Err(CliErrorCode::InvalidArgument.error(
                        // 保持跨实现稳定消息。
                        "capabilities method accepts at most one method ID.",
                    ));
                }
                // 委托 System 协调 method 状态 Module。
                service.method_capabilities(positionals.get(2).map(String::as_str))?
            // descriptor 子命令查询 app 或 operation 的 C++ 状态。
            } else if positionals
                // 读取第一个子命令。
                .get(1)
                // 只接受 descriptor 文本。
                .is_some_and(|value| value == "descriptor")
            {
                // descriptor 必须包含 app 且最多再带一个 operation。
                if !(3..=4).contains(&positionals.len()) {
                    // 返回与 C++ 对照一致的参数错误。
                    return Err(CliErrorCode::InvalidArgument.error(
                        // 保持跨实现稳定消息。
                        "capabilities descriptor requires <app> [operation].",
                    ));
                }
                // 读取已由长度门禁证明存在的 app ID。
                let app_id = &positionals[2];
                // 委托 System 协调 descriptor 状态 Module。
                service.descriptor_capabilities(
                    // 转发精确 app ID。
                    app_id,
                    // 转发可选 operation ID。
                    positionals.get(3).map(String::as_str),
                )?
            } else {
                // surface 查询最多接受一个 C++ 兼容别名。
                let valid_surface = positionals.len() <= 2
                    // 缺省 surface 合法。
                    && positionals.get(1).is_none_or(|surface| {
                        // 只接受 C++ 对照实现的封闭别名集合。
                        matches!(
                            surface.as_str(),
                            // 应用和窗口别名汇总到 app surface。
                            "app"
                                | "application"
                                | "window"
                                // 媒体别名汇总到 app surface。
                                | "media-session"
                                | "media"
                                // 进程别名汇总到 app surface。
                                | "process"
                                // 可访问性别名汇总到 app surface。
                                | "uia"
                                | "accessibility"
                        )
                    });
                // 非法 surface 或额外参数必须失败闭合。
                if !valid_surface {
                    // 返回与 C++ 对照一致的参数错误。
                    return Err(CliErrorCode::InvalidArgument.error(
                        // 保持跨实现稳定消息。
                        "capabilities accepts one migrated C++ surface.",
                    ));
                }
                // 委托 System 返回封闭 surface 投影。
                service.capability_surface()
            }
        }
        "catalog" | "apps" => service.catalog(positionals.get(1).map(String::as_str))?,
        // 路由固定 broker 的长操作 status/await Query 与 cancel Command。
        "operation" => long_operation::route(
            // 传递位置参数供封闭 action/handle 解析。
            positionals,
            // 传递原 option token 供禁止扩展门禁。
            option_tokens,
            // 传递已经有界解析的 timeout。
            options.timeout_ms,
            // 传递 provider-neutral 精确目标。
            &options.target,
            // 传递已经有界读取的 JSON 输入。
            options.input.as_ref(),
            // 传递逐操作显式确认。
            options.confirmed,
            // 借用当前 System。
            &service,
        )?,
        "methods" => service.methods(positionals.get(1).map(String::as_str))?,
        "describe" => {
            let app = positional(positionals, 1, "describe 需要 <app>。")?;
            service.describe(app, positionals.get(2).map(String::as_str))?
        }
        "doctor" => service.doctor(positionals.get(1).map(String::as_str)),
        // 路由精确窗口的零帧、零文件捕获预检。
        "preflight-capture" => {
            // 读取 C++ 兼容 app surface。
            let surface = positional(positionals, 1, "preflight-capture requires app.")?;
            // 只允许统一 app surface 且不接受额外位置参数。
            if surface != "app" || positionals.len() != 2 {
                // 保持 C++ 的 capability gap 语义。
                return Err(CliErrorCode::CapabilityGap.error(
                    // 不暗示其他 surface 会被静默接受。
                    "Capture preflight is only published for application windows.",
                ));
            }
            // 读取 canonical opaque 窗口目标。
            let session_id = options
                // 访问解析后的目标对象。
                .target
                // 只接受 sessionId。
                .get("sessionId")
                // 要求字符串值。
                .and_then(Value::as_str)
                // 拒绝空字符串。
                .filter(|value| !value.is_empty())
                // 缺失时返回 C++ 兼容参数错误。
                .ok_or_else(|| {
                    // 构造稳定错误。
                    CliErrorCode::InvalidArgument.error(
                        // 保持跨实现稳定消息。
                        "preflight-capture requires --target sessionId=<opaque>.",
                    )
                })?;
            // 委托 System 协调只读预检 Module。
            service.preflight_capture(session_id)?
        }
        // 路由精确窗口的确认式首帧元数据探针。
        "probe-capture-frame" => {
            // 读取 app surface，不接触目标。
            let surface = positional(positionals, 1, "probe-capture-frame requires app.")?;
            // 只允许统一 app surface 且不接受额外位置参数。
            if surface != "app" || positionals.len() != 2 {
                // 返回稳定 capability gap。
                return Err(CliErrorCode::CapabilityGap.error(
                    // 不暗示其他 surface 会被接受。
                    "Frame metadata capture is only published for application windows.",
                ));
            }
            // 确认必须先于目标字段读取。
            if !options.confirmed {
                // 缺少确认时立即失败。
                return Err(CliErrorCode::ConfirmationRequired.error(
                    // 不回显目标。
                    "Frame metadata capture requires explicit confirmation.",
                ));
            }
            // 在确认通过后读取 canonical opaque 窗口目标。
            let session_id = options
                // 访问解析后的目标对象。
                .target
                // 只接受 sessionId。
                .get("sessionId")
                // 要求字符串值。
                .and_then(Value::as_str)
                // 拒绝空字符串。
                .filter(|value| !value.is_empty())
                // 缺失时返回稳定参数错误。
                .ok_or_else(|| {
                    // 构造安全错误。
                    CliErrorCode::InvalidArgument.error(
                        // 不公开其他 target 字段。
                        "probe-capture-frame requires --target sessionId=<opaque>.",
                    )
                })?;
            // 委托 System 协调隔离首帧探针 Module。
            service.probe_capture_frame(session_id, true, options.timeout_ms)?
        }
        // 路由完整应用关系图发现纵切。
        "discover" => {
            // 只接受明确的 app 或 isolation surface。
            let surface = positional(positionals, 1, "discover 需要 app 或 isolation。")?;
            // 禁止未知 surface 或额外位置参数。
            if !matches!(surface, "app" | "isolation") || positionals.len() != 2 {
                // 返回稳定参数错误。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 提供封闭公开用法。
                    "discover 只接受 app 或 isolation surface。",
                ));
            }
            // 按公开 surface 委托唯一 System 入口。
            if surface == "app" {
                // 调用 Rust 主实现的应用关系图纵切。
                service.discover_app(
                    // 转发应用边界。
                    options.max_applications,
                    // 转发进程边界。
                    options.max_processes,
                    // 转发窗口边界。
                    options.max_windows,
                )?
            } else {
                // 只返回完成双向认证的独立会话 endpoint。
                service.discover_interactive_sessions()?
            }
        }
        // 路由无副作用的 Rust capability assessment。
        "assess" => {
            // 只接受 generic app surface。
            let surface = positional(positionals, 1, "assess 需要 app。")?;
            // 禁止未知 surface 或额外位置参数。
            if surface != "app" || positionals.len() != 2 {
                // 返回稳定参数错误。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 给出唯一公开用法。
                    "assess 只接受 app surface。",
                ));
            }
            // 读取版本化 capability ID。
            let capability = options
                // 访问解析后的 capability 选项。
                .capability
                // 借用字符串而不转移 options。
                .as_deref()
                // 拒绝空值。
                .filter(|value| !value.is_empty())
                // 缺失时返回参数错误。
                .ok_or_else(|| {
                    // 构造稳定错误。
                    CliErrorCode::InvalidArgument.error(
                        // 明确需要版本化 ID。
                        "assess requires --capability <capability@version>.",
                    )
                })?;
            // capability 外壳必须满足公共命名契约。
            if !capabilities::is_versioned_capability_id(capability) {
                // 非 canonical ID 不进入目标重新发现。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 不接受零版本或宽松别名。
                    "--capability must be a canonical versioned capability ID.",
                ));
            }
            // 读取精确 opaque session。
            let session_id = options
                // 访问目标对象。
                .target
                // 读取 sessionId。
                .get("sessionId")
                // 要求字符串。
                .and_then(Value::as_str)
                // 拒绝空值。
                .filter(|value| !value.is_empty())
                // 缺失时返回参数错误。
                .ok_or_else(|| {
                    // 构造稳定错误。
                    CliErrorCode::InvalidArgument.error(
                        // 明确只接受 opaque session。
                        "assess requires --target sessionId=<opaque>.",
                    )
                })?;
            // 委托 Module 完成重新发现与封闭决策。
            service.assess_capability(capability, session_id)?
        }
        "status" => match positionals.get(1) {
            Some(app) => service.execute(build_request(
                Verb::Status,
                app,
                None,
                options.input.as_ref(),
                &options,
            )?)?,
            None => service.doctor(None),
        },
        "sessions" => {
            let app = positional(positionals, 1, "sessions 需要 <app>。")?;
            service.execute(build_request(
                Verb::Sessions,
                app,
                None,
                options.input.as_ref(),
                &options,
            )?)?
        }
        "inspect" => {
            let app = positional(positionals, 1, "inspect 需要 <app>。")?;
            service.execute(build_request(
                Verb::Inspect,
                app,
                None,
                options.input.as_ref(),
                &options,
            )?)?
        }
        // 路由隔离的有界可访问性树读取。
        "inspect-tree" => {
            // 读取 legacy 兼容 surface。
            let app = positional(
                positionals,
                1,
                "inspect-tree 需要 <app|uia|accessibility>。",
            )?;
            // 接受 generic app surface 与两个只读别名，并拒绝额外位置参数。
            if !matches!(app, "app" | "uia" | "accessibility") || positionals.len() != 2 {
                // 返回稳定参数错误。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 提供唯一公开用法。
                    "inspect-tree 只接受 app、uia 或 accessibility surface。",
                ));
            }
            // 读取 canonical s2:w 目标。
            let session_id = options
                // 访问目标对象。
                .target
                // 读取 sessionId。
                .get("sessionId")
                // 要求字符串。
                .and_then(Value::as_str)
                // 拒绝空值。
                .filter(|value| !value.is_empty())
                // 映射参数错误。
                .ok_or_else(|| {
                    // 指引调用方提供 opaque 目标。
                    CliErrorCode::InvalidArgument
                        .error("--target sessionId=<s2:w:opaque> is required.")
                })?;
            // 委托 service 协调隔离 worker。
            service.inspect_accessibility_tree(
                // 转发 opaque 目标。
                session_id,
                // 转发深度边界。
                options.max_depth,
                // 转发节点数量边界。
                options.max_items,
                // 转发 view。
                &options.view,
                // 转发 deadline。
                options.timeout_ms,
                // 转发 CLI 不可降级的隔离要求。
                options.isolation_requirement,
            )?
        }
        "run" => {
            let app = positional(positionals, 1, "run 需要 <app> <operation>。")?;
            let operation = positional(positionals, 2, "run 需要 <app> <operation>。")?;
            service.execute(build_request(
                Verb::Run,
                app,
                Some(operation),
                options.input.as_ref(),
                &options,
            )?)?
        }
        "sequence" => {
            // sequence 命令必须显式提供 JSON 输入。
            let input = options.input.as_ref().ok_or_else(|| {
                // 缺失输入时返回稳定参数错误。
                CliErrorCode::InvalidArgument.error("sequence 需要 --input <file|->。")
            })?;
            // 把 JSON 外壳反序列化失败统一映射为参数错误。
            let sequence =
                serde_json::from_value::<SequenceInput>(input.clone()).map_err(|error| {
                    // 保持反序列化诊断并隐藏私有错误类型。
                    CliErrorCode::InvalidArgument.error(error.to_string())
                })?;
            service.sequence(sequence)?
        }
        other => {
            // 未知命令必须失败闭合并返回稳定参数错误。
            return Err(CliErrorCode::InvalidArgument.error(format!(
                "未知命令 '{}'; 使用 {} help。",
                other,
                env!("CARGO_PKG_NAME")
            )));
        }
    };
    let exit_code = if result.get("ok") == Some(&Value::Bool(false)) {
        2
    } else {
        0
    };
    Ok(CliOutput {
        json: result,
        pretty: options.pretty,
        exit_code,
    })
}

struct Options {
    target: JsonMap,
    args: JsonMap,
    input: Option<Value>,
    // 保存 assess 使用的版本化 capability ID。
    capability: Option<String>,
    max_items: usize,
    max_depth: usize,
    // 保存隔离 worker deadline。
    timeout_ms: u32,
    // 保存可访问性树 view。
    view: String,
    // 保存 application.discover 应用边界。
    max_applications: usize,
    // 保存 application.discover 进程边界。
    max_processes: usize,
    // 保存 application.discover 窗口边界。
    max_windows: usize,
    confirmed: bool,
    foreground_consent: bool,
    // 保存 CLI 不可降级的隔离要求。
    isolation_requirement: IsolationRequirement,
    pretty: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            target: Map::new(),
            args: Map::new(),
            input: None,
            // 默认不提供 assessment capability。
            capability: None,
            max_items: 50,
            max_depth: 4,
            // 与 C++ 兼容入口使用相同默认 deadline。
            timeout_ms: 5_000,
            // 默认使用 ControlView。
            view: "control".to_owned(),
            // 与对照实现使用相同应用硬边界。
            max_applications: 4096,
            // 与对照实现使用相同进程硬边界。
            max_processes: 4096,
            // 与对照实现使用相同窗口硬边界。
            max_windows: 4096,
            confirmed: false,
            foreground_consent: false,
            // 默认保留标准后台优先行为。
            isolation_requirement: IsolationRequirement::Standard,
            pretty: false,
        }
    }
}

fn split_arguments(argv: &[String]) -> (&[String], &[String]) {
    let option_index = argv
        .iter()
        .position(|token| token.starts_with('-'))
        .unwrap_or(argv.len());
    (&argv[..option_index], &argv[option_index..])
}

fn parse_options(tokens: &[String]) -> AppResult<Options> {
    let mut options = Options::default();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--pretty" => options.pretty = true,
            "--confirm" => options.confirmed = true,
            "--allow-foreground" => options.foreground_consent = true,
            // 严格模式不得被 JSON 输入或前台同意放宽。
            "--strict-isolation" => {
                // 设置严格零打扰要求。
                options.isolation_requirement = IsolationRequirement::Strict;
            }
            "--target" => {
                index += 1;
                let expression = required_option_value(tokens, index, "--target")?;
                insert_assignment(&mut options.target, expression, &[])?;
            }
            "--arg" => {
                index += 1;
                let expression = required_option_value(tokens, index, "--arg")?;
                insert_assignment(&mut options.args, expression, &["text", "key"])?;
            }
            "--input" => {
                index += 1;
                let source = required_option_value(tokens, index, "--input")?;
                options.input = Some(read_json(source)?);
            }
            // 解析 capability assessment 的版本化 ID。
            "--capability" => {
                // 移到选项值。
                index += 1;
                // 保存原始文本，后续执行严格契约验证。
                options.capability = Some(
                    // 读取必需选项值。
                    required_option_value(tokens, index, "--capability")?
                        // 建立独立所有权。
                        .to_owned(),
                );
            }
            "--max-items" => {
                index += 1;
                options.max_items = parse_limit(
                    required_option_value(tokens, index, "--max-items")?,
                    "max-items",
                )?;
            }
            "--max-depth" => {
                index += 1;
                options.max_depth = parse_nonnegative_limit(
                    required_option_value(tokens, index, "--max-depth")?,
                    "max-depth",
                )?;
            }
            // 解析隔离 worker deadline。
            "--timeout-ms" => {
                // 移到选项值。
                index += 1;
                // 解析 1..30000ms。
                options.timeout_ms = parse_timeout_ms(required_option_value(
                    // 传入 token 切片。
                    tokens,
                    // 传入当前值索引。
                    index,
                    // 传入选项名。
                    "--timeout-ms",
                )?)?;
            }
            // 解析可访问性树 view。
            "--view" => {
                // 移到选项值。
                index += 1;
                // 读取必需 view。
                let view = required_option_value(tokens, index, "--view")?;
                // 只允许 control 或 raw。
                if !matches!(view, "control" | "raw") {
                    // 返回稳定参数错误。
                    return Err(CliErrorCode::InvalidArgument.error(
                        // 说明封闭枚举。
                        "--view 只能是 control 或 raw。",
                    ));
                }
                // 保存已验证 view。
                options.view = view.to_owned();
            }
            // 解析应用发现边界。
            "--max-applications" => {
                // 移到选项值。
                index += 1;
                // 解析正整数，4096 上限由 Module 统一验证。
                options.max_applications = parse_limit(
                    // 读取必需值。
                    required_option_value(tokens, index, "--max-applications")?,
                    // 提供稳定选项名。
                    "max-applications",
                )?;
            }
            // 解析进程发现边界。
            "--max-processes" => {
                // 移到选项值。
                index += 1;
                // 解析正整数，4096 上限由 Module 统一验证。
                options.max_processes = parse_limit(
                    // 读取必需值。
                    required_option_value(tokens, index, "--max-processes")?,
                    // 提供稳定选项名。
                    "max-processes",
                )?;
            }
            // 解析窗口发现边界。
            "--max-windows" => {
                // 移到选项值。
                index += 1;
                // 解析正整数，4096 上限由 Module 统一验证。
                options.max_windows = parse_limit(
                    // 读取必需值。
                    required_option_value(tokens, index, "--max-windows")?,
                    // 提供稳定选项名。
                    "max-windows",
                )?;
            }
            "--help" | "-h" => {
                // 参数解析阶段的帮助选项沿用稳定参数错误 envelope。
                return Err(CliErrorCode::InvalidArgument
                    .error(format!("请使用 {} help。", env!("CARGO_PKG_NAME"))));
            }
            option => {
                // 未知选项必须失败闭合并回报具体选项名。
                return Err(CliErrorCode::InvalidArgument.error(format!("未知选项 '{}'.", option)));
            }
        }
        index += 1;
    }
    // 让 inspect-tree 等专用入口也读取 structured JSON 的隔离要求。
    if let Some(requirement) = input_isolation_requirement(options.input.as_ref())? {
        // CLI 与 JSON 任一严格都保持严格。
        options.isolation_requirement = options.isolation_requirement.combine(requirement);
    }
    // 返回已完成不可降级合并的选项。
    Ok(options)
}

fn build_request(
    verb: Verb,
    app: &str,
    operation: Option<&str>,
    input: Option<&Value>,
    options: &Options,
) -> AppResult<CommandRequest> {
    let mut request = CommandRequest {
        verb,
        app: app.to_owned(),
        operation: operation.map(ToOwned::to_owned),
        target: Map::new(),
        args: Map::new(),
        max_items: options.max_items,
        max_depth: options.max_depth,
        confirmed: options.confirmed,
        foreground_consent: options.foreground_consent,
        // 从 CLI 冻结初始隔离要求。
        isolation_requirement: options.isolation_requirement,
    };
    if let Some(Value::Object(object)) = input {
        if object.contains_key("target")
            // structured request 也可仅携带确认状态。
            || object.contains_key("args")
            // structured request 可携带逐操作确认。
            || object.contains_key("confirmed")
            // structured request 可携带前台同意。
            || object.contains_key("foregroundConsent")
            // structured request 可携带强类型隔离要求。
            || object.contains_key("isolationRequirement")
        {
            request.target = object_map(object.get("target"), "input.target")?;
            request.args = object_map(object.get("args"), "input.args")?;
            if let Some(value) = object.get("maxItems").and_then(Value::as_u64) {
                request.max_items = cast_usize(value)?;
            }
            if let Some(value) = object.get("maxDepth").and_then(Value::as_u64) {
                request.max_depth = cast_usize(value)?;
            }
            request.confirmed =
                request.confirmed || object.get("confirmed") == Some(&Value::Bool(true));
            request.foreground_consent = request.foreground_consent
                || object.get("foregroundConsent") == Some(&Value::Bool(true));
            // 直接构造请求的内部调用也必须解析可选隔离要求。
            if let Some(requirement) = input_isolation_requirement(input)? {
                // CLI 与 JSON 任一严格都保持严格。
                request.isolation_requirement = request.isolation_requirement.combine(requirement);
            }
        } else {
            request.args = object.clone();
        }
    } else if input.is_some() {
        // 非 object 输入不能进入统一请求参数。
        return Err(CliErrorCode::InvalidArgument.error("--input 必须是 JSON object。"));
    }
    request.target.extend(options.target.clone());
    request.args.extend(options.args.clone());
    // legacy 可访问性 inspect 需要显式 worker deadline。
    if matches!(app, "uia" | "accessibility") {
        // 插入 camelCase 协议字段。
        request
            // 访问 args 对象。
            .args
            // 以 canonical CLI 选项覆盖宽松 args 别名。
            .insert("timeoutMs".to_owned(), Value::from(options.timeout_ms));
    }
    Ok(request)
}

// 从 structured JSON 外壳读取封闭隔离要求。
fn input_isolation_requirement(input: Option<&Value>) -> AppResult<Option<IsolationRequirement>> {
    // 只在 JSON object 中读取固定字段。
    let Some(value) = input
        // 展开可选输入。
        .and_then(Value::as_object)
        // 读取固定 camelCase 字段。
        .and_then(|object| object.get("isolationRequirement"))
    else {
        // 缺失字段保持调用方已有要求。
        return Ok(None);
    };
    // 解析封闭枚举并拒绝未知值。
    serde_json::from_value::<IsolationRequirement>(value.clone())
        // 包装为可选增强要求。
        .map(Some)
        // 映射为稳定参数错误。
        .map_err(|_| {
            // 不回显任意输入内容。
            CliErrorCode::InvalidArgument.error(
                // 说明允许的封闭值。
                "input.isolationRequirement must be 'standard' or 'strict'.",
            )
        })
}

fn object_map(value: Option<&Value>, field: &str) -> AppResult<JsonMap> {
    match value {
        None => Ok(Map::new()),
        Some(Value::Object(object)) => Ok(object.clone()),
        // 拒绝非 object 字段并返回稳定参数错误。
        _ => Err(CliErrorCode::InvalidArgument.error(format!("{} 必须是 object。", field))),
    }
}

fn insert_assignment(map: &mut JsonMap, expression: &str, string_keys: &[&str]) -> AppResult<()> {
    // 拒绝缺少等号的赋值表达式。
    let (key, raw_value) = expression.split_once('=').ok_or_else(|| {
        // 返回包含原始表达式的稳定参数错误。
        CliErrorCode::InvalidArgument.error(format!("参数必须是 key=value：'{}'.", expression))
    })?;
    if key.is_empty() || key.contains('.') || key.starts_with('-') {
        // 拒绝越过当前 CLI 字段边界的参数名。
        return Err(CliErrorCode::InvalidArgument.error(format!("不支持的参数名 '{}'.", key)));
    }
    let value = if string_keys.contains(&key) {
        Value::String(raw_value.to_owned())
    } else {
        parse_scalar(raw_value)
    };
    map.insert(key.to_owned(), value);
    Ok(())
}

fn parse_scalar(value: &str) -> Value {
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => match value.parse::<i64>() {
            Ok(number) => Value::from(number),
            Err(_) => match value.parse::<f64>() {
                Ok(number) if number.is_finite() => Value::from(number),
                _ => Value::String(value.to_owned()),
            },
        },
    }
}

fn parse_limit(value: &str, option: &str) -> AppResult<usize> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            // 非正整数保持稳定参数错误语义。
            CliErrorCode::InvalidArgument.error(format!("--{} 必须是正整数。", option))
        })
}

// 解析允许零的非负 usize 选项。
fn parse_nonnegative_limit(value: &str, option: &str) -> AppResult<usize> {
    // 解析十进制无符号整数。
    value.parse::<usize>().map_err(|_| {
        // 返回稳定参数错误。
        CliErrorCode::InvalidArgument.error(
            // 指明选项要求。
            format!("--{} 必须是非负整数。", option),
        )
    })
}

// 解析 1..30000ms worker deadline。
fn parse_timeout_ms(value: &str) -> AppResult<u32> {
    // 解析并验证封闭范围。
    value
        // 解析 u32。
        .parse::<u32>()
        // 转换为 Option 以组合范围检查。
        .ok()
        // 只保留允许范围。
        .filter(|value| (1..=30_000).contains(value))
        // 映射参数错误。
        .ok_or_else(|| {
            // 返回稳定错误。
            CliErrorCode::InvalidArgument.error(
                // 说明允许范围。
                "--timeout-ms 必须是 1..30000 的整数。",
            )
        })
}

// 把协议无符号整数安全收窄为平台 usize。
fn cast_usize(value: u64) -> AppResult<usize> {
    // 平台范围溢出必须返回稳定参数错误。
    usize::try_from(value).map_err(|_| CliErrorCode::InvalidArgument.error("数值超出 usize 范围。"))
}

// 读取指定索引的必需位置参数。
fn positional<'a>(positionals: &'a [String], index: usize, message: &str) -> AppResult<&'a str> {
    positionals.get(index).map(String::as_str).ok_or_else(|| {
        // 缺失位置参数时使用调用点提供的稳定消息。
        CliErrorCode::InvalidArgument.error(message)
    })
}

// 读取指定选项之后的必需值。
fn required_option_value<'a>(
    tokens: &'a [String],
    index: usize,
    option: &str,
) -> AppResult<&'a str> {
    tokens
        .get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-') || (option == "--input" && *value == "-"))
        .ok_or_else(|| {
            // 缺失选项值时返回稳定参数错误。
            CliErrorCode::InvalidArgument.error(format!("{} 需要一个值。", option))
        })
}

fn help_json() -> Value {
    json!({
        "ok": true,
        "name": env!("CARGO_PKG_NAME"),
        "policy": "background preferred; foreground input requires explicit --allow-foreground consent",
        "usage": [
            "ai-computer-toolkit catalog [app] [--pretty]",
            "ai-computer-toolkit build-info [--pretty]",
            "ai-computer-toolkit capabilities [surface|method|descriptor] [args] [--pretty]",
            "ai-computer-toolkit methods [method] [--pretty]",
            "ai-computer-toolkit describe <app> [operation] [--pretty]",
            "ai-computer-toolkit doctor [app] [--pretty]",
            "ai-computer-toolkit status [app] [--pretty]",
            "ai-computer-toolkit operation status <s2:o:opaque> [--timeout-ms 1..30000] [--pretty]",
            "ai-computer-toolkit operation await <s2:o:opaque> [--timeout-ms 1..30000] [--pretty]",
            "ai-computer-toolkit operation cancel <s2:o:opaque> [--timeout-ms 1..30000] [--pretty]",
            "ai-computer-toolkit operation start window.record@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--timeout-ms 1..30000] [--pretty]",
            "ai-computer-toolkit discover app [--max-applications n] [--max-processes n] [--max-windows n] [--pretty]",
            "ai-computer-toolkit discover isolation [--pretty]",
            "ai-computer-toolkit preflight-capture app --target sessionId=<s2:w:opaque> [--pretty]",
            "ai-computer-toolkit assess app --capability <capability@version> --target sessionId=<opaque> [--pretty]",
            "ai-computer-toolkit sessions <app> [--target key=value] [--max-items n] [--pretty]",
            "ai-computer-toolkit inspect <app> --target sessionId=<id> [--max-depth n] [--max-items n] [--pretty]",
            "ai-computer-toolkit inspect-tree <app|uia|accessibility> --target sessionId=<s2:w:opaque> [--max-depth 0..20] [--max-items 1..4096] [--view control|raw] [--timeout-ms 1..30000] [--strict-isolation] [--pretty]",
            "ai-computer-toolkit run <app> <operation> --target key=value --arg key=value --confirm [--strict-isolation] [--allow-foreground] [--pretty]",
            "ai-computer-toolkit sequence --input <file|-> [--pretty]"
        ],
        "stdout": "JSON only; failures are structured JSON with a non-zero exit code."
    })
}

#[cfg(test)]
mod tests;
