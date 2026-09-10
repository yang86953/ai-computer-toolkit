//! Linux JSON CLI Adapter。

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::{
    capabilities,
    components::bounded_json_input,
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement, Verb},
    service::{AppControlService, SequenceInput},
};

pub struct CliOutput {
    pub json: Value,
    pub pretty: bool,
    pub exit_code: i32,
}

#[derive(Default)]
struct Options {
    pretty: bool,
    target: Map<String, Value>,
    args: Map<String, Value>,
    capability: Option<String>,
    input_source: Option<String>,
    confirmed: bool,
    foreground_consent: bool,
    max_items: usize,
    max_items_explicit: bool,
    max_depth: usize,
    max_depth_explicit: bool,
    max_applications: usize,
    max_applications_explicit: bool,
    max_processes: usize,
    max_processes_explicit: bool,
    max_windows: usize,
    max_windows_explicit: bool,
    unused_read_option: bool,
    strict_isolation: bool,
}

impl Options {
    fn with_defaults() -> Self {
        Self {
            max_items: 50,
            max_depth: 4,
            max_applications: 512,
            max_processes: 512,
            max_windows: 512,
            ..Self::default()
        }
    }
}

pub fn run(argv: Vec<String>) -> AppResult<CliOutput> {
    let (positionals, options) = parse(argv)?;
    let service = AppControlService::new();
    let command = positionals.first().map(String::as_str).unwrap_or("help");
    // 新媒体 facade 使用独立 capability 组，不扩写旧 UIX CLI 参数白名单。
    if command == "run"
        && positionals.get(1).is_some_and(|app| app == "app")
        && options
            .capability
            .as_deref()
            .is_some_and(crate::components::media_playback_contract::is_capability)
    {
        require_positionals(
            &positionals,
            3,
            "Media App operations require exactly one generic verb.",
        )?;
        let json = run_media_app(&service, &options, &positionals[2])?;
        return Ok(CliOutput {
            json,
            pretty: options.pretty,
            exit_code: 0,
        });
    }
    if options.input_source.is_some()
        && positionals.as_slice() != ["sequence"]
        && positionals.as_slice() != ["run", "app", "create"]
        && positionals.as_slice() != ["run", "app", "apply"]
        && positionals.as_slice() != ["run", "app", "read"]
        && positionals.as_slice() != ["run", "app", "screenshot"]
        && positionals.as_slice() != ["run", "app", "close"]
        && positionals.as_slice() != ["run", "process", "terminate-graceful"]
        && positionals.as_slice() != ["run", "process", "terminate-force"]
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "--input is only accepted by sequence, Linux app routes and explicit process termination routes.",
        ));
    }
    let json = match command {
        "help" => help_json(),
        "version" | "--version" | "-v" => json!({
            "ok": true,
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
        }),
        "build-info" => {
            require_positionals(
                &positionals,
                1,
                "build-info does not accept positional arguments.",
            )?;
            service.build_info()
        }
        "capabilities" => {
            require_positionals(
                &positionals,
                1,
                "capabilities on Linux does not accept a surface alias.",
            )?;
            service.capability_surface()
        }
        "catalog" | "apps" => {
            require_max_positionals(&positionals, 2)?;
            service.catalog(positionals.get(1).map(String::as_str))?
        }
        "methods" => {
            require_max_positionals(&positionals, 2)?;
            service.methods(positionals.get(1).map(String::as_str))?
        }
        "describe" => {
            if !(2..=3).contains(&positionals.len()) {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    "describe requires <app> [operation].",
                ));
            }
            service.describe(&positionals[1], positionals.get(2).map(String::as_str))?
        }
        "doctor" => {
            require_max_positionals(&positionals, 2)?;
            service.doctor(positionals.get(1).map(String::as_str))
        }
        "status" => {
            require_max_positionals(&positionals, 2)?;
            match positionals.get(1) {
                Some(app) => service.execute(request(Verb::Status, app, &options))?,
                None => service.doctor(None),
            }
        }
        "sessions" => {
            require_positionals(&positionals, 2, "sessions requires <surface>.")?;
            if positionals[1] == "window" {
                ensure_uix_window_discovery_options(&options)?;
            } else if positionals[1] == "app" {
                ensure_uix_app_sessions_options(&options)?;
            }
            service.execute(request(Verb::Sessions, &positionals[1], &options))?
        }
        "inspect" => {
            require_positionals(&positionals, 2, "inspect requires <surface>.")?;
            if positionals[1] == "window" {
                ensure_uix_window_metadata_options(&options)?;
            } else if positionals[1] == "app" {
                ensure_uix_app_metadata_options(&options)?;
            }
            service.execute(request(Verb::Inspect, &positionals[1], &options))?
        }
        "inspect-tree" => {
            if positionals.as_slice() != ["inspect-tree", "accessibility"] {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    "Linux UIX inspect-tree requires the accessibility surface.",
                ));
            }
            ensure_uix_tree_options(&options)?;
            let mut request = request(Verb::Run, "accessibility", &options);
            request.operation = Some("inspect-tree".to_owned());
            service.execute(request)?
        }
        "run" => {
            if positionals.as_slice() == ["run", "app", "create"] {
                // 应用启动确认和前景同意都必须先于 input 文件与目标解析。
                if !options.confirmed {
                    return Err(AppControlError::new(
                        "CONFIRMATION_REQUIRED",
                        "Linux application launch requires explicit confirmation.",
                    ));
                }
                if options.capability.as_deref() == Some(capabilities::APPLICATION_OPEN_V2)
                    && !options.foreground_consent
                {
                    return Err(AppControlError::new(
                        "FOREGROUND_CONSENT_REQUIRED",
                        "Linux application launch requires upfront foreground-impact consent.",
                    ));
                }
                ensure_application_open_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("application open requires input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux application open --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "app", &options);
                request.operation = Some("create".to_owned());
                request.args.insert(
                    "capability".to_owned(),
                    Value::String(capabilities::APPLICATION_OPEN_V2.to_owned()),
                );
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "app", "apply"] {
                // mutation 确认必须先于 input 文件、target、capability 和 provider。
                if !options.confirmed {
                    return Err(AppControlError::new(
                        "CONFIRMATION_REQUIRED",
                        "UIX mutation requires explicit confirmation.",
                    ));
                }
                if matches!(
                    options.capability.as_deref(),
                    Some(capabilities::WINDOW_LIFECYCLE_V2)
                        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE)
                        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION)
                        | Some(capabilities::WINDOW_LIFECYCLE_TRANSITION)
                        | Some(capabilities::WINDOW_ACTIVATE)
                        | Some(capabilities::WINDOW_ACTIVATE_TRANSITION)
                ) && !options.foreground_consent
                {
                    return Err(AppControlError::new(
                        "FOREGROUND_CONSENT_REQUIRED",
                        "The selected UIX window operation requires upfront foreground-impact consent.",
                    ));
                }
                ensure_uix_apply_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("apply options require input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "UIX app apply --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "app", &options);
                request.operation = Some("apply".to_owned());
                request.args.insert(
                    "capability".to_owned(),
                    Value::String(
                        options
                            .capability
                            .clone()
                            .unwrap_or_else(|| unreachable!("apply capability is required")),
                    ),
                );
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "app", "read"] {
                ensure_uix_read_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("wait options require input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "UIX window wait --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "app", &options);
                request.operation = Some("read".to_owned());
                request.args.insert(
                    "capability".to_owned(),
                    Value::String(
                        options
                            .capability
                            .clone()
                            .unwrap_or_else(|| unreachable!("read capability is required")),
                    ),
                );
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "app", "close"] {
                // 关闭确认必须先于 input 文件、target、capability 和 provider。
                if !options.confirmed {
                    return Err(AppControlError::new(
                        "CONFIRMATION_REQUIRED",
                        "UIX window close requires explicit confirmation.",
                    ));
                }
                ensure_uix_close_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("close options require input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "UIX window close --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "app", &options);
                request.operation = Some("close".to_owned());
                request.args.insert(
                    "capability".to_owned(),
                    Value::String(
                        options
                            .capability
                            .clone()
                            .unwrap_or_else(|| unreachable!("close capability is required")),
                    ),
                );
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "app", "screenshot"] {
                // 敏感像素读取确认必须先于 input 文件、target 和 Agent 发现。
                if !options.confirmed {
                    return Err(AppControlError::new(
                        "CONFIRMATION_REQUIRED",
                        "UIX window screenshot requires explicit confirmation.",
                    ));
                }
                ensure_uix_screenshot_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("screenshot options require input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "UIX window screenshot --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "app", &options);
                request.operation = Some("screenshot".to_owned());
                request.args.insert(
                    "capability".to_owned(),
                    Value::String(capabilities::WINDOW_SCREENSHOT_V2.to_owned()),
                );
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "desktop", "screenshot-interactive"] {
                let mut request = request(Verb::Run, "desktop", &options);
                request.operation = Some("screenshot-interactive".to_owned());
                service.execute(request)?
            } else if positionals.as_slice() == ["run", "process", "terminate-graceful"]
                || positionals.as_slice() == ["run", "process", "terminate-force"]
            {
                let force = positionals.as_slice() == ["run", "process", "terminate-force"];
                // 高风险或 critical 进程 mutation 必须先确认，之后才读取输入文件或接触目标。
                if !options.confirmed {
                    return Err(AppControlError::new(
                        "CONFIRMATION_REQUIRED",
                        if force {
                            "Linux forced process termination requires explicit confirmation."
                        } else {
                            "Linux graceful process termination requires explicit confirmation."
                        },
                    ));
                }
                ensure_process_termination_options(&options)?;
                let input = read_json_input(
                    options
                        .input_source
                        .as_deref()
                        .unwrap_or_else(|| unreachable!("termination options require input")),
                )?;
                if !input.is_object() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux process termination --input must contain a JSON object.",
                    ));
                }
                let mut request = request(Verb::Run, "process", &options);
                request.operation = Some(if force {
                    "terminate-force".to_owned()
                } else {
                    "terminate-graceful".to_owned()
                });
                request.args.insert("input".to_owned(), input);
                service.execute(request)?
            } else {
                return Err(platform_gap(
                    command,
                    positionals.get(1).map(String::as_str),
                ));
            }
        }
        "discover" => {
            if positionals.as_slice() != ["discover", "app"] {
                return Err(platform_gap(
                    command,
                    positionals.get(1).map(String::as_str),
                ));
            }
            match options.capability.as_deref() {
                None | Some(capabilities::APPLICATION_DISCOVER_V2) => service.discover_app(
                    options.max_applications,
                    options.max_processes,
                    options.max_windows,
                )?,
                Some(capabilities::APPLICATION_SESSION_DISCOVER_V2) => {
                    ensure_application_session_discovery_options(&options)?;
                    service.discover_application_sessions(
                        options.max_applications,
                        options.max_processes,
                    )?
                }
                Some(capabilities::APPLICATION_SESSION_DISCOVER_V3) => {
                    ensure_uix_application_session_discovery_options(&options)?;
                    service.discover_uix_application_sessions(
                        options.max_applications,
                        options.max_processes,
                        options.max_windows,
                    )?
                }
                Some(capabilities::APPLICATION_SESSION_DISCOVER_V4) => {
                    ensure_uix_application_session_discovery_options(&options)?;
                    service.discover_launch_aware_application_sessions(
                        options.max_applications,
                        options.max_processes,
                        options.max_windows,
                    )?
                }
                Some(capabilities::APPLICATION_DISCOVER_V3) => {
                    ensure_application_discovery_v3_options(&options)?;
                    service.discover_app_v3(
                        options.max_applications,
                        options.max_processes,
                        options.max_windows,
                    )?
                }
                Some(_) => {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "discover app only accepts application.discover@2, application.discover@3, or application.session.discover@2 through @4 on Linux.",
                    ));
                }
            }
        }
        "assess" => {
            if positionals.as_slice() != ["assess", "app"] {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    "assess requires the app surface.",
                ));
            }
            let capability = options.capability.as_deref().ok_or_else(|| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "assess requires --capability <id@version>.",
                )
            })?;
            if !capabilities::is_versioned_capability_id(capability) {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    "--capability must be a canonical versioned capability ID.",
                ));
            }
            let session_id = options
                .target
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    AppControlError::new(
                        "INVALID_ARGUMENT",
                        "assess requires --target sessionId=<opaque>.",
                    )
                })?;
            service.assess_capability(capability, session_id)?
        }
        "sequence" => {
            require_positionals(
                &positionals,
                1,
                "sequence does not accept positional arguments.",
            )?;
            ensure_sequence_options(&options)?;
            let input = read_json_input(
                options
                    .input_source
                    .as_deref()
                    .unwrap_or_else(|| unreachable!("sequence options require input")),
            )?;
            let sequence = serde_json::from_value::<SequenceInput>(input).map_err(|_| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "sequence --input does not match the closed Workflow schema.",
                )
            })?;
            service.sequence(sequence)?
        }
        _ => {
            return Err(platform_gap(
                command,
                positionals.get(1).map(String::as_str),
            ));
        }
    };

    let exit_code = if command == "sequence" && json.get("ok") == Some(&Value::Bool(false)) {
        2
    } else {
        0
    };
    Ok(CliOutput {
        json,
        pretty: options.pretty,
        exit_code,
    })
}

