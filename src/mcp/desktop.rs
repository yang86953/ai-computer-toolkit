//! MCP 工具到桌面会话的映射层。
//!
//! 本层持有本客户端独占的会话状态（broker、sessionId、最新 frameId、按需建立的
//! 私有图像目录），并把工具调用翻译为 broker 操作。授权布尔值只表达用户已有授权：
//! 这里不创造授权，也不提供后台隔离路线。捕获目录不可用只降级依赖它的动作，
//! initialize、tools/list 与 computer_status 不依赖它。

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde_json::{Value, json};

use super::broker::{Broker, BrokerFailure};

/// PNG 文件头；只接受真实 PNG，拒绝被替换的路径内容。
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// 内联图像的大小上限，避免把超大截图塞进协议帧。
const MAXIMUM_PNG_BYTES: usize = 32 * 1024 * 1024;

/// 默认返回图最长边。
const DEFAULT_MAX_DIMENSION: u64 = 1280;

/// 单次长流程执行允许的批次数上限；每批自身仍受 128 步输入契约约束。
const MAXIMUM_RUN_BATCHES: usize = 64;

/// 长流程回读图像张数上限。
const MAXIMUM_RUN_FRAMES: usize = 8;

/// 长流程缺省总预算，留在 MCP 客户端缺省工具超时之内。
const DEFAULT_RUN_TOTAL_MS: u64 = 45_000;

/// 长流程最大总预算；实际能否用满取决于调用方的工具超时配置。
const MAXIMUM_RUN_TOTAL_MS: u64 = 600_000;

/// 工具执行结果：MCP `content` 数组与是否错误。
pub struct ToolOutcome {
    pub content: Vec<Value>,
    pub is_error: bool,
}

/// 一次工具调用的授权来源：显式确认字段或 session 会话继承。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthorizationBasis {
    /// 调用方显式给出全套确认字段并全部通过校验。
    Explicit,
    /// 字段被省略且当前会话以 authorizationMode=session 打开。
    Inherited,
}

impl AuthorizationBasis {
    /// 输入类请求要下发给 broker 的确认字段；继承时省略，由 broker 侧同一事实校验。
    fn input_fields(self) -> Value {
        match self {
            Self::Explicit => json!({
                "confirmed": true,
                "foregroundConsent": true,
                "strictIsolation": false,
            }),
            Self::Inherited => json!({}),
        }
    }

    /// 截图/观察类请求的确认字段。
    fn capture_fields(self) -> Value {
        match self {
            Self::Explicit => json!({
                "confirmed": true,
                "strictIsolation": false,
            }),
            Self::Inherited => json!({}),
        }
    }
}

/// 校验显式/继承的确认字段组合；不含会话状态，便于独立回归。
///
/// 规则与 broker 模块一致：显式拒绝优先闭合；字段省略只允许继承自
/// session 授权会话；部分显式 + 部分省略同样要求 session 会话。
fn authorization_basis(
    confirmed: Option<bool>,
    foreground: Option<bool>,
    strict: Option<bool>,
    write: bool,
    session_scoped: bool,
    connected: bool,
) -> Result<AuthorizationBasis, BrokerFailure> {
    if confirmed == Some(false) || strict == Some(true) || (write && foreground == Some(false)) {
        return Err(BrokerFailure::failed(
            "CONSENT_REQUIRED",
            "Desktop foreground requires existing user authorization with strictIsolation=false.",
        ));
    }
    let complete = confirmed.is_some() && strict.is_some() && (!write || foreground.is_some());
    if complete {
        return Ok(AuthorizationBasis::Explicit);
    }
    if session_scoped && connected {
        return Ok(AuthorizationBasis::Inherited);
    }
    Err(BrokerFailure::failed(
        "CONSENT_REQUIRED",
        "Connect with authorizationMode=session to inherit one confirmation per session, or pass confirmed/foregroundConsent=true with strictIsolation=false explicitly.",
    ))
}

impl ToolOutcome {
    /// 纯文本结果。
    fn text(value: &Value) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": value.to_string() })],
            is_error: false,
        }
    }

    /// 文本 + PNG 图像结果。
    fn image(text: &Value, png: &[u8]) -> Self {
        Self {
            content: vec![
                json!({ "type": "text", "text": text.to_string() }),
                json!({
                    "type": "image",
                    "mimeType": "image/png",
                    "data": BASE64_STANDARD.encode(png),
                }),
            ],
            is_error: false,
        }
    }

    /// 失败结果：仍是成功的 JSON-RPC 响应，错误在工具层表达。
    fn failed(failure: &BrokerFailure) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": failure.payload().to_string() })],
            is_error: true,
        }
    }
}

/// 测试探针：暴露授权基判定矩阵给清单回归，不触碰会话状态。
#[cfg(test)]
pub(crate) fn authorization_basis_probe(
    confirmed: Option<bool>,
    foreground: Option<bool>,
    strict: Option<bool>,
    write: bool,
    session_scoped: bool,
    connected: bool,
) -> Result<&'static str, &'static str> {
    match authorization_basis(
        confirmed,
        foreground,
        strict,
        write,
        session_scoped,
        connected,
    ) {
        Ok(AuthorizationBasis::Explicit) => Ok("explicit"),
        Ok(AuthorizationBasis::Inherited) => Ok("inherited"),
        Err(_) => Err("CONSENT_REQUIRED"),
    }
}

