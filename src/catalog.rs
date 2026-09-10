use serde::{Serialize, Serializer};

// 导入 catalog 字段类型门禁需要检查的 JSON 值。
use serde_json::Value;

// 导入公开动词与强类型执行域。
use crate::domain::{ExecutionRealm, Verb};

mod app;
mod media;

use app::{APP_OPERATIONS, APP_VERBS};
use media::MEDIA_OPERATIONS;

// 表示 catalog 字段公开描述与运行时 JSON 类型的同源契约。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldValueType {
    // 表示普通字符串。
    String,
    // 表示带附加约束说明的字符串。
    DescribedString(&'static str),
    // 表示整数。
    Integer,
    // 表示带公开说明和可执行闭区间约束的整数。
    BoundedInteger(
        // 保存兼容既有 catalog 的公开说明。
        &'static str,
        // 保存允许的最小整数。
        i64,
        // 保存允许的最大整数。
        i64,
    ),
    // 表示任意 JSON 数值。
    Number,
    // 表示带公开说明和可执行闭区间约束的数值。
    BoundedNumber(
        // 保存兼容既有 catalog 的公开说明。
        &'static str,
        // 保存允许的最小数值。
        f64,
        // 保存允许的最大数值。
        f64,
    ),
    // 表示布尔值。
    Boolean,
    // 表示 JSON 对象。
    Object,
    // 表示只包含字符串的 JSON 数组。
    StringArray,
    // 表示不透明 session ID 字符串。
    OpaqueSessionId,
    // 表示版本化 capability ID 字符串。
    VersionedCapabilityId,
}

// 为字段类型提供 catalog 兼容文本和运行时类型门禁。
impl FieldValueType {
    // 返回既有 catalog 对外展示文本。
    pub const fn as_str(self) -> &'static str {
        // 按封闭类型输出稳定文本。
        match self {
            // 输出普通字符串类型。
            Self::String => "string",
            // 输出带约束的字符串说明。
            Self::DescribedString(description) => description,
            // 输出整数类型。
            Self::Integer => "integer",
            // 输出带范围的整数说明。
            Self::BoundedInteger(description, _, _) => description,
            // 输出普通数值类型。
            Self::Number => "number",
            // 输出带范围的数值说明。
            Self::BoundedNumber(description, _, _) => description,
            // 输出布尔类型。
            Self::Boolean => "boolean",
            // 输出对象类型。
            Self::Object => "object",
            // 输出字符串数组类型。
            Self::StringArray => "array<string>",
            // 输出 opaque session ID 语义类型。
            Self::OpaqueSessionId => "opaque-session-id",
            // 输出版本化 capability ID 语义类型。
            Self::VersionedCapabilityId => "versioned-capability-id",
        }
    }

    // 判断 JSON 值是否满足字段声明的基础类型。
    pub(crate) fn accepts_type(self, value: &Value) -> bool {
        // 只判断 catalog 拥有的 JSON 基础类型，不混入范围错误。
        match self {
            // 普通与带说明字符串都只接受 JSON 字符串。
            Self::String | Self::DescribedString(_) => value.is_string(),
            // 整数类型拒绝浮点 JSON Number。
            Self::Integer | Self::BoundedInteger(_, _, _) => {
                // 同时接受 serde_json 的有符号和无符号整数表示。
                value.as_i64().is_some() || value.as_u64().is_some()
            }
            // 数值类型接受整数和浮点 JSON Number。
            Self::Number | Self::BoundedNumber(_, _, _) => value.is_number(),
            // 布尔类型只接受 JSON boolean。
            Self::Boolean => value.is_boolean(),
            // 对象类型只接受 JSON object。
            Self::Object => value.is_object(),
            // 字符串数组要求每个元素都是字符串。
            Self::StringArray => value
                // 只处理数组。
                .as_array()
                // 核对全部元素类型，空数组仍是合法可选值。
                .is_some_and(|items| items.iter().all(Value::is_string)),
            // 两种语义 ID 在本层只认证 JSON 字符串，精确语义由所属边界继续校验。
            Self::OpaqueSessionId | Self::VersionedCapabilityId => value.is_string(),
        }
    }

    // 判断基础类型正确的 JSON 值是否满足 catalog 声明的数值约束。
    pub(crate) fn satisfies_constraint(self, value: &Value) -> bool {
        // 只有有界数值类型增加第二层约束，其余类型天然通过。
        match self {
            // 有界整数必须落在声明的闭区间内。
            Self::BoundedInteger(_, minimum, maximum) => value
                // JSON 整数统一读取为有符号表示；当前公开范围均可安全容纳。
                .as_i64()
                // 同时核对上下界。
                .is_some_and(|value| (minimum..=maximum).contains(&value)),
            // 有界数值必须落在声明的闭区间内。
            Self::BoundedNumber(_, minimum, maximum) => value
                // serde_json 只提供有限 JSON Number，因此可直接比较。
                .as_f64()
                // 同时核对上下界。
                .is_some_and(|value| (minimum..=maximum).contains(&value)),
            // 无额外数值约束的类型保持通过。
            _ => true,
        }
    }

    // 返回仅用于范围错误的稳定公开约束说明。
    pub(crate) fn constraint_description(self) -> Option<String> {
        // 从同一强类型约束生成诊断，不解析展示文本。
        match self {
            // 输出整数闭区间。
            Self::BoundedInteger(_, minimum, maximum) => {
                // 使用与 Rust 闭区间一致的公开记法。
                Some(format!("integer ({minimum}..={maximum})"))
            }
            // 输出数值闭区间。
            Self::BoundedNumber(_, minimum, maximum) => {
                // 使用 JSON 数值的紧凑展示。
                Some(format!("number ({minimum}..={maximum})"))
            }
            // 其他类型没有额外约束说明。
            _ => None,
        }
    }
}