fn read_json_input(source: &str) -> AppResult<Value> {
    let result = if source == "-" {
        bounded_json_input::read_stdin()
    } else {
        bounded_json_input::read_file(Path::new(source))
    };
    result.map_err(|error| match error {
        bounded_json_input::BoundedJsonInputError::TooLarge {
            observed_bytes,
            maximum_bytes,
        } => {
            // 消费 Component 证据但不把输入实际长度带到公共错误。
            let _ = observed_bytes;
            AppControlError::new(
                "INVALID_ARGUMENT",
                format!("--input JSON must not exceed {maximum_bytes} bytes."),
            )
        }
        bounded_json_input::BoundedJsonInputError::Read(error) => {
            // 只读取封闭错误类别，禁止把路径相关 I/O 文本公开。
            let _ = error.kind();
            AppControlError::new(
                "INVALID_ARGUMENT",
                "--input must name a readable JSON source.",
            )
        }
        bounded_json_input::BoundedJsonInputError::Invalid(message) => {
            // 消费内部诊断但不回显可能变化的 parser 文本。
            let _ = message.len();
            AppControlError::new(
                "INVALID_ARGUMENT",
                "--input must contain one valid UTF-8 JSON value.",
            )
        }
    })
}

fn request(verb: Verb, app: &str, options: &Options) -> CommandRequest {
    let mut request = CommandRequest::read(verb, app);
    request.target = options.target.clone();
    request.args = options.args.clone();
    request.max_items = options.max_items;
    request.max_depth = options.max_depth;
    request.isolation_requirement = if options.strict_isolation {
        IsolationRequirement::Strict
    } else {
        IsolationRequirement::Standard
    };
    request.confirmed = options.confirmed;
    request.foreground_consent = options.foreground_consent;
    request
}