/// 一个 MCP 客户端独占的桌面会话。
pub struct Desktop {
    broker: Option<Broker>,
    session: Option<String>,
    frame: Option<String>,
    /// 当前会话是否以 authorizationMode=session 打开（省略确认字段的依据）。
    session_scoped: bool,
    /// 私有捕获目录按需建立；失败保留稳定诊断供 computer_status 如实报告。
    directory: CaptureDirectory,
    cancellation: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
}

/// 捕获目录采用的父目录来源标识；采用的父目录值在各状态里如实报告。
const CAPTURE_PARENT_SOURCE: &str = "std::env::temp_dir()";

/// 捕获目录失败的稳定诊断：分类与阶段可直接定位平台错误。
///
/// 只携带六类差异与路径事实，不包含 SID、SDDL、token 或任何凭据内容。
#[derive(Clone, Copy)]
struct CaptureDirectoryDiagnostic {
    reason: &'static str,
    stage: &'static str,
}

impl CaptureDirectoryDiagnostic {
    /// 转为工具层失败；details 附带失败阶段、分类与采用的父目录。
    ///
    /// 父目录按采用值展示：TEMP 可能是 8.3 短路径别名（如指向自定义目录），
    /// 调用方需要看到真实采用值才能定位环境问题。
    fn broker_failure(&self, parent: &Path) -> BrokerFailure {
        BrokerFailure {
            code: "CAPTURE_DIRECTORY_UNAVAILABLE".to_owned(),
            message: format!(
                "Cannot establish the private capture directory (reason: {}, stage: {}); adopted parent: {}.",
                self.reason,
                self.stage,
                parent.to_string_lossy(),
            ),
            outcome_unknown: false,
            accepted_may_have_occurred: false,
            details: json!({
                "stage": self.stage,
                "reason": self.reason,
                "parent": parent.to_string_lossy(),
            }),
        }
    }
}

/// 私有捕获目录的按需生命周期：父目录固定采用，状态按需推进。
struct CaptureDirectory {
    /// 采用的父目录（生产来自 std::env::temp_dir()）。
    parent: PathBuf,
    /// 目录当前状态。
    state: CaptureDirectoryState,
}

/// 捕获目录在按需建立过程中的状态。
enum CaptureDirectoryState {
    /// 尚未尝试建立；initialize、tools/list 与 computer_status 都不依赖它。
    Uninitialized,
    /// 已创建并通过平台私有性验证的目录。
    Ready(PathBuf),
    /// 最近一次建立失败的诊断；下一次依赖动作会重试并刷新。
    Failed(CaptureDirectoryDiagnostic),
}

impl CaptureDirectory {
    /// 固定采用的父目录，从未初始化状态开始。
    fn new(parent: PathBuf) -> Self {
        Self {
            parent,
            state: CaptureDirectoryState::Uninitialized,
        }
    }

    /// 确保目录已就绪；首次与失败后的调用都会真实建立并验证。
    fn ensure_ready(&mut self) -> Result<(), BrokerFailure> {
        if matches!(self.state, CaptureDirectoryState::Ready(_)) {
            return Ok(());
        }
        let name = capture_directory_name(std::process::id());
        match create_capture_directory(&self.parent, &name) {
            Ok(path) => {
                self.state = CaptureDirectoryState::Ready(path);
                Ok(())
            }
            Err(diagnostic) => {
                let failure = diagnostic.broker_failure(&self.parent);
                self.state = CaptureDirectoryState::Failed(diagnostic);
                Err(failure)
            }
        }
    }

    /// 已就绪目录路径；只应在授权与资源门通过后的捕获路径上调用。
    fn ready_path(&self) -> Result<&Path, BrokerFailure> {
        match &self.state {
            CaptureDirectoryState::Ready(path) => Ok(path),
            state @ (CaptureDirectoryState::Uninitialized
            | CaptureDirectoryState::Failed(_)) => {
                let diagnostic = match state {
                    CaptureDirectoryState::Failed(diagnostic) => *diagnostic,
                    _ => CaptureDirectoryDiagnostic {
                        reason: "not-initialized",
                        stage: "require-capture-directory",
                    },
                };
                Err(diagnostic.broker_failure(&self.parent))
            }
        }
    }

    /// computer_status 用的诚实报告：未初始化、可用或降级原因与来源。
    fn status_report(&self) -> Value {
        let parent = self.parent.to_string_lossy();
        match &self.state {
            CaptureDirectoryState::Uninitialized => json!({
                "state": "uninitialized",
                "parentSource": CAPTURE_PARENT_SOURCE,
            }),
            CaptureDirectoryState::Ready(path) => json!({
                "state": "ready",
                "directory": path.to_string_lossy(),
                "parent": parent,
                "parentSource": CAPTURE_PARENT_SOURCE,
            }),
            CaptureDirectoryState::Failed(diagnostic) => json!({
                "state": "unavailable",
                "reason": diagnostic.reason,
                "stage": diagnostic.stage,
                "parent": parent,
                "parentSource": CAPTURE_PARENT_SOURCE,
            }),
        }
    }

