//! capability assessment 的静态规则表。

// 导入 Rust 主注册表与 opaque 类别。
use crate::{
    // 导入 capability ID。
    capabilities,
    // 导入 opaque 目标类别。
    components::opaque_id::OpaqueTargetKind,
    // 导入跨目录、assessment 与运行时共享的执行域。
    domain::ExecutionRealm,
};

// 导入父 Module 的规则类型与构造器。
use super::{AssessmentTargetKind, CapabilityRule, rule};

// 构造 Browser Session Module 自有目标的静态规则常量。
#[allow(clippy::too_many_arguments)]
const fn browser_session_rule(
    // 接收版本化 capability ID。
    id: &'static str,
    // 接收 execution realm。
    execution_realm: ExecutionRealm,
    // 接收 Rust 承接状态。
    rust_available: bool,
    // 接收逐操作确认要求。
    requires_confirmation: bool,
    // 接收前台同意要求。
    requires_foreground_consent: bool,
    // 接收只读分类。
    read_only: bool,
    // 接收约束摘要。
    scope: &'static str,
) -> CapabilityRule {
    // 返回不扩大全局 opaque 类别的 Browser Session 规则。
    CapabilityRule {
        // 保存 capability ID。
        id,
        // 使用 assessment 私有 Browser Session 类别。
        target_kind: AssessmentTargetKind::BrowserSession,
        // 保存执行域。
        execution_realm,
        // 保存迁移状态。
        rust_available,
        // 保存确认策略。
        requires_confirmation,
        // 保存前台策略。
        requires_foreground_consent,
        // 保存只读分类。
        read_only,
        // 保存范围摘要。
        scope,
    }
}