/// 媒体 CLI 只物化现有公开 facade 请求；控制确认先于输入文件读取。
fn run_media_app(
    service: &AppControlService,
    options: &Options,
    operation: &str,
) -> AppResult<Value> {
    let control = options.capability.as_deref() == Some(capabilities::MEDIA_PLAYBACK_CONTROL_V3);
    if control && !options.confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "Media playback control requires explicit confirmation.",
        ));
    }
    if !options.args.is_empty()
        || options.foreground_consent
        || (!control && options.confirmed)
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
        || options.unused_read_option
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Media App accepts only capability, exact target when required, input, confirmation for control, strict isolation and pretty output.",
        ));
    }
    let input = match options.input_source.as_deref() {
        Some(source) => read_json_input(source)?,
        None => json!({}),
    };
    let mut request = request(Verb::Run, "app", options);
    request.operation = Some(operation.into());
    request
        .args
        .insert("capability".into(), json!(options.capability));
    request.args.insert("input".into(), input);
    service.execute(request)
}

fn ensure_sequence_options(options: &Options) -> AppResult<()> {
    if options.input_source.is_none()
        || options.capability.is_some()
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
        || options.unused_read_option
        || options.strict_isolation
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "sequence accepts only --input <file|-> and --pretty; every step owns its confirmation, consent and isolation fields.",
        ));
    }
    Ok(())
}