    /// 释放已建立的目录；未建立或失败状态不产生文件系统动作。
    fn remove(&mut self) {
        if let CaptureDirectoryState::Ready(path) =
            std::mem::replace(&mut self.state, CaptureDirectoryState::Uninitialized)
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// 在采用父目录内建立验证过的私有捕获目录。
///
/// Windows 走 owner-only 组件：创建即安装受保护 DACL 并回读逐值验证；
/// 六类错误差异完整保留，不吞成泛化授权或图像投递错误。
#[cfg(target_os = "windows")]
fn create_capture_directory(
    parent: &Path,
    name: &str,
) -> Result<PathBuf, CaptureDirectoryDiagnostic> {
    use crate::components::owner_only_directory_windows::{
        OwnerOnlyDirectoryError, ensure_owner_only_child,
    };
    ensure_owner_only_child(parent, name).map_err(|error| match error {
        OwnerOnlyDirectoryError::InvalidParent => CaptureDirectoryDiagnostic {
            reason: "invalid-parent",
            stage: "validate-parent",
        },
        OwnerOnlyDirectoryError::InvalidName => CaptureDirectoryDiagnostic {
            reason: "invalid-name",
            stage: "validate-name",
        },
        OwnerOnlyDirectoryError::SecurityUnavailable => CaptureDirectoryDiagnostic {
            reason: "security-unavailable",
            stage: "security-descriptor",
        },
        OwnerOnlyDirectoryError::DirectoryUnavailable => CaptureDirectoryDiagnostic {
            reason: "directory-unavailable",
            stage: "create-directory",
        },
        OwnerOnlyDirectoryError::DirectoryUntrusted => CaptureDirectoryDiagnostic {
            reason: "directory-untrusted",
            stage: "validate-created-directory",
        },
        OwnerOnlyDirectoryError::PermissionUnavailable => CaptureDirectoryDiagnostic {
            reason: "permission-unavailable",
            stage: "install-or-verify-permissions",
        },
    })
}

/// Unix 路线：0700 私有目录，父目录必须已存在。
#[cfg(not(target_os = "windows"))]
fn create_capture_directory(
    parent: &Path,
    name: &str,
) -> Result<PathBuf, CaptureDirectoryDiagnostic> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(parent.join(name))
        .map(|_| parent.join(name))
        .map_err(|_| CaptureDirectoryDiagnostic {
            reason: "create-directory-failed",
            stage: "create-directory",
        })
}

impl Desktop {
    /// 建立会话容器；此时不启动 broker、不打开 Portal，也不建立捕获目录。
    pub fn new() -> Self {
        Self {
            broker: None,
            session: None,
            frame: None,
            session_scoped: false,
            directory: CaptureDirectory::new(std::env::temp_dir()),
            cancellation: Default::default(),
        }
    }

    /// 测试注入：替换捕获目录父路径并回到未初始化状态，经真实调用边界驱动失败。
    #[cfg(test)]
    pub(crate) fn set_capture_parent_for_tests(&mut self, parent: PathBuf) {
        self.directory = CaptureDirectory::new(parent);
    }

    pub(crate) fn set_cancellation(
        &mut self,
        token: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    ) {
        self.cancellation = token.clone();
        if let Some(broker) = self.broker.as_mut() {
            broker.set_cancellation(token);
        }
    }

    /// 执行一次工具调用。
    pub fn call(&mut self, name: &str, arguments: &Value) -> ToolOutcome {
        match self.dispatch(name, arguments) {
            Ok(outcome) => outcome,
            Err(failure) => ToolOutcome::failed(&failure),
        }
    }

    /// 分派单个工具；未知工具在此闭合。
    fn dispatch(&mut self, name: &str, arguments: &Value) -> Result<ToolOutcome, BrokerFailure> {
        if self.cancellation.is_cancelled() {
            return Err(BrokerFailure::failed(
                "CANCELLED",
                "Request cancelled before dispatch.",
            ));
        }
        let arguments = arguments.as_object().cloned().ok_or_else(|| {
            BrokerFailure::failed("INVALID_ARGUMENT", "Tool arguments must be an object.")
        })?;
        if let Some(schema) = super::tools::input_schema(name) {
            let properties = &schema["properties"];
            for (key, value) in &arguments {
                let Some(spec) = properties.get(key) else {
                    return Err(BrokerFailure::failed(
                        "INVALID_ARGUMENT",
                        "Unknown tool argument.",
                    ));
                };
                if let Some(allowed) = spec.get("enum").and_then(Value::as_array) {
                    if !allowed.contains(value) {
                        return Err(BrokerFailure::failed(
                            "INVALID_ARGUMENT",
                            "Tool argument is outside the declared enum.",
                        ));
                    }
                }
                let valid = match spec["type"].as_str() {
                    Some("integer") => value.as_u64().is_some_and(|n| {
                        n >= spec["minimum"].as_u64().unwrap_or(0)
                            && n <= spec["maximum"].as_u64().unwrap_or(u64::MAX)
                    }),
                    Some("boolean") => value.is_boolean(),
                    Some("string") => value.is_string(),
                    Some("array") => value.is_array(),
                    _ => true,
                };
                if !valid {
                    return Err(BrokerFailure::failed(
                        "INVALID_ARGUMENT",
                        "Tool argument has an invalid type or range.",
                    ));
                }
            }
        }
        let flag = |key: &str| arguments.get(key).and_then(Value::as_bool) == Some(true);
        match name {
            "computer_status" => {
                let mut data = match self.broker.as_mut() {
                    Some(broker) => broker.call("sessions", json!({}), Duration::from_secs(70))?,
                    None => json!({ "sessions": [] }),
                };
                data = data.get("data").cloned().unwrap_or(data);
                // 捕获目录是本客户端独占资源：如实报告未初始化、可用或降级原因，
                // 目录不可用不让 status 连坐成失败。
                if let Some(target) = data.as_object_mut() {
                    target.insert(
                        "captureDirectory".to_owned(),
                        self.directory.status_report(),
                    );
                }
                Ok(ToolOutcome::text(&data))
            }
            "computer_connect" => {
                self.require_authorization(&arguments, true)?;
                self.connect(
                    &arguments,
                    flag("confirmed"),
                    flag("foregroundConsent"),
                    flag("strictIsolation"),
                )
            }
            "computer_disconnect" => self.disconnect(&arguments),
            "computer_authorization" => self.authorization(&arguments),
            "computer_observe" => {
                let basis = self.require_authorization(&arguments, false)?;
                self.require_capture_directory()?;
                self.require_session(&arguments)?;
                self.observe(&arguments, basis)
            }
            "computer_interact" | "computer_keys" | "computer_pointer" => {
                let basis = self.require_authorization(&arguments, true)?;
                self.require_capture_directory()?;
                self.require_session(&arguments)?;
                self.input(name, &arguments, basis)
            }
            "computer_run" => {
                let basis = self.require_authorization(&arguments, true)?;
                self.require_capture_directory()?;
                self.require_session(&arguments)?;
                self.run(&arguments, basis)
            }
            _ => Err(BrokerFailure::failed(
                "INVALID_ARGUMENT",
                "Unknown tool; call tools/list for the closed catalog.",
            )),
        }
    }