// 保持 FieldDescriptor 的公开 JSON 仍输出既有类型字符串。
impl Serialize for FieldValueType {
    // 把强类型值序列化为稳定 catalog 文本。
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        // 使用任意 serde serializer，避免绑定具体输出格式。
        S: Serializer,
    {
        // 输出与迁移前逐字一致的文本。
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldDescriptor {
    pub name: &'static str,
    // 同时驱动公开 catalog 与 provider 前 JSON 类型校验。
    pub value_type: FieldValueType,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationDescriptor {
    pub id: &'static str,
    pub operation: &'static str,
    pub summary: &'static str,
    pub target_fields: &'static [FieldDescriptor],
    pub argument_fields: &'static [FieldDescriptor],
    pub mutates: bool,
    pub requires_confirmation: bool,
    pub requires_session_id: bool,
    pub background_policy: &'static str,
    // 输出与 assessment、运行请求和结果共享的精确执行域。
    #[serde(rename = "executionRealm")]
    pub execution_realm: ExecutionRealm,
    pub methods: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct AppDescriptor {
    pub id: &'static str,
    pub summary: &'static str,
    pub public_verbs: &'static [Verb],
    pub operations: &'static [OperationDescriptor],
    pub blocked: &'static [&'static str],
    pub background_policy: &'static str,
    // 输出 status、sessions 与普通 inspect 的强类型读取域。
    #[serde(rename = "readExecutionRealm")]
    pub read_execution_realm: ExecutionRealm,
}

const HWND_TARGET: &[FieldDescriptor] = &[FieldDescriptor {
    name: "sessionId",
    // 使用封闭字符串类型。
    value_type: FieldValueType::String,
    required: true,
}];
// 精确进程 mutation 只接受 canonical opaque process sessionId。
const PROCESS_TARGET: &[FieldDescriptor] = &[FieldDescriptor {
    name: "sessionId",
    value_type: FieldValueType::OpaqueSessionId,
    required: true,
}];
// 版本二温和终止把有界 deadline 封装在严格 input 对象中。
const PROCESS_TERMINATION_ARGS: &[FieldDescriptor] = &[FieldDescriptor {
    name: "input",
    value_type: FieldValueType::Object,
    required: true,
}];
const BROWSER_TARGET: &[FieldDescriptor] = &[FieldDescriptor {
    name: "url",
    // 使用封闭字符串类型。
    value_type: FieldValueType::String,
    required: true,
}];
const SCREENSHOT_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "path",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        required: true,
    },
    FieldDescriptor {
        name: "width",
        // 使用封闭整数类型。
        value_type: FieldValueType::Integer,
        required: false,
    },
    FieldDescriptor {
        name: "height",
        // 使用封闭整数类型。
        value_type: FieldValueType::Integer,
        required: false,
    },
    FieldDescriptor {
        name: "timeoutMs",
        // 保持公开整数文本并执行浏览器截图等待范围。
        value_type: FieldValueType::BoundedInteger("integer", 1_000, 300_000),
        required: false,
    },
    FieldDescriptor {
        name: "overwrite",
        // 使用封闭布尔类型。
        value_type: FieldValueType::Boolean,
        required: false,
    },
];
const DESKTOP_SCREENSHOT_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "path",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        required: true,
    },
    FieldDescriptor {
        name: "timeoutMs",
        // 保持公开整数文本并执行窗口截图等待范围。
        value_type: FieldValueType::BoundedInteger("integer", 250, 30_000),
        required: false,
    },
    FieldDescriptor {
        name: "overwrite",
        // 使用封闭布尔类型。
        value_type: FieldValueType::Boolean,
        required: false,
    },
];
// Portal 截图仅接受可原子落盘的 PNG 路径与有界等待参数。
const PORTAL_SCREENSHOT_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "path",
        value_type: FieldValueType::DescribedString("string (.png)"),
        required: true,
    },
    FieldDescriptor {
        name: "timeoutMs",
        value_type: FieldValueType::BoundedInteger(
            "integer (1000..=30000; default 30000)",
            1_000,
            30_000,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "overwrite",
        value_type: FieldValueType::Boolean,
        required: false,
    },
];
const DESKTOP_RECORD_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "path",
        // 使用带格式说明的封闭字符串类型。
        value_type: FieldValueType::DescribedString("string (.mp4)"),
        required: true,
    },
    FieldDescriptor {
        name: "durationMs",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (1000..=300000; default 30000)",
            // 复用公开录制时长下界。
            1_000,
            // 复用公开录制时长上界。
            300_000,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "fps",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger("integer (1..=10; default 2)", 1, 10),
        required: false,
    },
    FieldDescriptor {
        name: "maxWidth",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (320..=1920; default 960)",
            // 复用公开录制宽度下界。
            320,
            // 复用公开录制宽度上界。
            1_920,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "quality",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (1..=100; default 75)",
            // 复用公开录制质量下界。
            1,
            // 复用公开录制质量上界。
            100,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "maxKeyframes",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger("integer (2..=20; default 8)", 2, 20),
        required: false,
    },
    FieldDescriptor {
        name: "changeThreshold",
        // 使用带范围说明的封闭数值类型。
        value_type: FieldValueType::BoundedNumber(
            // 保持既有 catalog 文本。
            "number (0.005..=0.5; default 0.035)",
            // 复用公开变化阈值下界。
            0.005,
            // 复用公开变化阈值上界。
            0.5,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "analysisDir",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        required: false,
    },
    FieldDescriptor {
        name: "timeoutMs",
        // 使用带范围说明的封闭整数类型。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (250..=30000; default 5000)",
            // 复用公开单帧等待下界。
            250,
            // 复用公开单帧等待上界。
            30_000,
        ),
        required: false,
    },
    FieldDescriptor {
        name: "overwrite",
        // 使用封闭布尔类型。
        value_type: FieldValueType::Boolean,
        required: false,
    },
];
// 定义 Standard Edit 兼容与正式路线共享的公开参数。
const SET_TEXT_ARGS: &[FieldDescriptor] = &[
    // 声明必填 UTF-8 文本。
    FieldDescriptor {
        // 使用稳定字段名。
        name: "text",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        // 文本始终必填。
        required: true,
    },
    // 声明可选有界 deadline。
    FieldDescriptor {
        // 使用实现已读取的稳定字段名。
        name: "timeoutMs",
        // 输出实现已认证的范围和默认值。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (1..=30000; default 2000)",
            // 复用公开编辑等待下界。
            1,
            // 复用公开编辑等待上界。
            30_000,
        ),
        // 缺失时由 Standard Edit Module 使用默认值。
        required: false,
    },
];
const NOTEPAD_ARGS: &[FieldDescriptor] = &[FieldDescriptor {
    name: "text",
    // 使用封闭字符串类型。
    value_type: FieldValueType::String,
    required: true,
}];
// 定义显式前台按键操作的完整公开参数。
const KEY_ARGS: &[FieldDescriptor] = &[
    // 声明必填按键或组合键。
    FieldDescriptor {
        // 使用稳定字段名。
        name: "key",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        // 按键始终必填。
        required: true,
    },
    // 声明可选按住时长。
    FieldDescriptor {
        // 使用实现已读取的稳定字段名。
        name: "holdMs",
        // 输出实现已认证的范围和默认值。
        value_type: FieldValueType::BoundedInteger(
            // 保持既有 catalog 文本。
            "integer (0..=5000; default 0)",
            // 允许零时长表示完整按键动作。
            0,
            // 限制最长按住时长。
            5_000,
        ),
        // 缺失时表示不额外按住。
        required: false,
    },
    // 声明可选按键阶段。
    FieldDescriptor {
        // 使用实现已读取的稳定字段名。
        name: "phase",
        // 输出封闭枚举及其默认值。
        value_type: FieldValueType::DescribedString(
            // 保持公开文本简洁且 provider-neutral。
            "string (press|down|up; default press)",
        ),
        // 缺失时执行完整按下与释放。
        required: false,
    },
];
const CLICK_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "x",
        // 使用封闭整数类型。
        value_type: FieldValueType::Integer,
        required: true,
    },
    FieldDescriptor {
        name: "y",
        // 使用封闭整数类型。
        value_type: FieldValueType::Integer,
        required: true,
    },
];
const LAUNCH_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "path",
        // 使用封闭字符串类型。
        value_type: FieldValueType::String,
        required: true,
    },
    FieldDescriptor {
        name: "argv",
        // 使用封闭字符串数组类型。
        value_type: FieldValueType::StringArray,
        required: false,
    },
];
const BROWSER_OPERATIONS: &[OperationDescriptor] = &[OperationDescriptor {
    id: "browser.screenshot",
    operation: "screenshot",
    summary: "使用临时隔离 profile 的 headless Chromium 截图。",
    target_fields: BROWSER_TARGET,
    argument_fields: SCREENSHOT_ARGS,
    mutates: true,
    requires_confirmation: true,
    requires_session_id: false,
    background_policy: "guaranteed",
    // 正式浏览器路径固定使用 Rust 隔离 worker。
    execution_realm: ExecutionRealm::IsolatedWorker,
    methods: &["headless-browser"],
}];