fn parse(argv: Vec<String>) -> AppResult<(Vec<String>, Options)> {
    let mut positionals = Vec::new();
    let mut options = Options::with_defaults();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--pretty" => options.pretty = true,
            "--target" => {
                index += 1;
                let assignment = argv.get(index).ok_or_else(|| {
                    AppControlError::new("INVALID_ARGUMENT", "--target requires key=value.")
                })?;
                let (key, value) = assignment.split_once('=').ok_or_else(|| {
                    AppControlError::new("INVALID_ARGUMENT", "--target requires key=value.")
                })?;
                if !matches!(key, "sessionId" | "name" | "processId") || value.is_empty() {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Unsupported or empty Linux target field.",
                    ));
                }
                options
                    .target
                    .insert(key.to_owned(), Value::String(value.to_owned()));
            }
            "--capability" => {
                index += 1;
                options.capability = Some(required_value(&argv, index, "--capability")?.to_owned());
            }
            "--max-items" => {
                index += 1;
                options.max_items = parse_limit(required_value(&argv, index, "--max-items")?)?;
                options.max_items_explicit = true;
            }
            "--max-depth" => {
                index += 1;
                options.max_depth = required_value(&argv, index, "--max-depth")?
                    .parse::<usize>()
                    .ok()
                    .filter(|value| *value <= 20)
                    .ok_or_else(|| {
                        AppControlError::new(
                            "INVALID_ARGUMENT",
                            "--max-depth must be an integer from 0 through 20.",
                        )
                    })?;
                options.max_depth_explicit = true;
            }
            "--max-applications" => {
                index += 1;
                options.max_applications =
                    parse_limit(required_value(&argv, index, "--max-applications")?)?;
                options.max_applications_explicit = true;
            }
            "--max-processes" => {
                index += 1;
                options.max_processes =
                    parse_limit(required_value(&argv, index, "--max-processes")?)?;
                options.max_processes_explicit = true;
            }
            "--max-windows" => {
                index += 1;
                options.max_windows = parse_limit(required_value(&argv, index, "--max-windows")?)?;
                options.max_windows_explicit = true;
            }
            "--confirm" => options.confirmed = true,
            "--allow-foreground" => options.foreground_consent = true,
            // Linux Portal 路线不接受隔离降级，但仍兼容解析公共严格隔离开关。
            "--strict-isolation" => options.strict_isolation = true,
            "--arg" => {
                index += 1;
                insert_argument(&mut options.args, required_value(&argv, index, "--arg")?)?;
            }
            "--timeout-ms" => {
                index += 1;
                let value = required_value(&argv, index, "--timeout-ms")?
                    .parse::<u64>()
                    .map_err(|_| {
                        AppControlError::new("INVALID_ARGUMENT", "--timeout-ms must be an integer.")
                    })?;
                options.args.insert("timeoutMs".to_owned(), json!(value));
            }
            // 输入源只保存路径；UIX mutation 通过确认后才会实际读取。
            "--input" => {
                index += 1;
                options.input_source = Some(required_value(&argv, index, "--input")?.to_owned());
            }
            // Linux 只读 UIX 路线不接受旧 UIA view 参数。
            "--view" => {
                let option = argv[index].clone();
                index += 1;
                let _ = required_value(&argv, index, &option)?;
                options.unused_read_option = true;
            }
            value if value.starts_with('-') && !matches!(value, "--version" | "-v") => {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    "The option is not supported by the Linux adapter.",
                ));
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }
    Ok((positionals, options))
}