    /// 采集/输入共用的授权检查；授权不足时不触达桌面。
    ///
    /// connect 总是要求完整显式确认（一次确认点）；其余工具按
    /// [`authorization_basis`] 允许 session 会话继承省略字段。
    fn require_authorization(
        &self,
        arguments: &serde_json::Map<String, Value>,
        write: bool,
    ) -> Result<AuthorizationBasis, BrokerFailure> {
        authorization_basis(
            arguments.get("confirmed").and_then(Value::as_bool),
            arguments.get("foregroundConsent").and_then(Value::as_bool),
            arguments.get("strictIsolation").and_then(Value::as_bool),
            write,
            self.session_scoped,
            self.broker.is_some(),
        )
    }

    /// 捕获类动作的私有目录资源门：授权之后、会话归属之前按需建立。
    ///
    /// 目录不可用只拒绝依赖它的动作；status、disconnect、授权管理与
    /// initialize/tools/list 都不受连坐。失败被记住并如实报告，
    /// 下一次依赖动作会再次尝试建立。
    fn require_capture_directory(&mut self) -> Result<(), BrokerFailure> {
        self.directory.ensure_ready()
    }

    /// 校验调用方只使用本客户端返回的会话标识。
    fn require_session(
        &self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<(), BrokerFailure> {
        let requested = arguments.get("sessionId").and_then(Value::as_str);
        match (requested, self.session.as_deref()) {
            (Some(requested), Some(current)) if requested == current && self.broker.is_some() => {
                Ok(())
            }
            _ => Err(BrokerFailure::failed(
                "STALE_SESSION",
                "Connect and use this client's returned sessionId.",
            )),
        }
    }

    /// 输入类工具必须引用最新帧；旧帧一律失效。
    fn require_frame(
        &self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<(), BrokerFailure> {
        let requested = arguments.get("frameId").and_then(Value::as_str);
        match (requested, self.frame.as_deref()) {
            (Some(requested), Some(current)) if requested == current => Ok(()),
            _ => Err(BrokerFailure::failed(
                "STALE_FRAME",
                "Observe again and use the latest frameId; inputs are never replayed.",
            )),
        }
    }

    /// 建立独占会话。
    fn connect(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
        confirmed: bool,
        foreground: bool,
        isolated: bool,
    ) -> Result<ToolOutcome, BrokerFailure> {
        if !confirmed || !foreground || isolated {
            return Err(BrokerFailure::failed(
                "CONSENT_REQUIRED",
                "Desktop foreground requires existing user authorization and strictIsolation=false.",
            ));
        }
        if self.broker.is_some() {
            return Err(BrokerFailure::failed(
                "ALREADY_CONNECTED",
                "Disconnect the current client first; no implicit reconnection.",
            ));
        }
        let program = std::env::current_exe()
            .map_err(|error| BrokerFailure::failed("BROKER_START_FAILED", error.to_string()))?;
        let program = program.to_string_lossy().to_string();
        let timeout_ms = arguments
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(30_000);
        let session_scope =
            arguments.get("authorizationMode").and_then(Value::as_str) == Some("session");
        let remember = arguments
            .get("rememberAuthorization")
            .and_then(Value::as_bool)
            == Some(true);
        let mut broker = Broker::start(&program)?;
        broker.set_cancellation(self.cancellation.clone());
        let mut open = json!({
            "confirmed": true,
            "foregroundConsent": true,
            "strictIsolation": false,
            "timeoutMs": timeout_ms,
        });
        if session_scope {
            open["authorizationScope"] = json!("session");
        }
        if remember {
            open["rememberAuthorization"] = json!(true);
        }
        let response = broker.call(
            "open",
            open,
            Duration::from_millis(timeout_ms) + Duration::from_secs(5),
        );
        match response {
            Ok(response) => {
                let data = response.get("data").cloned().unwrap_or(json!({}));
                let session = data
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        BrokerFailure::failed(
                            "BROKER_PROTOCOL_FAILED",
                            "Broker returned no sessionId.",
                        )
                    })?
                    .to_owned();
                if self.cancellation.is_cancelled() {
                    let _ = broker.call(
                        "close",
                        json!({"sessionId":session}),
                        Duration::from_secs(5),
                    );
                    broker.stop();
                    return Err(BrokerFailure::failed(
                        "CANCELLED",
                        "Connection closed after cancellation.",
                    ));
                }
                self.session = Some(session);
                self.session_scoped = data
                    .get("authorization")
                    .and_then(|value| value.get("mode"))
                    == Some(&json!("session"))
                    || session_scope;
                self.broker = Some(broker);
                Ok(ToolOutcome::text(&data))
            }
            Err(failure) => {
                broker.stop();
                Err(failure)
            }
        }
    }