const WIN32_OPERATIONS: &[OperationDescriptor] = &[OperationDescriptor {
    id: "win32-control.set-text",
    operation: "set-text",
    summary: "仅向标准 Edit 控件发送系统定义的 WM_SETTEXT，不激活窗口。",
    target_fields: HWND_TARGET,
    argument_fields: SET_TEXT_ARGS,
    mutates: true,
    requires_confirmation: true,
    requires_session_id: true,
    background_policy: "guaranteed",
    // 固定 WM_SETTEXT 只认证为同会话无焦点域。
    execution_realm: ExecutionRealm::SameSessionNoFocus,
    methods: &["win32-message"],
}];
const NOTEPAD_OPERATIONS: &[OperationDescriptor] = &[OperationDescriptor {
    id: "notepad.open-and-write-text",
    operation: "open-and-write-text",
    summary: "原子创建 UTF-8 文本文件，再由系统 Notepad 打开；不模拟键盘输入。",
    target_fields: &[],
    argument_fields: NOTEPAD_ARGS,
    mutates: true,
    requires_confirmation: true,
    requires_session_id: false,
    background_policy: "guaranteed",
    // 新文件与无激活 Notepad 启动属于主机后台域。
    execution_realm: ExecutionRealm::HostBackground,
    methods: &["file-automation", "command-line"],
}];
const DESKTOP_OPERATIONS: &[OperationDescriptor] = &[
    OperationDescriptor {
        id: "desktop.screenshot-interactive",
        operation: "screenshot-interactive",
        summary: "通过 Wayland XDG Desktop Portal 显示系统选择器并截取用户选定的屏幕源，不提供 X11 回退。",
        target_fields: HWND_TARGET,
        argument_fields: PORTAL_SCREENSHOT_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "foreground-consent-required",
        execution_realm: ExecutionRealm::HostForeground,
        methods: &["xdg-desktop-portal-screenshot"],
    },
    OperationDescriptor {
        id: "desktop.screenshot",
        operation: "screenshot",
        summary: "通过 Windows Graphics Capture 截取精确窗口，不激活窗口或发送输入。",
        target_fields: HWND_TARGET,
        argument_fields: DESKTOP_SCREENSHOT_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "best-effort",
        // 正式截图路径固定使用 Rust 隔离 worker。
        execution_realm: ExecutionRealm::IsolatedWorker,
        methods: &["windows-graphics-capture"],
    },
    OperationDescriptor {
        id: "desktop.record",
        operation: "record",
        summary: "低帧率录制精确窗口为 H.264 MP4，并生成供 AI 优先分析的变化关键帧故事板。",
        target_fields: HWND_TARGET,
        argument_fields: DESKTOP_RECORD_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "best-effort",
        // 正式录制路径固定使用 Rust 隔离 worker；runtime 缺失时结构化失败。
        execution_realm: ExecutionRealm::IsolatedWorker,
        methods: &["windows-graphics-capture", "media-foundation-h264"],
    },
    OperationDescriptor {
        id: "desktop.type-text",
        operation: "type-text",
        summary: "先向唯一标准 Edit 发送后台 WM_SETTEXT；无安全后台路径时请求前台授权。",
        target_fields: HWND_TARGET,
        argument_fields: SET_TEXT_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "prefer-background-then-consent",
        // 已认证后台分支只属于同会话无焦点域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        methods: &["win32-message", "foreground-input"],
    },
    OperationDescriptor {
        id: "desktop.press-key",
        operation: "press-key",
        summary: "恢复并激活精确窗口后发送按键；必须先取得前台授权。",
        target_fields: HWND_TARGET,
        argument_fields: KEY_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "foreground-consent",
        // 原生按键明确属于主机前台域。
        execution_realm: ExecutionRealm::HostForeground,
        methods: &["foreground-input"],
    },
    OperationDescriptor {
        id: "desktop.click",
        operation: "click",
        summary: "恢复并激活精确窗口后点击屏幕坐标；必须先取得前台授权。",
        target_fields: HWND_TARGET,
        argument_fields: CLICK_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "foreground-consent",
        // 原生点击明确属于主机前台域。
        execution_realm: ExecutionRealm::HostForeground,
        methods: &["foreground-input"],
    },
    OperationDescriptor {
        id: "desktop.launch",
        operation: "launch",
        summary: "直接启动指定可执行文件，不经 shell，不调用焦点 API。",
        target_fields: &[],
        argument_fields: LAUNCH_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: false,
        background_policy: "best-effort",
        // 直接启动可见应用属于主机前台影响域。
        execution_realm: ExecutionRealm::HostForeground,
        methods: &["command-line"],
    },
];