/// UIX mutation 只接受冻结的版本二输入文件和精确窗口目标。
fn ensure_uix_apply_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    let capability_options_valid = match options.capability.as_deref() {
        Some(capabilities::UI_ELEMENT_ACTION_V2)
        | Some(capabilities::UI_ELEMENT_TRANSITION)
        | Some(capabilities::UI_INPUT_KEY_V2)
        | Some(capabilities::UI_INPUT_KEY_SEQUENCE)
        | Some(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION)
        | Some(capabilities::UI_INPUT_KEY_TRANSITION)
        | Some(capabilities::UI_INPUT_SEQUENCE)
        | Some(capabilities::UI_INPUT_SEQUENCE_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_V2)
        | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE)
        | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_CLICK_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE)
        | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_SEQUENCE)
        | Some(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION)
        | Some(capabilities::UI_INPUT_POINTER_DRAG)
        | Some(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION) => !options.foreground_consent,
        Some(capabilities::WINDOW_LIFECYCLE_V2)
        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE)
        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION)
        | Some(capabilities::WINDOW_LIFECYCLE_TRANSITION)
        | Some(capabilities::WINDOW_ACTIVATE)
        | Some(capabilities::WINDOW_ACTIVATE_TRANSITION) => options.foreground_consent,
        _ => false,
    };
    if !capability_options_valid
        || options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app apply requires ui.element.action@2, ui.element.transition@1, ui.input.key@2, ui.input.key.sequence@1, ui.input.key.transition@1, ui.input.sequence@1, ui.input.pointer@2, ui.input.pointer.click.sequence@1, ui.input.pointer.click.transition@1, ui.input.pointer.move.transition@1, ui.input.pointer.move.sequence@1, ui.input.pointer.sequence@1, ui.input.pointer.sequence.transition@1, ui.input.pointer.drag@1, or ui.input.pointer.drag.transition@1 without foreground consent, or window.lifecycle@2/window.lifecycle.sequence@1/window.lifecycle.sequence.transition@1/window.lifecycle.transition@1/window.activate@1/window.activate.transition@1 with --allow-foreground; all require one sessionId, --input and --confirm.",
        ));
    }
    Ok(())
}