    /// 查看或撤销本工具记住的桌面授权；与 CLI/broker 共用同一授权事实。
    fn authorization(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutcome, BrokerFailure> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let operation = match action {
            "status" => "authorization-status",
            "forget" => "forget-authorization",
            _ => {
                return Err(BrokerFailure::failed(
                    "INVALID_ARGUMENT",
                    "computer_authorization action must be 'status' or 'forget'.",
                ));
            }
        };
        let response = match self.broker.as_mut() {
            Some(broker) => broker.call(operation, json!({}), Duration::from_secs(20))?,
            None => {
                // 未连接时临时拉起 broker 读取/清除共享的用户级保存授权。
                let program = std::env::current_exe().map_err(|error| {
                    BrokerFailure::failed("BROKER_START_FAILED", error.to_string())
                })?;
                let program = program.to_string_lossy().to_string();
                let mut broker = Broker::start(&program)?;
                let response = broker.call(operation, json!({}), Duration::from_secs(20));
                let response = match response {
                    Ok(response) => response,
                    Err(failure) => {
                        broker.stop();
                        return Err(failure);
                    }
                };
                broker.stop();
                response
            }
        };
        if operation == "forget-authorization" {
            // 撤销同时停止本客户端 live 会话：重置本地会话状态并释放 broker。
            self.session = None;
            self.frame = None;
            self.session_scoped = false;
            if let Some(mut broker) = self.broker.take() {
                broker.stop();
            }
        }
        let data = response.get("data").cloned().unwrap_or(json!({}));
        Ok(ToolOutcome::text(&data))
    }

    /// 关闭会话、读回空 sessions 并释放 broker。
    fn disconnect(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.require_session(arguments)?;
        let session = self.session.clone().unwrap_or_default();
        let Some(mut broker) = self.broker.take() else {
            return Err(BrokerFailure::failed("BROKER_CLOSED", "No active broker."));
        };
        let result = (|| {
            let closed = broker.call(
                "close",
                json!({ "sessionId": session }),
                Duration::from_secs(70),
            )?;
            let sessions = broker.call("sessions", json!({}), Duration::from_secs(70))?;
            let listed = sessions
                .get("data")
                .and_then(|data| data.get("sessions"))
                .cloned()
                .unwrap_or(json!([]));
            if listed != json!([]) {
                return Err(BrokerFailure::failed(
                    "CLEANUP_FAILED",
                    "The broker still reports open sessions.",
                ));
            }
            let mut data = closed.get("data").cloned().unwrap_or(json!({}));
            if let (Some(target), Some(extra)) = (
                data.as_object_mut(),
                sessions.get("data").and_then(Value::as_object),
            ) {
                for (key, value) in extra {
                    target.insert(key.clone(), value.clone());
                }
            }
            Ok(ToolOutcome::text(&data))
        })();
        self.session = None;
        self.frame = None;
        self.session_scoped = false;
        broker.stop();
        result
    }

    /// 截图并内联返回 PNG。
    fn observe(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
        basis: AuthorizationBasis,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.frame = None;
        let capture = self.capture_fields(arguments)?;
        let session = self.session.clone().unwrap_or_default();
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
        let response = broker.call(
            "observe",
            with_extra(
                json!({
                    "sessionId": session,
                    "input": capture,
                }),
                basis.capture_fields(),
            ),
            Duration::from_secs(70),
        )?;
        self.image_result(&response, &capture)
    }