// 集中声明 Rust registry 与 C++ 待迁移 capability 的 assessment 语义。
pub(super) const RULES: &[CapabilityRule] = &[
    // Rust 主实现的应用关系图发现。
    rule(
        capabilities::APPLICATION_DISCOVER,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "installed-and-running-application-inventory",
    ),
    // Linux XDG v2 只由 Linux assessment Module 发布；Windows 静态表保持不可用。
    rule(
        capabilities::APPLICATION_DISCOVER_V2,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        false,
        false,
        false,
        true,
        "linux-xdg-desktop-entry-and-procfs-independent-inventories",
    ),
    // Linux v3 启动状态清单只由 Linux Module 发布；Windows 保持不可用。
    rule(
        capabilities::APPLICATION_DISCOVER_V3,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        false,
        false,
        false,
        true,
        "linux-xdg-procfs-inventory-with-toolkit-fixture-launch-status",
    ),
    // Rust 主实现的进程发现。
    rule(
        capabilities::PROCESS_DISCOVER,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "running-process-inventory",
    ),
    // Rust 主实现的窗口发现。
    rule(
        capabilities::WINDOW_DISCOVER,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "visible-titled-top-level-windows",
    ),
    // Rust 主实现的认证独立交互会话发现。
    rule(
        // 使用稳定版本化独立会话发现 ID。
        capabilities::INTERACTIVE_SESSION_DISCOVER,
        // 发现入口由当前 host session 发起。
        OpaqueTargetKind::Host,
        // WTS 枚举与认证握手属于主机无头读取。
        ExecutionRealm::HostHeadless,
        // Rust System 与 Interactive Isolation Module 已承接。
        true,
        // 只读发现不要求逐操作确认。
        false,
        // 只读发现不要求前景许可。
        false,
        // 发现不执行目标 mutation。
        true,
        // 固定只发布双向认证 endpoint。
        "certified-independent-interactive-session-endpoints",
    ),
    // Rust 主实现的进程元数据读取。
    rule(
        capabilities::PROCESS_METADATA_READ,
        OpaqueTargetKind::Process,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "public-name-state-relations-and-relative-integrity",
    ),
    // Rust 主实现的精确进程优雅终止。
    rule(
        // 使用稳定版本化优雅终止 ID。
        capabilities::PROCESS_TERMINATE_GRACEFUL,
        // 只接受精确 opaque 进程目标。
        OpaqueTargetKind::Process,
        // 固定消息投递与等待在主机后台域执行。
        ExecutionRealm::HostBackground,
        // Rust 生产 Module 与 Adapter 已承接。
        true,
        // 进程生命周期 mutation 必须逐操作确认。
        true,
        // 优雅终止不允许激活目标或使用前台输入。
        false,
        // 终止会改变外部进程状态。
        false,
        // 冻结精确身份、固定优雅机制与禁止强制回退。
        "confirmed-exact-process-graceful-termination-without-force-fallback",
    ),
    // Linux pidfd v2 只由 Linux assessment Module 发布；Windows 静态表保持不可用。
    rule(
        capabilities::PROCESS_TERMINATE_GRACEFUL_V2,
        OpaqueTargetKind::Process,
        ExecutionRealm::HostBackground,
        false,
        true,
        false,
        false,
        "linux-same-non-root-uid-exact-process-pidfd-graceful-termination",
    ),
    // Rust 主实现的精确进程强制终止。
    rule(
        // 使用稳定版本化强制终止 ID。
        capabilities::PROCESS_TERMINATE_FORCE,
        // 只接受精确 opaque 进程目标。
        OpaqueTargetKind::Process,
        // 固定内核终止与等待在主机后台域执行。
        ExecutionRealm::HostBackground,
        // Rust 生产 Module 与 Adapter 已承接。
        true,
        // 强制终止的独立高风险动作必须逐操作确认。
        true,
        // 强制终止不允许激活目标或使用前台输入。
        false,
        // 终止会改变外部进程状态。
        false,
        // 冻结精确身份、保护目标与固定强制机制。
        "confirmed-exact-process-force-termination-with-protected-target-gates",
    ),
    // Rust 主实现的窗口元数据读取。
    rule(
        capabilities::WINDOW_METADATA_READ,
        OpaqueTargetKind::Window,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "public-window-metadata",
    ),
    // Rust 主实现的隔离可访问性树读取。
    rule(
        capabilities::ACCESSIBILITY_TREE_READ,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        false,
        false,
        true,
        "bounded-tree-without-value-text-or-bounds",
    ),
    // Rust 主实现的 provider-neutral 元素定位。
    rule(
        capabilities::UI_ELEMENT_LOCATE,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        false,
        false,
        true,
        "bounded-semantic-location-with-physical-hit-region",
    ),
    // Rust 主实现的 provider-neutral 语义元素动作。
    rule(
        capabilities::UI_ELEMENT_ACTION,
        OpaqueTargetKind::Window,
        ExecutionRealm::SameSessionNoFocus,
        true,
        true,
        false,
        false,
        "confirmed-provider-neutral-semantic-element-action",
    ),
    // Rust 主实现的有界语义元素等待。
    rule(
        capabilities::UI_ELEMENT_WAIT,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        false,
        false,
        true,
        "bounded-semantic-element-wait-with-stability",
    ),
    // Rust 主实现的精确窗口关闭等待。
    rule(
        capabilities::WINDOW_CLOSED_WAIT,
        OpaqueTargetKind::Window,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "bounded-exact-window-absence-wait-with-stability",
    ),
    // Rust 主实现的应用启动请求。
    rule(
        capabilities::APPLICATION_OPEN,
        OpaqueTargetKind::Application,
        ExecutionRealm::HostForeground,
        true,
        true,
        false,
        false,
        "exact-installed-application-shell-launch",
    ),
    // Linux Desktop Entry 工具自有夹具路线不复用 Windows v1 Shell 语义。
    rule(
        capabilities::APPLICATION_OPEN_V2,
        OpaqueTargetKind::Application,
        ExecutionRealm::HostForeground,
        false,
        true,
        true,
        false,
        "linux-toolkit-self-executable-fixture-fixed-argv-no-shell",
    ),
    // Rust 主实现的文本文件创建。
    rule(
        capabilities::TEXT_DOCUMENT_CREATE,
        OpaqueTargetKind::Application,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "atomic-new-utf8-artifact",
    ),
    // Rust 主实现的图像画布创建。
    rule(
        capabilities::IMAGE_CANVAS_CREATE,
        OpaqueTargetKind::Application,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "provider-owned-image-canvas",
    ),
    // Rust 主实现的图层变更。
    rule(
        capabilities::IMAGE_LAYERS_APPLY,
        OpaqueTargetKind::Document,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "provider-owned-layer-patch",
    ),
    // Rust 主实现的制品保存。
    rule(
        capabilities::ARTIFACT_SAVE,
        OpaqueTargetKind::Document,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "provider-owned-artifact-save",
    ),
    // Rust 主实现的图像导出。
    rule(
        capabilities::IMAGE_EXPORT,
        OpaqueTargetKind::Document,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "provider-owned-image-export",
    ),
    // Rust 主实现的文档关闭。
    rule(
        capabilities::DOCUMENT_CLOSE,
        OpaqueTargetKind::Document,
        ExecutionRealm::HostBackground,
        true,
        true,
        false,
        false,
        "exact-document-close",
    ),
    // Rust 主实现的窗口关闭。
    rule(
        capabilities::WINDOW_CLOSE,
        OpaqueTargetKind::Window,
        ExecutionRealm::SameSessionNoFocus,
        true,
        true,
        false,
        false,
        "exact-background-window-close",
    ),
    // Rust 主实现的通用窗口状态与几何生命周期控制。
    rule(
        // 使用稳定版本化 capability ID。
        capabilities::WINDOW_LIFECYCLE,
        // 只接受精确 opaque 窗口目标。
        OpaqueTargetKind::Window,
        // 可见状态与几何 mutation 属于主机前景域。
        ExecutionRealm::HostForeground,
        // Rust 生产 Module 与 Adapter 已承接。
        true,
        // 每次 mutation 均要求逐操作确认。
        true,
        // 每次 mutation 均要求预先前景影响同意。
        true,
        // 生命周期动作会改变外部窗口状态。
        false,
        // 冻结精确状态、几何与最终读回约束。
        "confirmed-exact-window-state-and-geometry",
    ),
    // Rust 主实现的窗口截图。
    rule(
        capabilities::WINDOW_SCREENSHOT,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        true,
        false,
        false,
        "confirmed-rust-worker-atomic-png",
    ),
    // Rust 主实现的窗口录制。
    rule(
        capabilities::WINDOW_RECORD,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        true,
        false,
        false,
        "confirmed-rust-worker-recording-transaction",
    ),
    // Rust 主实现的标准 Edit 精确控件文本输入。
    rule(
        capabilities::UI_TEXT_INPUT,
        OpaqueTargetKind::Control,
        ExecutionRealm::SameSessionNoFocus,
        true,
        true,
        false,
        false,
        "exact-standard-edit-text-input",
    ),
    // Rust 主实现的前台键盘输入。
    rule(
        capabilities::UI_INPUT_KEY,
        OpaqueTargetKind::Window,
        ExecutionRealm::HostForeground,
        true,
        true,
        true,
        false,
        "explicit-foreground-key-input",
    ),
    // Rust 主实现的前台指针输入。
    rule(
        capabilities::UI_INPUT_POINTER,
        OpaqueTargetKind::Window,
        ExecutionRealm::HostForeground,
        true,
        true,
        true,
        false,
        "explicit-foreground-pointer-input",
    ),
    // Rust 主实现的统一应用 session 聚合发现。
    rule(
        capabilities::APPLICATION_SESSION_DISCOVER,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "bounded-provider-neutral-app-sessions",
    ),
    // Linux 版本二聚合只由 Linux assessment Module 发布；Windows 保持不可用。
    rule(
        capabilities::APPLICATION_SESSION_DISCOVER_V2,
        OpaqueTargetKind::Host,
        ExecutionRealm::HostHeadless,
        false,
        false,
        false,
        true,
        "linux-xdg-and-procfs-read-only-unrelated-sessions",
    ),
    // Linux 版本三聚合只由同会话 UIX Agent assessment 发布；其他平台保持不可用。
    rule(
        capabilities::APPLICATION_SESSION_DISCOVER_V3,
        OpaqueTargetKind::Host,
        ExecutionRealm::SameSessionNoFocus,
        false,
        false,
        false,
        true,
        "linux-xdg-procfs-and-opt-in-uix-window-sessions",
    ),
    // Linux 版本四聚合在版本三关系模型上原子加入认证启动状态。
    rule(
        capabilities::APPLICATION_SESSION_DISCOVER_V4,
        OpaqueTargetKind::Host,
        ExecutionRealm::SameSessionNoFocus,
        false,
        false,
        false,
        true,
        "linux-launch-aware-xdg-procfs-and-opt-in-uix-window-sessions",
    ),
    // Rust 主实现的固定 Browser Session 打开命令。
    rule(
        // 使用稳定版本化 Browser Session 打开 ID。
        capabilities::BROWSER_SESSION_OPEN,
        // 打开只能绑定当前 canonical host session。
        OpaqueTargetKind::Host,
        // 固定 Broker 与 worker 路线属于隔离执行域。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 已承接该能力。
        true,
        // 生命周期 mutation 必须逐操作确认。
        true,
        // 固定后台路线不要求前台影响同意。
        false,
        // 打开会改变外部会话生命周期。
        false,
        // 只允许固定 Broker 创建全新隔离会话。
        "fixed-isolated-browser-session-open",
    ),
    // Rust 主实现的固定 Browser Session 关闭命令。
    browser_session_rule(
        // 使用稳定版本化 Browser Session 关闭 ID。
        capabilities::BROWSER_SESSION_CLOSE,
        // 固定 Broker 与 worker 回收属于隔离执行域。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 已承接该能力。
        true,
        // 生命周期 mutation 必须逐操作确认。
        true,
        // 严格零干扰关闭不要求前台影响同意。
        false,
        // 关闭会改变外部会话生命周期。
        false,
        // 只允许关闭当前 Broker 代际的 live 精确会话。
        "canonical-live-browser-session-close",
    ),
    // Rust 主实现的固定 Browser Session 页面导航命令。
    browser_session_rule(
        // 使用稳定版本化页面导航 ID。
        capabilities::BROWSER_PAGE_NAVIGATE,
        // 页面命令固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 导航会改变页面代际，必须逐操作确认。
        true,
        // 严格零干扰路线不要求前台影响同意。
        false,
        // 导航会改变外部页面状态。
        false,
        // 只允许导航当前 Broker 代际的 live 精确会话。
        "confirmed-canonical-live-browser-page-navigation",
    ),
    // Rust 主实现的固定 Browser Session 页面等待 Query。
    browser_session_rule(
        // 使用稳定版本化页面等待 ID。
        capabilities::BROWSER_PAGE_WAIT,
        // 页面 Query 固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 只读等待不要求逐操作确认。
        false,
        // 严格零干扰路线不要求前台影响同意。
        false,
        // 等待只观察当前页面状态。
        true,
        // 只等待当前 live 页面上的封闭 provider-neutral 条件。
        "bounded-canonical-live-browser-page-wait",
    ),
    // Rust 主实现的固定 Browser Session 页面查询 Query。
    browser_session_rule(
        // 使用稳定版本化页面查询 ID。
        capabilities::BROWSER_PAGE_QUERY,
        // 页面 Query 固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 只读查询不要求逐操作确认。
        false,
        // 严格零干扰路线不要求前台影响同意。
        false,
        // 查询只读取有界语义结果。
        true,
        // 只返回当前 live 页面上的有界 provider-neutral 语义命中。
        "bounded-canonical-live-browser-page-query",
    ),
    // Rust 主实现的固定 Browser Session 元素点击命令。
    browser_session_rule(
        // 使用稳定版本化元素点击 ID。
        capabilities::BROWSER_ELEMENT_CLICK,
        // 元素命令固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 点击会改变外部页面状态，必须逐操作确认。
        true,
        // 严格零干扰路线不要求前台影响同意。
        false,
        // 点击属于 mutation Command。
        false,
        // 只允许点击当前页面代际签发的 live 元素。
        "confirmed-canonical-live-browser-element-click",
    ),
    // Rust 主实现的固定 Browser Session 元素输入命令。
    browser_session_rule(
        // 使用稳定版本化元素输入 ID。
        capabilities::BROWSER_ELEMENT_TYPE,
        // 元素命令固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 输入会改变外部页面状态，必须逐操作确认。
        true,
        // 严格零干扰路线不要求前台影响同意。
        false,
        // 输入属于 mutation Command。
        false,
        // 只允许向当前页面代际签发的 live 元素输入有界文本。
        "confirmed-canonical-live-browser-element-type",
    ),
    // Rust 主实现的固定 Browser Session 页面截图 Query。
    browser_session_rule(
        // 使用稳定版本化页面截图 ID。
        capabilities::BROWSER_PAGE_SCREENSHOT,
        // 页面截图固定经 Broker 在隔离 worker 域执行。
        ExecutionRealm::IsolatedWorker,
        // Rust Browser Session Module 与 Broker 纵切已承接该能力。
        true,
        // 只读截图不要求逐操作确认。
        false,
        // 后台截图不要求前台影响同意。
        false,
        // 截图只读取有界 PNG。
        true,
        // 只返回当前 live 页面上的有界 PNG 投影。
        "bounded-canonical-live-browser-page-screenshot",
    ),
    // Rust 主实现的隔离浏览器截图。
    rule(
        capabilities::BROWSER_SCREENSHOT,
        OpaqueTargetKind::Host,
        ExecutionRealm::IsolatedWorker,
        true,
        true,
        false,
        false,
        "fixed-isolated-headless-browser-screenshot",
    ),
    // C++ 待迁移的媒体 session 发现。
    rule(
        "media.session.discover@1",
        OpaqueTargetKind::Host,
        ExecutionRealm::None,
        false,
        false,
        false,
        true,
        "pending-rust-migration",
    ),
    // C++ 待迁移的媒体状态读取。
    rule(
        "media.playback.state.read@1",
        OpaqueTargetKind::Media,
        ExecutionRealm::None,
        false,
        false,
        false,
        true,
        "pending-rust-migration",
    ),
    // C++ 待迁移的媒体控制。
    rule(
        "media.playback.control@1",
        OpaqueTargetKind::Media,
        ExecutionRealm::None,
        false,
        true,
        false,
        false,
        "pending-rust-migration",
    ),
    // Rust 主实现的零帧窗口捕获预检。
    rule(
        capabilities::WINDOW_CAPTURE_PREFLIGHT,
        OpaqueTargetKind::Window,
        ExecutionRealm::HostHeadless,
        true,
        false,
        false,
        true,
        "exact-window-capture-metadata-without-frame-or-session",
    ),
    // Rust 主实现的确认式窗口帧探针。
    rule(
        capabilities::WINDOW_CAPTURE_FRAME_PROBE,
        OpaqueTargetKind::Window,
        ExecutionRealm::IsolatedWorker,
        true,
        true,
        false,
        true,
        "confirmed-frame-metadata-without-surface-or-files",
    ),
];