#[cfg(target_os = "linux")]
const PROCESS_OPERATIONS: &[OperationDescriptor] = &[
    OperationDescriptor {
        id: "process.terminate.graceful@2",
        operation: "terminate-graceful",
        summary: "向同 UID 非 root 的精确 Linux 进程代际发送一次温和终止请求并等待 pidfd 退出。",
        target_fields: PROCESS_TARGET,
        argument_fields: PROCESS_TERMINATION_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "guaranteed",
        execution_realm: ExecutionRealm::HostBackground,
        methods: &["exact-process-self-termination-request"],
    },
    OperationDescriptor {
        id: "process.terminate.force@2",
        operation: "terminate-force",
        summary: "显式强制终止同 UID 非 root、procfs owner epoch 与 pidfd 均已绑定的精确 Linux 进程代际。",
        target_fields: PROCESS_TARGET,
        argument_fields: PROCESS_TERMINATION_ARGS,
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "guaranteed",
        execution_realm: ExecutionRealm::HostBackground,
        methods: &["exact-process-kernel-forced-termination"],
    },
];
#[cfg(not(target_os = "linux"))]
const PROCESS_OPERATIONS: &[OperationDescriptor] = &[];

const READ_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect];
#[cfg(target_os = "linux")]
const PROCESS_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect, Verb::Run];
#[cfg(not(target_os = "linux"))]
const PROCESS_VERBS: &[Verb] = READ_VERBS;
const BROWSER_VERBS: &[Verb] = &[Verb::Status, Verb::Run];
const WIN32_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect, Verb::Run];
const NOTEPAD_VERBS: &[Verb] = &[Verb::Status, Verb::Run];
const DESKTOP_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect, Verb::Run];
const MEDIA_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect, Verb::Run];

