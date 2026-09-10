use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub type JsonMap = Map<String, Value>;
pub type AppResult<T> = Result<T, AppControlError>;

// 表示公开契约允许的封闭执行域。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
// 使用契约中的 kebab-case 文本。
#[serde(rename_all = "kebab-case")]
pub enum ExecutionRealm {
    // 表示主机无头只读执行。
    HostHeadless,
    // 表示主机后台但可能产生可见产物的执行。
    HostBackground,
    // 表示同一交互会话中不取得焦点的执行。
    SameSessionNoFocus,
    // 表示受 Job 生命周期约束的独立 companion worker。
    IsolatedWorker,
    // 表示会影响主机前台的执行。
    HostForeground,
    // 表示不存在可执行路径。
    None,
}

// 为执行域提供策略判断。
impl ExecutionRealm {
    // 返回稳定契约文本。
    pub const fn as_str(self) -> &'static str {
        // 映射全部封闭枚举值。
        match self {
            // 输出主机无头域。
            Self::HostHeadless => "host-headless",
            // 输出主机后台域。
            Self::HostBackground => "host-background",
            // 输出同会话无焦点域。
            Self::SameSessionNoFocus => "same-session-no-focus",
            // 输出隔离 worker 域。
            Self::IsolatedWorker => "isolated-worker",
            // 输出主机前台域。
            Self::HostForeground => "host-foreground",
            // 输出无执行域。
            Self::None => "none",
        }
    }

    // 判断执行域能否满足严格零打扰策略。
    pub const fn permits_strict_isolation(self) -> bool {
        // 仅认证无头路径与隔离 worker 可进入严格模式。
        matches!(self, Self::HostHeadless | Self::IsolatedWorker)
    }
}

// 表示调用方要求的隔离强度。
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
// 使用稳定 kebab-case 输入输出。
#[serde(rename_all = "kebab-case")]
pub enum IsolationRequirement {
    // 保留兼容的后台优先策略。
    #[default]
    Standard,
    // 要求严格零打扰且禁止前台或同会话降级。
    Strict,
}

// 为隔离要求提供不可降级合并。
impl IsolationRequirement {
    // 合并 CLI 与 JSON 输入时保留较强要求。
    pub const fn combine(self, other: Self) -> Self {
        // 任一来源要求严格时都不得被另一来源放宽。
        if matches!(self, Self::Strict) || matches!(other, Self::Strict) {
            // 返回严格要求。
            Self::Strict
        } else {
            // 两个来源均为标准要求。
            Self::Standard
        }
    }
}

// 表示 System 对主机影响的封闭策略。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
// 使用稳定 kebab-case 证据文本。
#[serde(rename_all = "kebab-case")]
pub enum HostImpactPolicy {
    // 表示现有后台优先兼容策略。
    BackgroundPreferred,
    // 表示严格零打扰且禁止复用前台同意。
    StrictNoInterference,
}

// 从调用方隔离要求推导不可变主机影响策略。
impl From<IsolationRequirement> for HostImpactPolicy {
    // 执行封闭映射。
    fn from(requirement: IsolationRequirement) -> Self {
        // 按两种隔离强度映射。
        match requirement {
            // 标准请求保持后台优先。
            IsolationRequirement::Standard => Self::BackgroundPreferred,
            // 严格请求要求零打扰。
            IsolationRequirement::Strict => Self::StrictNoInterference,
        }
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppControlError {
    pub code: &'static str,
    pub message: String,
    pub details: Value,
}

impl AppControlError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Value::Null,
        }
    }

    pub fn with_details(code: &'static str, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into(),
            details,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Verb {
    Status,
    Sessions,
    Inspect,
    Run,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Sessions => "sessions",
            Self::Inspect => "inspect",
            Self::Run => "run",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub verb: Verb,
    pub app: String,
    pub operation: Option<String>,
    pub target: JsonMap,
    pub args: JsonMap,
    pub max_items: usize,
    pub max_depth: usize,
    pub confirmed: bool,
    pub foreground_consent: bool,
    // 保存调用方要求且不可由 provider 放宽的隔离强度。
    pub isolation_requirement: IsolationRequirement,
}

impl CommandRequest {
    pub fn read(verb: Verb, app: impl Into<String>) -> Self {
        Self {
            verb,
            app: app.into(),
            operation: None,
            target: JsonMap::new(),
            args: JsonMap::new(),
            max_items: 50,
            max_depth: 4,
            confirmed: false,
            foreground_consent: false,
            // 兼容请求默认使用后台优先策略。
            isolation_requirement: IsolationRequirement::Standard,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope<'a> {
    pub ok: bool,
    pub error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody<'a> {
    pub code: &'a str,
    pub message: &'a str,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: &'a Value,
}

pub fn error_json(error: &AppControlError) -> Value {
    serde_json::to_value(ErrorEnvelope {
        ok: false,
        error: ErrorBody {
            code: error.code,
            message: &error.message,
            details: &error.details,
        },
    })
    .unwrap_or_else(|serialization_error| {
        serde_json::json!({
            "ok": false,
            "error": { "code": "SERIALIZATION_FAILED", "message": serialization_error.to_string() }
        })
    })
}