/// Linux application.open@2 只接受精确目标、空输入、确认与前景同意。
fn ensure_application_open_options(options: &Options) -> AppResult<()> {
    if options.capability.as_deref() != Some(capabilities::APPLICATION_OPEN_V2)
        || options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || !options.confirmed
        || !options.foreground_consent
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Linux app create requires application.open@2, one sessionId, --input, --confirm and --allow-foreground.",
        ));
    }
    Ok(())
}

/// UIX 只读等待只接受冻结输入、精确窗口和无副作用选项。
fn ensure_uix_read_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if !matches!(
        options.capability.as_deref(),
        Some(capabilities::UI_ELEMENT_LOCATE_V2)
            | Some(capabilities::UI_ELEMENT_WAIT_V2)
            | Some(capabilities::WINDOW_REVISION_WAIT)
            | Some(capabilities::WINDOW_CLOSED_WAIT_V2)
            | Some(capabilities::WINDOW_STATE_READ)
            | Some(capabilities::WINDOW_STATE_WAIT)
    ) || options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app read requires --capability ui.element.locate@2, ui.element.wait@2, window.revision.wait@1, window.closed.wait@2, window.state.read@1, or window.state.wait@1, one sessionId and --input.",
        ));
    }
    Ok(())
}

/// UIX 截图只接受精确窗口、冻结输入、确认和同会话执行选项。
fn ensure_uix_screenshot_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.capability.as_deref() != Some(capabilities::WINDOW_SCREENSHOT_V2)
        || options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || !options.confirmed
        || options.foreground_consent
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app screenshot requires window.screenshot@2, one sessionId, --input and --confirm without foreground consent.",
        ));
    }
    Ok(())
}

/// UIX close 只接受精确窗口、冻结输入、确认和无前景激活选项。
fn ensure_uix_close_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if !matches!(
        options.capability.as_deref(),
        Some(capabilities::WINDOW_CLOSE_V2) | Some(capabilities::WINDOW_CLOSE_TRANSITION)
    ) || options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || !options.confirmed
        || options.foreground_consent
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app close requires --capability window.close@2 or window.close.transition@1, one sessionId, --input and --confirm without foreground consent.",
        ));
    }
    Ok(())
}

/// 两条显式 pidfd 终止路线都只接受一个 opaque 目标、严格输入与显式确认。
fn ensure_process_termination_options(options: &Options) -> AppResult<()> {
    if options.input_source.is_none()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || options.capability.is_some()
        || !options.confirmed
        || options.foreground_consent
        || options.unused_read_option
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Linux process termination requires one sessionId, --input and --confirm without foreground consent.",
        ));
    }
    Ok(())
}

/// 版本二聚合只接受两个真实生效的独立清单上限，拒绝静默参数。
fn ensure_application_session_discovery_options(options: &Options) -> AppResult<()> {
    if options.max_items_explicit
        || options.max_windows_explicit
        || options.unused_read_option
        || options.strict_isolation
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "application.session.discover@2 only accepts --max-applications, --max-processes, --capability and --pretty.",
        ));
    }
    Ok(())
}

/// 版本三应用清单只接受真实生效的清单上限与 capability 选择。
fn ensure_application_discovery_v3_options(options: &Options) -> AppResult<()> {
    if options.max_items_explicit
        || options.max_depth_explicit
        || options.unused_read_option
        || options.strict_isolation
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "application.discover@3 only accepts --max-applications, --max-processes, --max-windows, --capability and --pretty.",
        ));
    }
    Ok(())
}