    /// 发送输入并在同一请求内读回截图。
    fn input(
        &mut self,
        name: &str,
        arguments: &serde_json::Map<String, Value>,
        basis: AuthorizationBasis,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.require_frame(arguments)?;
        let frame = arguments
            .get("frameId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // 预检拒绝（acceptedMayHaveOccurred=false）没有投递任何事件，观察仍然成立，
        // 这一帧不该被消耗；只有输入可能已经送出时才作废旧帧。
        let keep_alive = frame.clone();
        self.frame = None;
        let capture = self.capture_fields(arguments)?;
        let session = self.session.clone().unwrap_or_default();
        let timeout_ms = arguments
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(3_000);
        let steps = match name {
            "computer_keys" => {
                let keys = arguments.get("keys").cloned().unwrap_or(json!([]));
                json!([{ "type": "key", "keys": keys }])
            }
            _ => arguments.get("steps").cloned().unwrap_or(json!([])),
        };
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
        if name == "computer_pointer" {
            let interaction = match broker.call(
                "input-pointer",
                with_extra(
                    json!({
                        "sessionId": session,
                        "input": {
                            "coordinateSpace": super::tools::relative_coordinate_space(),
                            "steps": steps,
                            "timeoutMs": timeout_ms,
                        },
                    }),
                    basis.input_fields(),
                ),
                Duration::from_millis(timeout_ms) + Duration::from_secs(5),
            ) {
                Ok(interaction) => interaction,
                Err(failure) => {
                    restore_undelivered_frame(&mut self.frame, &failure, keep_alive);
                    return Err(failure);
                }
            };
            let interaction = interaction.get("data").cloned().unwrap_or(json!({}));
            let response = match broker.call(
                "observe",
                with_extra(
                    json!({
                        "sessionId": session,
                        "input": capture,
                    }),
                    basis.capture_fields(),
                ),
                Duration::from_secs(70),
            ) {
                Ok(response) => response,
                Err(error) => {
                    // 输入已投递但观察失败：保留事实，禁止自动重放。
                    return Err(BrokerFailure {
                        code: "OBSERVATION_FAILED_AFTER_INPUT".to_owned(),
                        message: format!(
                            "Input was dispatched but the follow-up observation failed: {}",
                            error.message
                        ),
                        outcome_unknown: true,
                        accepted_may_have_occurred: true,
                        details: error.details,
                    });
                }
            };
            let mut outcome = self.image_result(&response, &capture)?;
            // 附上相对指针交互的回执，供调用方核对。
            if let Some(first) = outcome.content.first_mut() {
                let mut combined = first
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .unwrap_or(json!({}));
                if let Some(target) = combined.as_object_mut() {
                    target.insert("interaction".to_owned(), interaction);
                }
                *first = json!({ "type": "text", "text": combined.to_string() });
            }
            return Ok(outcome);
        }
        let response = match broker.call(
            "interact",
            with_extra(
                json!({
                    "sessionId": session,
                    "input": { "frameId": frame, "steps": steps, "timeoutMs": timeout_ms },
                    "observation": capture,
                }),
                basis.input_fields(),
            ),
            Duration::from_millis(timeout_ms) + Duration::from_secs(70),
        ) {
            Ok(response) => response,
            Err(failure) => {
                restore_undelivered_frame(&mut self.frame, &failure, keep_alive);
                return Err(failure);
            }
        };
        self.image_result(&response, &capture)
    }

    /// 长流程批量执行：服务端在内部循环「补帧 → 送一批 → 读回新帧」，把多批合成一次工具调用。
    ///
    /// 帧契约不变：每一批都用送出当时的最新帧。任一环节失败都停止后续批次并保留事实，
    /// 绝不自动重放；回读图像数量有界，避免把整段流程的截图都塞回模型。
    fn run(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
        basis: AuthorizationBasis,
    ) -> Result<ToolOutcome, BrokerFailure> {
        let batches = arguments
            .get("batches")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if batches.is_empty() || batches.len() > MAXIMUM_RUN_BATCHES {
            return Err(BrokerFailure::failed(
                "INVALID_ARGUMENT",
                format!("computer_run requires 1..={MAXIMUM_RUN_BATCHES} batches."),
            ));
        }
        let total = batches.len();
        let capture_every = arguments
            .get("captureEveryBatches")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let max_frames = arguments
            .get("maxFrames")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, MAXIMUM_RUN_FRAMES as u64) as usize;
        let stop_on_error = arguments
            .get("stopOnError")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let total_ms = arguments
            .get("totalTimeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_RUN_TOTAL_MS)
            .clamp(1_000, MAXIMUM_RUN_TOTAL_MS);
        let started = std::time::Instant::now();
        let session = self.session.clone().unwrap_or_default();

        // 起始帧由 run 自己补：调用方不必先 observe，也不必逐批给 frameId。
        self.frame = None;
        let mut frame = self.observe_frame(&session, arguments, basis)?;
        let mut records: Vec<Value> = Vec::new();
        let mut frames: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut stopped: Option<Value> = None;

        for (index, batch) in batches.iter().enumerate() {
            if self.cancellation.is_cancelled() {
                stopped = Some(json!({ "reason": "CANCELLED", "batch": index + 1 }));
                break;
            }
            if started.elapsed().as_millis() as u64 >= total_ms {
                stopped = Some(json!({ "reason": "TOTAL_TIMEOUT", "batch": index + 1 }));
                break;
            }
            let steps = batch.get("steps").cloned().unwrap_or(json!([]));
            let timeout_ms = batch
                .get("timeoutMs")
                .and_then(Value::as_u64)
                .unwrap_or(3_000)
                .clamp(1, 30_000);
            let capture = self.capture_fields(arguments)?;
            let path = capture
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let broker = self
                .broker
                .as_mut()
                .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
            let response = broker.call(
                "interact",
                with_extra(
                    json!({
                        "sessionId": session,
                        "input": { "frameId": frame, "steps": steps, "timeoutMs": timeout_ms },
                        "observation": capture,
                    }),
                    basis.input_fields(),
                ),
                Duration::from_millis(timeout_ms) + Duration::from_secs(70),
            );
            let response = match response {
                Ok(response) => response,
                Err(failure) => {
                    // 输入可能已经送出：这一帧作废，不重放；预检拒绝才允许补帧继续。
                    self.frame = None;
                    let may_have_occurred = failure.accepted_may_have_occurred;
                    records.push(json!({
                        "batch": index + 1,
                        "name": batch.get("name"),
                        "accepted": false,
                        "error": failure.payload(),
                        "acceptedMayHaveOccurred": may_have_occurred,
                    }));
                    if stop_on_error || may_have_occurred {
                        stopped = Some(json!({
                            "reason": "INPUT_FAILED",
                            "batch": index + 1,
                            "message": failure.message,
                        }));
                        break;
                    }
                    frame = self.observe_frame(&session, arguments, basis)?;
                    continue;
                }
            };
            let data = response.get("data").cloned().unwrap_or(json!({}));
            let observation = data
                .get("observation")
                .cloned()
                .unwrap_or_else(|| data.clone());
            let next = observation
                .get("frameId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            records.push(json!({
                "batch": index + 1,
                "name": batch.get("name"),
                "accepted": true,
                "completedSteps": data.get("completedSteps"),
                "inputEventsSent": data.get("inputEventsSent"),
                "effectConfirmed": data.get("effectConfirmed"),
                "frameId": next,
            }));
            // 关键帧回读：只读被选中的批次，且张数有界。
            if wants_frame(index, total, capture_every) && frames.len() < max_frames {
                if let Ok(raw) =
                    read_capture(Path::new(&path), &observation, Some(session.as_str()))
                {
                    frames.push((index + 1, raw));
                }
            }
            let _ = fs::remove_file(&path);
            match next {
                Some(next) => {
                    frame = next.clone();
                    self.frame = Some(next);
                }
                None => {
                    self.frame = None;
                    frame = self.observe_frame(&session, arguments, basis)?;
                }
            }
        }