const CATALOG: &[AppDescriptor] = &[
    AppDescriptor {
        id: "app",
        summary: "Unified opaque-session and versioned-capability facade for application control.",
        public_verbs: APP_VERBS,
        operations: APP_OPERATIONS,
        blocked: &[
            "provider-specific public commands",
            "native handles as public targets",
            "arbitrary scripts, COM members, menu ids, or shell commands",
            "silent fallback from structured APIs to foreground input",
        ],
        background_policy: "background-preferred",
        // provider session 聚合与普通 inspect 属于主机无头读取。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "uia",
        summary: "只读 Windows UI Automation 窗口发现和控件树检查。",
        public_verbs: READ_VERBS,
        operations: &[],
        blocked: &[
            "SetFocus",
            "Invoke",
            "Value.SetValue",
            "Toggle",
            "Select",
            "Scroll",
        ],
        background_policy: "guaranteed",
        // status 与 sessions 为主机无头；inspect 由 Policy 特化为隔离 worker。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "window",
        summary: "只读 Win32 顶层窗口发现。",
        public_verbs: READ_VERBS,
        operations: &[],
        blocked: &["focus", "show", "close", "move", "resize"],
        background_policy: "guaranteed",
        // 窗口目录与元数据读取属于主机无头域。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "process",
        summary: "精确进程代际发现、读取与受保护生命周期控制。",
        // 允许 canonical s2:p 的精确读取和显式确认 mutation。
        public_verbs: PROCESS_VERBS,
        operations: PROCESS_OPERATIONS,
        blocked: &[
            "arbitrary native process ids",
            "arbitrary signals",
            "automatic force fallback",
        ],
        background_policy: "guaranteed",
        // ToolHelp 目录与元数据读取属于主机无头域。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "browser",
        summary: "隔离的一次性 headless Chromium 检查与截图。",
        public_verbs: BROWSER_VERBS,
        operations: BROWSER_OPERATIONS,
        blocked: &[
            "用户 profile",
            "用户标签页",
            "扩展",
            "保存的凭据",
            "可见浏览器窗口",
        ],
        background_policy: "guaranteed",
        // 环境状态读取不启动浏览器，属于主机无头域。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "win32-control",
        summary: "经过白名单认证的 Win32 控件操作。",
        public_verbs: WIN32_VERBS,
        operations: WIN32_OPERATIONS,
        blocked: &["任意消息号", "指针参数", "WM_USER 及自定义消息", "窗口激活"],
        background_policy: "guaranteed",
        // 控件目录与元数据读取属于主机无头域。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "notepad",
        summary: "可见 Notepad 的后台文档创建与打开。",
        public_verbs: NOTEPAD_VERBS,
        operations: NOTEPAD_OPERATIONS,
        blocked: &[
            "键鼠输入",
            "剪贴板",
            "窗口激活",
            "保存已有文档",
            "关闭已有文档",
        ],
        background_policy: "guaranteed",
        // runtime 状态读取不启动 Notepad，属于主机无头域。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "media-session",
        summary: "Windows GSMTC 系统媒体会话的后台播放控制。",
        public_verbs: MEDIA_VERBS,
        operations: MEDIA_OPERATIONS,
        blocked: &["未注册媒体会话的应用", "窗口激活", "键鼠输入"],
        background_policy: "guaranteed",
        // 当前只读媒体枚举不产生主机可见影响。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
    AppDescriptor {
        id: "desktop",
        summary: "任意 Windows 窗口的后台优先通用控制，前台输入必须二次授权。",
        public_verbs: DESKTOP_VERBS,
        operations: DESKTOP_OPERATIONS,
        blocked: &[
            "未确认的前台激活",
            "未确认的键鼠输入",
            "Shell 命令拼接",
            "模糊窗口选择",
        ],
        background_policy: "background-preferred",
        // 桌面 status、sessions 与 inspect 只读取窗口目录。
        read_execution_realm: ExecutionRealm::HostHeadless,
    },
];

pub fn apps() -> &'static [AppDescriptor] {
    CATALOG
}

pub fn app(id: &str) -> Option<&'static AppDescriptor> {
    CATALOG.iter().find(|entry| entry.id == id)
}

pub fn operation(app_id: &str, operation_id: &str) -> Option<&'static OperationDescriptor> {
    app(app_id)?
        .operations
        .iter()
        .find(|entry| entry.operation == operation_id)
}