/// 版本三聚合接受三个真实清单上限，但拒绝所有会改变执行域或被静默忽略的选项。
fn ensure_uix_application_session_discovery_options(options: &Options) -> AppResult<()> {
    if options.max_items_explicit
        || options.unused_read_option
        || options.strict_isolation
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "application.session.discover@3 or @4 only accepts --max-applications, --max-processes, --max-windows, --capability and --pretty.",
        ));
    }
    Ok(())
}

/// UIX 只读树入口拒绝所有无效或会改变执行域的兼容选项。
fn ensure_uix_tree_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.unused_read_option
        || options.capability.is_some()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX inspect-tree accepts only target.sessionId, --max-depth, --max-items and --pretty.",
        ));
    }
    Ok(())
}

fn ensure_uix_window_discovery_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.unused_read_option
        || options.capability.is_some()
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX window discovery accepts only --max-items and --pretty.",
        ));
    }
    Ok(())
}

fn ensure_uix_app_sessions_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.unused_read_option
        || options.capability.is_some()
        || !options.target.is_empty()
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app sessions accept only --max-items and --pretty.",
        ));
    }
    Ok(())
}

fn ensure_uix_window_metadata_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.unused_read_option
        || options.capability.is_some()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX window metadata accepts only target.sessionId and --pretty.",
        ));
    }
    Ok(())
}

fn ensure_uix_app_metadata_options(options: &Options) -> AppResult<()> {
    reject_uix_strict_isolation(options)?;
    if options.unused_read_option
        || options.capability.is_some()
        || options.target.len() != 1
        || options
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !options.args.is_empty()
        || options.confirmed
        || options.foreground_consent
        || options.max_items_explicit
        || options.max_depth_explicit
        || options.max_applications_explicit
        || options.max_processes_explicit
        || options.max_windows_explicit
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX app inspect accepts only target.sessionId and --pretty.",
        ));
    }
    Ok(())
}

fn reject_uix_strict_isolation(options: &Options) -> AppResult<()> {
    if options.strict_isolation {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "UIX Agent requests execute in the same session and cannot satisfy strict isolation.",
            json!({
                "platform": "linux",
                "provider": "uix-agent-v1",
                "executionRealm": "same-session-no-focus",
                "fallback": "none",
            }),
        ));
    }
    Ok(())
}

fn insert_argument(args: &mut Map<String, Value>, assignment: &str) -> AppResult<()> {
    let (key, raw) = assignment
        .split_once('=')
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "--arg requires key=value."))?;
    if raw.is_empty() || !matches!(key, "path" | "timeoutMs" | "overwrite") {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Unsupported or empty Linux operation argument.",
        ));
    }
    let value = match key {
        "timeoutMs" => json!(raw.parse::<u64>().map_err(|_| {
            AppControlError::new("INVALID_ARGUMENT", "timeoutMs must be an integer.")
        })?),
        "overwrite" => json!(raw.parse::<bool>().map_err(|_| {
            AppControlError::new("INVALID_ARGUMENT", "overwrite must be true or false.")
        })?),
        _ => Value::String(raw.to_owned()),
    };
    args.insert(key.to_owned(), value);
    Ok(())
}

fn required_value<'a>(argv: &'a [String], index: usize, option: &str) -> AppResult<&'a str> {
    argv.get(index).map(String::as_str).ok_or_else(|| {
        AppControlError::new("INVALID_ARGUMENT", format!("{option} requires a value."))
    })
}

fn parse_limit(value: &str) -> AppResult<usize> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            AppControlError::new("INVALID_ARGUMENT", "Limits must be positive integers.")
        })
}

fn require_positionals(positionals: &[String], expected: usize, message: &str) -> AppResult<()> {
    if positionals.len() == expected {
        Ok(())
    } else {
        Err(AppControlError::new("INVALID_ARGUMENT", message))
    }
}

fn require_max_positionals(positionals: &[String], maximum: usize) -> AppResult<()> {
    if positionals.len() <= maximum {
        Ok(())
    } else {
        Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Too many positional arguments.",
        ))
    }
}

fn platform_gap(command: &str, surface: Option<&str>) -> AppControlError {
    AppControlError::with_details(
        "CAPABILITY_UNAVAILABLE",
        "The requested command has no certified Linux provider.",
        json!({
            "platform": "linux",
            "command": command,
            "surface": surface,
            "executionRealm": "none",
            "fallback": "none",
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
        }),
    )
}