        let mut summary = json!({
            "requestedBatches": total,
            "executedBatches": records.len(),
            "batches": records,
            "capturedFrames": frames.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            "elapsedMs": started.elapsed().as_millis() as u64,
        });
        if let (Some(target), Some(reason)) = (summary.as_object_mut(), stopped) {
            target.insert("stopped".to_owned(), reason);
        }
        let mut content = vec![json!({ "type": "text", "text": summary.to_string() })];
        for (_, raw) in &frames {
            content.push(json!({
                "type": "image",
                "mimeType": "image/png",
                "data": BASE64_STANDARD.encode(raw),
            }));
        }
        Ok(ToolOutcome {
            content,
            is_error: false,
        })
    }

    /// 只补帧不回图：长流程内部用，帧内容不必回传模型。
    ///
    /// 内部帧与回读帧使用同一个 maxDimension：observation-px 由捕获图尺寸决定，
    /// 两者不一致会让调用方按前一张图算出的坐标落到另一套坐标系里。
    fn observe_frame(
        &mut self,
        session: &str,
        arguments: &serde_json::Map<String, Value>,
        basis: AuthorizationBasis,
    ) -> Result<String, BrokerFailure> {
        let capture = self.capture_fields(arguments)?;
        let path = capture
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
        let response = broker.call(
            "observe",
            with_extra(
                json!({
                    "sessionId": session,
                    "input": capture,
                }),
                basis.capture_fields(),
            ),
            Duration::from_secs(70),
        );
        let _ = fs::remove_file(&path);
        let response = response?;
        let data = response.get("data").cloned().unwrap_or(json!({}));
        let observation = data.get("observation").cloned().unwrap_or(data);
        let frame = observation
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                BrokerFailure::failed("OBSERVATION_FAILED", "Observation returned no frameId.")
            })?;
        self.frame = Some(frame.clone());
        Ok(frame)
    }

    /// 构造截图请求字段：私有临时文件 + 有界尺寸。
    ///
    /// 路径来自已就绪的私有捕获目录；资源门未通过时这里防御性失败，
    /// 不落到其他可写位置。
    fn capture_fields(
        &self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<Value, BrokerFailure> {
        let path = self
            .directory
            .ready_path()?
            .join(format!("{}.png", unique_name()));
        Ok(json!({
            "path": path.to_string_lossy(),
            "maxDimension": arguments
                .get("maxDimension")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_MAX_DIMENSION),
            "timeoutMs": 5000,
        }))
    }

    /// 读取截图为内联 MCP 图像；任何不一致都收敛为投递失败。
    fn image_result(
        &mut self,
        response: &Value,
        capture: &Value,
    ) -> Result<ToolOutcome, BrokerFailure> {
        let data = response.get("data").cloned().unwrap_or(json!({}));
        let observation = data
            .get("observation")
            .cloned()
            .unwrap_or_else(|| data.clone());
        self.frame = observation
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let path = capture
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = read_capture(Path::new(path), &observation, self.session.as_deref());
        // 隐私：无论成功与否都移除这份瞬时文件。
        let _ = fs::remove_file(path);
        match result {
            Ok(raw) => {
                let mut clean = observation.clone();
                if let Some(target) = clean.as_object_mut() {
                    target.remove("path");
                }
                let mut details = data.clone();
                match details.as_object_mut() {
                    Some(target) if target.contains_key("observation") => {
                        target.insert("observation".to_owned(), clean);
                    }
                    _ => details = clean,
                }
                Ok(ToolOutcome::image(&details, &raw))
            }
            Err(failure) => {
                // 帧已不可信；要求重新观察而不是重放输入。
                self.frame = None;
                Err(failure)
            }
        }
    }

    /// 释放本客户端资源；EOF 与信号退出都走这里。
    pub fn dispose(&mut self) {
        self.session = None;
        self.frame = None;
        self.session_scoped = false;
        if let Some(mut broker) = self.broker.take() {
            broker.stop();
        }
        self.directory.remove();
    }
}

/// 该批次是否需要回读图像：总是回读最后一批，其余按 captureEveryBatches 采样。
fn wants_frame(index: usize, total: usize, capture_every: u64) -> bool {
    if index + 1 == total {
        return true;
    }
    capture_every > 0 && (index as u64 + 1) % capture_every == 0
}

/// 把额外字段合并进 broker 请求对象；继承授权时 extra 为空对象即完全省略。
fn with_extra(mut request: Value, extra: Value) -> Value {
    if let (Some(target), Some(fields)) = (request.as_object_mut(), extra.as_object()) {
        for (key, value) in fields {
            target.insert(key.clone(), value.clone());
        }
    }
    request
}

/// 预检拒绝时把帧还给会话：broker 明确报告没有投递事件，观察结论仍然成立。
/// 一旦输入可能已经送出（含结果未知），保持失效，绝不自动重放。
fn restore_undelivered_frame(frame: &mut Option<String>, failure: &BrokerFailure, frame_id: String) {
    if !failure.accepted_may_have_occurred {
        *frame = Some(frame_id);
    }
}