fn help_json() -> Value {
    json!({
        "ok": true,
        "name": env!("CARGO_PKG_NAME"),
        "platform": "linux",
        "policy": "capability-first; unavailable Linux providers fail closed with no fallback",
        "usage": [
            "ai-computer-toolkit version",
            "ai-computer-toolkit build-info [--pretty]",
            "ai-computer-toolkit capabilities [--pretty]",
            "ai-computer-toolkit status [app|process|desktop|window|accessibility] [--pretty]",
            "ai-computer-toolkit sessions app [--max-items n] [--pretty]",
            "ai-computer-toolkit sessions process [--target name=<name>] [--max-items n] [--pretty]",
            "ai-computer-toolkit inspect process --target sessionId=<s2:p:opaque> [--pretty]",
            "ai-computer-toolkit run process terminate-graceful --target sessionId=<s2:p:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run process terminate-force --target sessionId=<s2:p:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit sessions window [--max-items n] [--pretty]",
            "ai-computer-toolkit inspect window --target sessionId=<s2:w:opaque> [--pretty]",
            "ai-computer-toolkit inspect-tree accessibility --target sessionId=<s2:w:opaque> [--max-depth 0..20] [--max-items 1..4096] [--pretty]",
            "ai-computer-toolkit discover app [--max-processes n] [--pretty]",
            "ai-computer-toolkit discover app --capability application.discover@3 [--max-applications n] [--max-processes n] [--max-windows n] [--pretty]",
            "ai-computer-toolkit discover app --capability application.session.discover@2 [--max-applications n] [--max-processes n] [--pretty]",
            "ai-computer-toolkit discover app --capability application.session.discover@3 [--max-applications n] [--max-processes n] [--max-windows n] [--pretty]",
            "ai-computer-toolkit discover app --capability application.session.discover@4 [--max-applications n] [--max-processes n] [--max-windows n] [--pretty]",
            "ai-computer-toolkit assess app --capability <capability@version> --target sessionId=<opaque> [--pretty]",
            "ai-computer-toolkit run app discover --capability media.session.discover@3 [--input <file|->] [--strict-isolation] [--pretty]",
            "ai-computer-toolkit run app read --capability media.playback.state.read@3 --target sessionId=<s2:m:opaque> [--input <file|->] [--strict-isolation] [--pretty]",
            "ai-computer-toolkit run app apply --capability media.playback.control@3 --target sessionId=<s2:m:opaque> --input <file|-> --confirm [--strict-isolation] [--pretty]",
            "ai-computer-toolkit run app create --capability application.open@2 --target sessionId=<s2:a:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.element.action@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.element.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.key@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.key.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.key.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.click.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.click.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.move.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.move.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.sequence.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.drag@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability ui.input.pointer.drag.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app apply --capability window.lifecycle@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability window.lifecycle.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability window.lifecycle.sequence.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability window.lifecycle.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability window.activate@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app apply --capability window.activate.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit run app read --capability window.revision.wait@1 --target sessionId=<s2:w:opaque> --input <file|-> [--pretty]",
            "ai-computer-toolkit run app read --capability window.closed.wait@2 --target sessionId=<s2:w:opaque> --input <file|-> [--pretty]",
            "ai-computer-toolkit run app read --capability window.state.read@1 --target sessionId=<s2:w:opaque> --input <file|-> [--pretty]",
            "ai-computer-toolkit run app read --capability window.state.wait@1 --target sessionId=<s2:w:opaque> --input <file|-> [--pretty]",
            "ai-computer-toolkit run app read --capability ui.element.wait@2 --target sessionId=<s2:w:opaque> --input <file|-> [--pretty]",
            "ai-computer-toolkit run app screenshot --capability window.screenshot@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app close --capability window.close@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run app close --capability window.close.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm [--pretty]",
            "ai-computer-toolkit run desktop screenshot-interactive --target sessionId=<s2:h:opaque> --arg path=<output.png> --confirm --allow-foreground [--pretty]",
            "ai-computer-toolkit sequence --input <file|-> [--pretty]",
        ],
        "stdout": "JSON only; failures are structured JSON with a non-zero exit code.",
    })
}