/// 校验并读取截图文件；身份不一致、软链接、超大或非 PNG 都拒绝。
fn read_capture(
    path: &Path,
    observation: &Value,
    session: Option<&str>,
) -> Result<Vec<u8>, BrokerFailure> {
    let declared = observation.get("path").and_then(Value::as_str);
    let observed_session = observation.get("sessionId").and_then(Value::as_str);
    if declared != Some(path.to_string_lossy().as_ref()) || observed_session != session {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture identity mismatch; input may already have completed. Observe, never replay.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
            details: Value::Null,
        });
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| BrokerFailure {
        code: "IMAGE_DELIVERY_FAILED".to_owned(),
        message: format!("Capture is unavailable: {error}"),
        outcome_unknown: true,
        accepted_may_have_occurred: true,
        details: Value::Null,
    })?;
    if metadata.file_type().is_symlink() || metadata.len() > MAXIMUM_PNG_BYTES as u64 {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture is a symlink or exceeds the inline image limit.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
            details: Value::Null,
        });
    }
    let raw = fs::read(path).map_err(|error| BrokerFailure {
        code: "IMAGE_DELIVERY_FAILED".to_owned(),
        message: format!("Capture cannot be read: {error}"),
        outcome_unknown: true,
        accepted_may_have_occurred: true,
        details: Value::Null,
    })?;
    if !raw.starts_with(PNG_MAGIC) {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture is not a PNG image.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
            details: Value::Null,
        });
    }
    Ok(raw)
}

/// 取得进程内唯一组件：单调纳秒时间戳与进程内递增序号。
fn unique_components() -> (u64, u64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    (nanos, sequence)
}

/// 生成进程内唯一的一次性图像名。
fn unique_name() -> String {
    let (nanos, sequence) = unique_components();
    format!("{nanos:032x}{sequence:016x}")
}

/// 生成 MCP 私有捕获目录的单段名：固定前缀 + 进程号 + 紧凑唯一后缀。
///
/// 后缀 = 16 位十六进制纳秒（u64 精确值）+ 12 位十六进制序号（截 48 位：
/// 同一纳秒内完成 2^48 次递增物理上不可能，截断不损害唯一性），长度按构造
/// 封顶；最坏 21+10+1+28=60 字节，覆盖最大 u32 进程号仍低于 Windows
/// owner-only 组件的 64 字节单段上限，字符集保持小写 ASCII、数字与连字符。
pub(crate) fn capture_directory_name(pid: u32) -> String {
    let (nanos, sequence) = unique_components();
    let sequence = sequence & 0xffff_ffff_ffff;
    format!("computer-control-mcp-{pid}-{nanos:016x}{sequence:012x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_run_reads_back_the_last_batch_and_sampled_batches() {
        // 默认只回读最后一批：长流程的截图数量不随批次数增长。
        assert!(!wants_frame(0, 4, 0));
        assert!(wants_frame(3, 4, 0));
        // 采样时按间隔回读，最后一批始终回读。
        assert!(wants_frame(1, 4, 2));
        assert!(!wants_frame(2, 4, 2));
        assert!(wants_frame(3, 4, 2));
        // 单批也要回读，调用方才看得到结果。
        assert!(wants_frame(0, 1, 0));
    }

    #[test]
    fn capture_directory_names_stay_within_the_component_bound() {
        for pid in [1_u32, std::process::id(), u32::MAX] {
            // 真实生成器输出的字符集必须与 Windows owner-only 组件的固定校验一致。
            let name = capture_directory_name(pid);
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
                "generated name must keep the fixed charset: {name}"
            );
            assert!(
                name.len() <= 64,
                "generated name must fit the 64-byte segment bound: {name}"
            );
        }
        // 最坏长度按构造封顶：前缀（含尾分隔）21 + 最大进程号 10 + 分隔 1 + 后缀 28。
        assert_eq!(capture_directory_name(u32::MAX).len(), 60);
    }

    #[test]
    fn capture_directory_names_stay_unique_and_file_names_keep_their_shape() {
        // 同一进程内连号生成不得碰撞：纳秒时间戳与序号共同保证唯一性。
        let names = std::iter::repeat_with(|| capture_directory_name(7))
            .take(64)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), 64, "rapid generation must not collide");
        // 截图文件命名继续使用 48 位十六进制一次性名，与目录名单互不冲突。
        assert_eq!(format!("{}.png", unique_name()).len(), 52);
    }

    #[test]
    fn pre_dispatch_rejection_keeps_the_observation_alive() {
        let frame = "a".repeat(32);
        // 预检拒绝没有投递任何事件，观察仍然成立：帧还给会话，省掉一次重新观察。
        let mut current = None;
        restore_undelivered_frame(
            &mut current,
            &BrokerFailure::failed("INVALID_ARGUMENT", "rejected before dispatch"),
            frame.clone(),
        );
        assert_eq!(current.as_deref(), Some(frame.as_str()));
        // 输入可能已经送出（含结果未知）：调用前帧已作废，这里必须保持失效，只能重新观察。
        let mut dispatched = None;
        let mut failure = BrokerFailure::failed("OUTCOME_UNKNOWN", "input may have been sent");
        failure.accepted_may_have_occurred = true;
        restore_undelivered_frame(&mut dispatched, &failure, frame);
        assert_eq!(dispatched, None);
    }
}
