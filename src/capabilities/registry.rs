//! 版本化 capability 的集中定义表。

use super::{CapabilityAction, CapabilityDefinition, CapabilitySurface, ExecutionRealm, ids::*};

// 构造固定 Browser Session Broker 承接的 App capability 定义。
const fn browser_session_definition(
    // 接收稳定版本化 ID。
    id: &'static str,
    // 接收 generic action 分类。
    action: CapabilityAction,
    // 接收公开后台执行说明。
    execution: &'static str,
    // 接收冻结输入 schema ID。
    input_schema: &'static str,
) -> CapabilityDefinition {
    // 返回不允许前台回退的隔离 worker 定义。
    CapabilityDefinition {
        // Browser Session 操作只属于统一 App surface。
        surface: CapabilitySurface::App,
        // 保存稳定 capability ID。
        id,
        // 保存 generic action。
        action,
        // 保存公开执行说明。
        execution,
        // 固定 Broker 与 Module 位于隔离 worker 域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 固定路线不要求前台影响同意。
        requires_foreground_consent: false,
        // 不允许以前台同意改变固定路线。
        requires_upfront_foreground_consent: false,
        // 保存严格输入 schema。
        input_schema,
    }
}

// 集中登记全部 Rust provider 当前公开的版本化 capability。
pub(crate) const ALL: &[CapabilityDefinition] = &[
    // v3 独立属于 Linux App surface；不改变 Media v2 候选的所有权与可用性。
    #[cfg(target_os = "linux")]
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: MEDIA_SESSION_DISCOVER_V3,
        action: CapabilityAction::Discover,
        execution: "isolated-worker",
        execution_realm: ExecutionRealm::IsolatedWorker,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/session-discover/v3",
    },
    #[cfg(target_os = "linux")]
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: MEDIA_PLAYBACK_STATE_READ_V3,
        action: CapabilityAction::Read,
        execution: "isolated-worker",
        execution_realm: ExecutionRealm::IsolatedWorker,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/playback-state/v3",
    },
    #[cfg(target_os = "linux")]
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: MEDIA_PLAYBACK_CONTROL_V3,
        action: CapabilityAction::Apply,
        execution: "isolated-worker",
        execution_realm: ExecutionRealm::IsolatedWorker,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/playback-control/v3",
    },
    // 登记应用打开 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: APPLICATION_OPEN,
        action: CapabilityAction::Create,
        execution: "background-preferred",
        // 应用启动会产生主机可见前台影响。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://application/open/v1",
    },
    // 登记 Linux 指向当前工具自身映像的 Desktop Entry 夹具启动 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: APPLICATION_OPEN_V2,
        action: CapabilityAction::Create,
        execution: "foreground-consent-required",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://application/open/v2",
    },
    // 登记画布创建 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: IMAGE_CANVAS_CREATE,
        action: CapabilityAction::Create,
        execution: "background",
        // provider 自有画布在主机后台域创建。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://image/canvas-create/v1",
    },
    // 登记文本文档创建 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: TEXT_DOCUMENT_CREATE,
        action: CapabilityAction::Create,
        execution: "background-preferred",
        // 原子文件与无激活启动属于主机后台域。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://text/document-create/v1",
    },
    // 登记图层变更 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: IMAGE_LAYERS_APPLY,
        action: CapabilityAction::Apply,
        execution: "background",
        // provider 自有图层变更属于主机后台域。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://image/layers-apply/v1",
    },
    // 登记标准 Edit 固定消息文本输入 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_TEXT_INPUT,
        action: CapabilityAction::Apply,
        execution: "background",
        // 标准 Edit 消息只认证为同会话无焦点域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/text-input/v1",
    },
    // 登记键盘输入 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_KEY,
        action: CapabilityAction::Apply,
        execution: "foreground",
        // 原生键盘输入属于主机前台域。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://ui/key-input/v1",
    },
    // 登记 UIX 协作式应用内完整按键 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布精确协作式窗口 mutation。
        surface: CapabilitySurface::App,
        // 使用独立版本避免改写 Windows 主机前台输入契约。
        id: UI_INPUT_KEY_V2,
        // 应用内按键统一使用 apply 动作。
        action: CapabilityAction::Apply,
        // UIX 事件进入应用 UI 树但不取得主机焦点。
        execution: "background",
        // 认证为同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // mutation 仍需要逐操作确认，但不需要主机前景同意。
        requires_foreground_consent: false,
        // 该路线从不请求主机前景授权。
        requires_upfront_foreground_consent: false,
        // 指向严格版本二输入 schema。
        input_schema: "schema://ui/key-input/v2",
    },
    // 登记 UIX 同一认证连接内的有界成对按键序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_KEY_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/key-sequence/v1",
    },
    // 登记 UIX 同一认证连接内的完整按键序列与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_KEY_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/key-sequence-transition/v1",
    },
    // 登记 UIX 同一认证连接内的完整按键与语义后置条件同步 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_KEY_TRANSITION,
        action: CapabilityAction::Apply,
        // 按键只进入应用内部 UI 树，不请求主机前景。
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/key-transition/v1",
    },
    // 登记 UIX 同一认证连接内的有界键盘与指针混合序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/input-sequence/v1",
    },
    // 登记 UIX 同一认证连接内的完整跨模态序列与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/input-sequence-transition/v1",
    },
    // 登记 Portal/EIS 授权桌面会话内的键盘输入候选。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_KEY_V3,
        action: CapabilityAction::Apply,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://ui/key-input/v3",
    },
    // 登记指针输入 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER,
        action: CapabilityAction::Apply,
        execution: "foreground",
        // 原生指针输入属于主机前台域。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://ui/pointer-input/v1",
    },
    // 登记 UIX 协作式应用内指针 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布精确协作式窗口 mutation。
        surface: CapabilitySurface::App,
        // 使用独立版本避免改写 Windows 主机前台指针契约。
        id: UI_INPUT_POINTER_V2,
        // 应用内 move/click 统一使用 apply 动作。
        action: CapabilityAction::Apply,
        // UIX 事件进入应用 UI 树但不移动桌面指针。
        execution: "background",
        // 认证为同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // mutation 仍需要逐操作确认但不需要主机前景同意。
        requires_foreground_consent: false,
        // 该路线从不请求主机前景授权。
        requires_upfront_foreground_consent: false,
        // 指向严格版本二输入 schema。
        input_schema: "schema://ui/pointer-input/v2",
    },
    // 登记 UIX 同一认证连接内的有界普通左键点击序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_CLICK_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-click-sequence/v1",
    },
    // 登记 UIX 完整普通左键点击序列与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-click-sequence-transition/v1",
    },
    // 登记 UIX 同一认证连接内的普通左键点击与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_CLICK_TRANSITION,
        action: CapabilityAction::Apply,
        // click 只进入应用内部 UI 树，不请求主机前景或桌面指针。
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-click-transition/v1",
    },
    // 登记 UIX 同一认证连接内的单次指针移动与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_MOVE_TRANSITION,
        // pointer_move 只进入应用内部 UI 树，不请求主机前景或桌面指针。
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-move-transition/v1",
    },
    // 登记 UIX 同一认证连接内的有界纯移动点序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_MOVE_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-move-sequence/v1",
    },
    // 登记 UIX 完整移动点序列与语义后置条件同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-move-sequence-transition/v1",
    },
    // 登记 UIX 同一认证连接内的有界悬停—普通左键点击混合序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-sequence/v1",
    },
    // 登记完整悬停—点击序列与语义后置条件的同连接同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-sequence-transition/v1",
    },
    // 登记 UIX 同一认证连接内的原子左键拖拽。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_DRAG,
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-drag/v1",
    },
    // 登记 UIX 请求内配平拖拽与语义后置条件的同连接同步。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_DRAG_TRANSITION,
        // 固定左键拖拽只进入应用内部 UI 树，不请求主机前景或桌面指针。
        action: CapabilityAction::Apply,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/pointer-drag-transition/v1",
    },
    // 登记 Portal/EIS 授权桌面会话内的相对指针输入候选。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_INPUT_POINTER_V3,
        action: CapabilityAction::Apply,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://ui/pointer-input/v3",
    },
    // 登记制品保存 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: ARTIFACT_SAVE,
        action: CapabilityAction::Save,
        execution: "background",
        // provider 自有保存属于主机后台域。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://artifact/save/v1",
    },
    // 登记图像导出 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: IMAGE_EXPORT,
        action: CapabilityAction::Export,
        execution: "background",
        // provider 自有导出属于主机后台域。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://image/export/v1",
    },
    // 登记文档关闭 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DOCUMENT_CLOSE,
        action: CapabilityAction::Close,
        execution: "background",
        // provider 自有文档关闭属于主机后台域。
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://document/close/v1",
    },
    // 登记窗口关闭 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_CLOSE,
        action: CapabilityAction::Close,
        execution: "background",
        // 精确 WM_CLOSE 只认证为同会话无焦点域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/close/v1",
    },
    // 登记 UIX Agent 精确窗口关闭请求 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向当前协作式 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本二 ID，避免扩写 Windows WM_CLOSE 版本一语义。
        id: WINDOW_CLOSE_V2,
        // 复用统一 close generic verb。
        action: CapabilityAction::Close,
        // 关闭请求不激活窗口，公开分类保持 background。
        execution: "background",
        // 只进入认证应用自己的 UI turn，不取得桌面焦点。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // UIX close 不请求前景激活，因此无需前景影响同意。
        requires_foreground_consent: false,
        // 确认由统一 mutation Policy 独立强制。
        requires_upfront_foreground_consent: false,
        // 指向严格版本二输入 schema。
        input_schema: "schema://window/close/v2",
    },
    // 登记 UIX Agent 精确窗口关闭并等待终态 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向当前协作式 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本化 ID，避免扩写单独关闭请求语义。
        id: WINDOW_CLOSE_TRANSITION,
        // 关闭并观察终态统一使用 close generic verb。
        action: CapabilityAction::Close,
        // 关闭请求不激活窗口，公开分类保持 background。
        execution: "background",
        // 请求与显式终态等待均留在认证应用会话，不取得桌面焦点。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // UIX close transition 不请求前景激活，因此无需前景影响同意。
        requires_foreground_consent: false,
        // 确认由统一 mutation Policy 独立强制。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一关闭 transition 输入 schema。
        input_schema: "schema://window/close-transition/v1",
    },
    // 登记精确进程优雅终止 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布进程生命周期 mutation。
        surface: CapabilitySurface::App,
        // 使用独立优雅终止 ID，禁止 input 改写风险。
        id: PROCESS_TERMINATE_GRACEFUL,
        // 复用统一 close generic verb。
        action: CapabilityAction::Close,
        // 固定窗口关闭请求在主机后台执行。
        execution: "background",
        // 不激活目标且保证宿主前景不变。
        execution_realm: ExecutionRealm::HostBackground,
        // 不使用任何前台输入或激活。
        requires_foreground_consent: false,
        // 执行前不要求前台影响同意。
        requires_upfront_foreground_consent: false,
        // 两级风险共享同一严格有界输入 schema。
        input_schema: "schema://process/termination/v1",
    },
    // 登记精确进程强制终止 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布进程生命周期 mutation。
        surface: CapabilitySurface::App,
        // 使用独立强制终止 ID，禁止优雅路径自动升级。
        id: PROCESS_TERMINATE_FORCE,
        // 复用统一 close generic verb。
        action: CapabilityAction::Close,
        // 固定内核终止在主机后台执行。
        execution: "background",
        // 不激活目标且保证宿主前景不变。
        execution_realm: ExecutionRealm::HostBackground,
        // 不使用任何前台输入或激活。
        requires_foreground_consent: false,
        // 执行前不要求前台影响同意。
        requires_upfront_foreground_consent: false,
        // 两级风险共享同一严格有界输入 schema。
        input_schema: "schema://process/termination/v1",
    },
    // 登记通用窗口状态与几何生命周期 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布精确窗口 mutation。
        surface: CapabilitySurface::App,
        // 使用稳定版本化 capability ID。
        id: WINDOW_LIFECYCLE,
        // 状态与几何变更统一使用 apply 动作。
        action: CapabilityAction::Apply,
        // descriptor 声明可见前景影响执行方式。
        execution: "foreground",
        // 可见窗口状态与位置变化属于主机前景域。
        execution_realm: ExecutionRealm::HostForeground,
        // 公开 descriptor 必须声明前景同意要求。
        requires_foreground_consent: true,
        // 执行前必须已有显式前景影响同意。
        requires_upfront_foreground_consent: true,
        // 指向严格版本一输入 schema。
        input_schema: "schema://window/lifecycle/v1",
    },
    // 登记 UIX 协作式窗口状态与 logical 客户区生命周期 capability。
    CapabilityDefinition {
        // 通过统一 app facade 发布精确协作式窗口 mutation。
        surface: CapabilitySurface::App,
        // 使用独立版本避免改写 Windows 物理外框契约。
        id: WINDOW_LIFECYCLE_V2,
        // 状态与 logical resize 统一使用 apply 动作。
        action: CapabilityAction::Apply,
        // 可见窗口状态与尺寸变化属于主机前景影响。
        execution: "foreground",
        // 使用强类型主机前景域。
        execution_realm: ExecutionRealm::HostForeground,
        // descriptor 声明需要前景影响同意。
        requires_foreground_consent: true,
        // 执行前必须已有显式前景影响同意。
        requires_upfront_foreground_consent: true,
        // 指向严格版本二输入 schema。
        input_schema: "schema://window/lifecycle/v2",
    },
    // 登记 UIX 同一认证连接内的窗口生命周期动作序列。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_LIFECYCLE_SEQUENCE,
        action: CapabilityAction::Apply,
        execution: "foreground",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://window/lifecycle-sequence/v1",
    },
    // 登记 UIX 完整窗口生命周期序列与框架状态条件等待。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_LIFECYCLE_SEQUENCE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "foreground",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://window/lifecycle-sequence-transition/v1",
    },
    // 登记 UIX 同一认证连接内的窗口生命周期动作与状态条件等待。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_LIFECYCLE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "foreground",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://window/lifecycle-transition/v1",
    },
    // 登记 UIX 协作式窗口前台激活请求 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_ACTIVATE,
        action: CapabilityAction::Apply,
        execution: "foreground",
        // 激活会请求合成器改变主机前台焦点。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://window/activate/v1",
    },
    // 登记 UIX 同一认证连接内的窗口激活与焦点观察 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_ACTIVATE_TRANSITION,
        action: CapabilityAction::Apply,
        execution: "foreground",
        // 激活请求可能改变宿主前台焦点，必须显式取得前台同意。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://window/activate-transition/v1",
    },
    // 登记 UIX 协作式窗口框架当前状态读取 capability。
    CapabilityDefinition {
        // 通过统一 app facade 对精确窗口 generation 执行只读观测。
        surface: CapabilitySurface::App,
        // 独立 ID 避免扩写冻结的窗口元数据与生命周期契约。
        id: WINDOW_STATE_READ,
        // 状态观测不改变窗口或应用。
        action: CapabilityAction::Read,
        // 认证读取发生在应用会话且不请求焦点。
        execution: "background",
        // 使用强类型同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 只读观测不需要前景影响同意。
        requires_foreground_consent: false,
        // 只读观测不要求预先前景影响同意。
        requires_upfront_foreground_consent: false,
        // 指向严格空对象输入 schema。
        input_schema: "schema://window/state-read/v1",
    },
    // 登记 UIX 协作式窗口框架当前状态条件等待 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_STATE_WAIT,
        action: CapabilityAction::Read,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/state-wait/v1",
    },
    // 登记窗口截图 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_SCREENSHOT,
        action: CapabilityAction::Screenshot,
        execution: "background",
        // 正式契约要求截图在隔离 worker 域执行。
        execution_realm: ExecutionRealm::IsolatedWorker,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/screenshot/v1",
    },
    // 登记 UIX Agent 精确窗口 generation 的应用表面截图。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_SCREENSHOT_V2,
        action: CapabilityAction::Screenshot,
        execution: "background",
        // 截图只通过认证应用连接读取自身表面，不取得桌面焦点。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/screenshot/v2",
    },
    // 登记不冒充精确窗口语义的 Wayland Portal 交互式截图 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DESKTOP_SCREENSHOT_INTERACTIVE,
        action: CapabilityAction::Screenshot,
        execution: "foreground",
        // 系统 Portal 选择器与屏幕读取均发生在主机会话前景域。
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://desktop/screenshot-interactive/v1",
    },
    // 登记由单线程长期 stdio broker 持有的 Portal 桌面会话打开 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DESKTOP_SESSION_OPEN,
        action: CapabilityAction::Create,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://desktop/session-open/v1",
    },
    // 登记同一 broker owner generation 内的显式会话关闭 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DESKTOP_SESSION_CLOSE,
        action: CapabilityAction::Close,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://desktop/session-close/v1",
    },
    // 登记同一 broker owner generation 内的一次性 PNG 屏幕读取。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: SCREEN_CAPTURE,
        action: CapabilityAction::Screenshot,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://screen/capture/v1",
    },
    // 紧凑观察与交互只经同一 Portal broker 执行，不回退其他控制器。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DESKTOP_OBSERVE,
        action: CapabilityAction::Screenshot,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostBackground,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://screen/capture/v1",
    },
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: DESKTOP_INTERACTION,
        action: CapabilityAction::Apply,
        execution: "persistent-jsonl-stdio",
        execution_realm: ExecutionRealm::HostForeground,
        requires_foreground_consent: true,
        requires_upfront_foreground_consent: true,
        input_schema: "schema://desktop/interaction/v1",
    },
    // 登记窗口录制 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: WINDOW_RECORD,
        action: CapabilityAction::Record,
        execution: "background",
        // 正式契约要求录制在隔离 worker 域执行。
        execution_realm: ExecutionRealm::IsolatedWorker,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/record/v1",
    },
    // 登记固定 Broker 的浏览器会话打开 capability。
    browser_session_definition(
        // 使用稳定打开 ID。
        BROWSER_SESSION_OPEN,
        // 打开投影到 generic create。
        CapabilityAction::Create,
        // 公开后台执行。
        "background",
        // 复用冻结生命周期 schema。
        "schema://browser/session-lifecycle/v1",
    ),
    // 登记固定 Broker 的浏览器会话关闭 capability。
    browser_session_definition(
        // 使用稳定关闭 ID。
        BROWSER_SESSION_CLOSE,
        // 关闭投影到 generic close。
        CapabilityAction::Close,
        // 公开后台执行。
        "background",
        // 复用冻结生命周期 schema。
        "schema://browser/session-lifecycle/v1",
    ),
    // 登记固定 Broker 的浏览器页面导航 capability。
    browser_session_definition(
        // 使用稳定导航 ID。
        BROWSER_PAGE_NAVIGATE,
        // 导航投影到 generic apply。
        CapabilityAction::Apply,
        // 公开后台执行。
        "background",
        // 使用冻结导航 schema。
        "schema://browser/page-navigate/v1",
    ),
    // 登记固定 Broker 的浏览器页面等待 capability。
    browser_session_definition(
        // 使用稳定等待 ID。
        BROWSER_PAGE_WAIT,
        // 等待投影到 generic read。
        CapabilityAction::Read,
        // 公开后台执行。
        "background",
        // 使用冻结等待 schema。
        "schema://browser/page-wait/v1",
    ),
    // 登记固定 Broker 的浏览器页面查询 capability。
    browser_session_definition(
        // 使用稳定查询 ID。
        BROWSER_PAGE_QUERY,
        // 查询投影到 generic read。
        CapabilityAction::Read,
        // 公开后台执行。
        "background",
        // 使用冻结查询 schema。
        "schema://browser/page-query/v1",
    ),
    // 登记固定 Broker 的浏览器元素点击 capability。
    browser_session_definition(
        // 使用稳定点击 ID。
        BROWSER_ELEMENT_CLICK,
        // 点击投影到 generic apply。
        CapabilityAction::Apply,
        // 公开后台执行。
        "background",
        // 使用冻结点击 schema。
        "schema://browser/element-click/v1",
    ),
    // 登记固定 Broker 的浏览器元素输入 capability。
    browser_session_definition(
        // 使用稳定输入 ID。
        BROWSER_ELEMENT_TYPE,
        // 输入投影到 generic apply。
        CapabilityAction::Apply,
        // 公开后台执行。
        "background",
        // 使用冻结输入 schema。
        "schema://browser/element-type/v1",
    ),
    // 登记固定 Broker 的浏览器页面截图 capability。
    browser_session_definition(
        // 使用稳定页面截图 ID。
        BROWSER_PAGE_SCREENSHOT,
        // 页面截图投影到 generic read。
        CapabilityAction::Read,
        // 公开后台优先执行。
        "background-preferred",
        // 使用冻结页面截图 schema。
        "schema://browser/page-screenshot/v1",
    ),
    // 登记隔离浏览器截图 capability。
    CapabilityDefinition {
        // 使用独立 browser surface，禁止被 generic app provider 误路由。
        surface: CapabilitySurface::Browser,
        // 使用稳定版本化 ID。
        id: BROWSER_SCREENSHOT,
        // 声明截图动作。
        action: CapabilityAction::Screenshot,
        // 声明无前台输入的后台执行。
        execution: "background",
        // 正式契约要求隔离 worker 域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 不需要前台影响同意。
        requires_foreground_consent: false,
        // 不需要预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向浏览器截图输入契约。
        input_schema: "schema://browser/screenshot/v1",
    },
    // 登记完整应用关系图发现 capability。
    CapabilityDefinition {
        // 隔离于变更型 app facade surface。
        surface: CapabilitySurface::ApplicationDiscovery,
        // 使用稳定版本化 ID。
        id: APPLICATION_DISCOVER,
        // 声明发现动作。
        action: CapabilityAction::Discover,
        // Registry、Shell、ToolHelp 与 EnumWindows 均在主机无头域只读执行。
        execution: "host-headless",
        // 使用强类型主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读发现不需要前台确认。
        requires_foreground_consent: false,
        // 只读发现不要求预先前台确认。
        requires_upfront_foreground_consent: false,
        // 指向完整关系图输入契约。
        input_schema: "schema://application/discover/v1",
    },
    // 登记不改变冻结 Windows v1 语义的 Linux 应用清单 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::ApplicationDiscovery,
        id: APPLICATION_DISCOVER_V2,
        action: CapabilityAction::Discover,
        execution: "host-headless",
        execution_realm: ExecutionRealm::HostHeadless,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://application/discover/v2",
    },
    // 登记与版本二启动路线原子发布的 Linux 应用清单。
    CapabilityDefinition {
        surface: CapabilitySurface::ApplicationDiscovery,
        id: APPLICATION_DISCOVER_V3,
        action: CapabilityAction::Discover,
        execution: "host-headless",
        execution_realm: ExecutionRealm::HostHeadless,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://application/discover/v3",
    },
    // 登记统一应用 session 聚合发现 capability。
    CapabilityDefinition {
        // 与变更型 provider capability 隔离，避免被 generic run verb 解析。
        surface: CapabilitySurface::ApplicationSessionDiscovery,
        // 使用 C++ 兼容基线的稳定版本化 ID。
        id: APPLICATION_SESSION_DISCOVER,
        // 声明无副作用的发现动作。
        action: CapabilityAction::Discover,
        // 聚合只调用当前进程内的只读 provider session 入口。
        execution: "host-headless",
        // 使用强类型主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读 session 聚合不需要前台同意。
        requires_foreground_consent: false,
        // 只读 session 聚合不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向统一应用 session 发现契约。
        input_schema: "schema://application/session-discover/v1",
    },
    // 登记不改变 Windows v1 的 Linux 只读应用 session 聚合 capability。
    CapabilityDefinition {
        // 使用独立 surface，禁止进入变更型 app facade。
        surface: CapabilitySurface::ApplicationSessionDiscovery,
        // 使用显式版本二 ID。
        id: APPLICATION_SESSION_DISCOVER_V2,
        // 声明无副作用发现动作。
        action: CapabilityAction::Discover,
        // XDG 与 procfs 快照都在主机无头域完成。
        execution: "host-headless",
        // 使用 provider-neutral 主机无头执行域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读聚合不需要前景影响同意。
        requires_foreground_consent: false,
        // 聚合不要求预先前景同意。
        requires_upfront_foreground_consent: false,
        // 指向闭合版本二输入契约。
        input_schema: "schema://application/session-discover/v2",
    },
    // 登记 UIX-aware Linux 只读应用 session 聚合 capability。
    CapabilityDefinition {
        // 使用独立聚合 surface，不进入写操作 facade。
        surface: CapabilitySurface::ApplicationSessionDiscovery,
        // 使用显式版本三 ID，保持版本二零窗口契约不变。
        id: APPLICATION_SESSION_DISCOVER_V3,
        // 声明无副作用发现动作。
        action: CapabilityAction::Discover,
        // UIX 本机会话 socket 不抢焦点，也不宣称全局桌面覆盖。
        execution: "same-session-no-focus",
        // 使用协作式同会话执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 只读 Agent 清单不要求前景同意。
        requires_foreground_consent: false,
        // 不允许以前景同意改变 provider 范围。
        requires_upfront_foreground_consent: false,
        // 指向闭合版本三输入契约。
        input_schema: "schema://application/session-discover/v3",
    },
    // 登记同时发布认证启动状态的 UIX-aware Linux 只读应用 session 聚合。
    CapabilityDefinition {
        surface: CapabilitySurface::ApplicationSessionDiscovery,
        id: APPLICATION_SESSION_DISCOVER_V4,
        action: CapabilityAction::Discover,
        execution: "same-session-no-focus",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://application/session-discover/v4",
    },
    // 登记认证独立交互会话发现 capability。
    CapabilityDefinition {
        // 与变更型 app facade 隔离，避免被 generic run 误路由。
        surface: CapabilitySurface::InteractiveSessionDiscovery,
        // 使用稳定版本化 ID。
        id: INTERACTIVE_SESSION_DISCOVER,
        // 声明无副作用发现动作。
        action: CapabilityAction::Discover,
        // WTS 枚举与认证 pipe 握手都属于主机无头读取。
        execution: "host-headless",
        // 发现本身不在目标 worker 执行 mutation。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读发现不需要前台许可。
        requires_foreground_consent: false,
        // 只读发现不要求预先前台许可。
        requires_upfront_foreground_consent: false,
        // 指向已冻结的独立会话观察 schema。
        input_schema: "schema://interactive-session/discover/v1",
    },
    // 登记隔离有界可访问性树读取 capability。
    CapabilityDefinition {
        // 限定到只读可访问性 surface。
        surface: CapabilitySurface::Accessibility,
        // 使用稳定版本化 ID。
        id: ACCESSIBILITY_TREE_READ,
        // 声明读取动作。
        action: CapabilityAction::Read,
        // provider 必须位于 Job 隔离 worker。
        execution: "isolated-worker",
        // 使用强类型隔离 worker 域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 只读结构观察不需要前台确认。
        requires_foreground_consent: false,
        // 只读结构观察不要求预先前台确认。
        requires_upfront_foreground_consent: false,
        // 指向有界可访问性树输入契约。
        input_schema: "schema://accessibility/tree-read/v1",
    },
    // 登记 Linux AT-SPI 只读树候选；默认生产组合根不装配真实 bus provider。
    CapabilityDefinition {
        surface: CapabilitySurface::Accessibility,
        id: ACCESSIBILITY_TREE_READ_V2,
        action: CapabilityAction::Read,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://accessibility/tree-read/v2",
    },
    // UIX Agent 只读树通过同用户本机端点执行，不取得焦点或调用桌面输入。
    CapabilityDefinition {
        surface: CapabilitySurface::Accessibility,
        id: ACCESSIBILITY_TREE_READ_V3,
        action: CapabilityAction::Read,
        execution: "same-session-no-focus",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://accessibility/tree-read/v3",
    },
    // 登记 provider-neutral UIA 元素定位 capability。
    CapabilityDefinition {
        // 通过统一 app facade 精确窗口 provider 发布。
        surface: CapabilitySurface::App,
        // 使用稳定版本化 ID。
        id: UI_ELEMENT_LOCATE,
        // 声明无副作用读取动作。
        action: CapabilityAction::Read,
        // 语义搜索、bounds 与命中点均在 Job-bounded worker 中执行。
        execution: "isolated-worker",
        // 使用强类型隔离 worker 域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 只读定位不需要前台同意。
        requires_foreground_consent: false,
        // 只读定位不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一输入 schema。
        input_schema: "schema://ui/element-locate/v1",
    },
    // 登记协作式 UIX 快照上的只读元素定位 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向精确 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本避免改写冻结的 Windows UIA 物理坐标契约。
        id: UI_ELEMENT_LOCATE_V2,
        // 声明无副作用读取动作。
        action: CapabilityAction::Read,
        // 语义定位通过同用户应用 Agent 完成且不请求主机焦点。
        execution: "same-session-no-focus",
        // 使用强类型同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 只读定位不需要前景同意。
        requires_foreground_consent: false,
        // 不允许以前景同意改变协作式只读路线。
        requires_upfront_foreground_consent: false,
        // 指向严格版本二 UIX selector 输入契约。
        input_schema: "schema://ui/element-locate/v2",
    },
    // 登记 provider-neutral 语义元素动作 capability。
    CapabilityDefinition {
        // 通过统一 app facade 精确窗口 provider 发布。
        surface: CapabilitySurface::App,
        // 使用稳定版本化 ID。
        id: UI_ELEMENT_ACTION,
        // 复用统一 apply generic verb。
        action: CapabilityAction::Apply,
        // 公开粗粒度执行分类保持 background。
        execution: "background",
        // 使用强类型同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 语义动作不授权前台输入。
        requires_foreground_consent: false,
        // 语义动作不要求前台影响同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一输入 schema。
        input_schema: "schema://ui/element-action/v1",
    },
    // 登记协作式语义快照上的精确元素动作 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向精确 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本避免改写冻结的 UIA selector 契约。
        id: UI_ELEMENT_ACTION_V2,
        // 复用统一 apply generic verb。
        action: CapabilityAction::Apply,
        // 动作在应用 UI turn 内执行但不请求主机前景激活。
        execution: "background",
        // 使用强类型同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 应用内确认 UI 不等同于主机前景输入授权。
        requires_foreground_consent: false,
        // 不允许把前景同意当作 UIX 动作授权。
        requires_upfront_foreground_consent: false,
        // 指向版本二精确快照输入契约。
        input_schema: "schema://ui/element-action/v2",
    },
    // 登记 provider-neutral UIA 元素等待 capability。
    CapabilityDefinition {
        // 通过统一 app facade 精确窗口 provider 发布。
        surface: CapabilitySurface::App,
        // 使用稳定版本化 ID。
        id: UI_ELEMENT_WAIT,
        // 声明无副作用读取动作。
        action: CapabilityAction::Read,
        // 每次采样都使用 Job-bounded UIA worker。
        execution: "isolated-worker",
        // 使用强类型隔离 worker 域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 只读等待不需要前台同意。
        requires_foreground_consent: false,
        // 只读等待不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一输入 schema。
        input_schema: "schema://ui/element-wait/v1",
    },
    // 登记 UIX 精确窗口语义修订驱动的元素等待 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::App,
        id: UI_ELEMENT_WAIT_V2,
        action: CapabilityAction::Read,
        execution: "background",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://ui/element-wait/v2",
    },
    // 登记 UIX snapshot-scoped 语义动作与提交后条件同步 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向精确 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本化 ID，避免扩写元素动作或元素等待契约。
        id: UI_ELEMENT_TRANSITION,
        // 动作与条件同步统一使用 apply generic verb。
        action: CapabilityAction::Apply,
        // 语义 mutation 在应用会话内执行且不请求主机焦点。
        execution: "background",
        // 使用强类型同会话无焦点执行域。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 应用内语义动作需要显式确认，但不需要前台同意。
        requires_foreground_consent: false,
        // 不允许以前台同意改变协作式语义路线。
        requires_upfront_foreground_consent: false,
        // 指向 snapshot-scoped 元素 transition 输入契约。
        input_schema: "schema://ui/element-transition/v1",
    },
    // 登记 provider-neutral 精确窗口关闭等待 capability。
    CapabilityDefinition {
        // 通过统一 app facade 精确窗口 provider 发布。
        surface: CapabilitySurface::App,
        // 使用稳定版本化 ID。
        id: WINDOW_CLOSED_WAIT,
        // 声明无副作用读取动作。
        action: CapabilityAction::Read,
        // 公开 descriptor 使用既有后台执行分类。
        execution: "background",
        // 使用强类型主机无头执行域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读等待不需要前台同意。
        requires_foreground_consent: false,
        // 只读等待不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一输入 schema。
        input_schema: "schema://window/closed-wait/v1",
    },
    // 登记 UIX Agent 精确 generation 关闭等待 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向当前协作式 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立版本二 ID，避免扩写主机轮询版本一语义。
        id: WINDOW_CLOSED_WAIT_V2,
        // 只观察 Agent 发布的关闭事实。
        action: CapabilityAction::Read,
        // 公开粗粒度执行分类保持 background。
        execution: "background",
        // 等待发生在认证应用会话但不请求焦点。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 只读等待不需要前台同意。
        requires_foreground_consent: false,
        // 只读等待不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本二输入 schema。
        input_schema: "schema://window/closed-wait/v2",
    },
    // 登记协作式精确窗口修订与呈现等待 capability。
    CapabilityDefinition {
        // 通过统一 app facade 面向当前 UIX 窗口发布。
        surface: CapabilitySurface::App,
        // 使用独立稳定版本化 ID。
        id: WINDOW_REVISION_WAIT,
        // 等待只观察应用发布的单调修订事实。
        action: CapabilityAction::Read,
        // 公开粗粒度执行分类保持 background。
        execution: "background",
        // 等待发生在认证应用的同一交互会话但不请求焦点。
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        // 只读等待不需要前台同意。
        requires_foreground_consent: false,
        // 只读等待不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向严格版本一输入 schema。
        input_schema: "schema://window/revision-wait/v1",
    },
    // 登记进程发现 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::Process,
        id: PROCESS_DISCOVER,
        action: CapabilityAction::Discover,
        execution: "host-headless",
        // ToolHelp 发现属于主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://process/discover/v1",
    },
    // 登记进程元数据读取 capability。
    CapabilityDefinition {
        surface: CapabilitySurface::Process,
        id: PROCESS_METADATA_READ,
        action: CapabilityAction::Read,
        execution: "host-headless",
        // 进程元数据读取属于主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://process/metadata-read/v1",
    },
    // 登记 Linux 精确进程代际的温和终止 capability。
    CapabilityDefinition {
        // 直接属于 process surface，不复用冻结的 Windows app.close v1。
        surface: CapabilitySurface::Process,
        // 使用独立版本二 ID，固定为同 UID 非 root 的 pidfd SIGTERM。
        id: PROCESS_TERMINATE_GRACEFUL_V2,
        // 生命周期 mutation 使用关闭动作类别。
        action: CapabilityAction::Close,
        // 不激活窗口且不发送桌面输入。
        execution: "background",
        // 信号请求在宿主后台执行。
        execution_realm: ExecutionRealm::HostBackground,
        // 不需要前景激活同意。
        requires_foreground_consent: false,
        // mutation 确认由统一 Policy 独立强制。
        requires_upfront_foreground_consent: false,
        // 指向只接受总 deadline 的严格版本二输入。
        input_schema: "schema://process/termination-graceful/v2",
    },
    // 登记 Linux 精确进程代际的显式强制终止 capability。
    CapabilityDefinition {
        // 独立属于 process surface，不扩写 Windows critical/integrity v1。
        surface: CapabilitySurface::Process,
        // 使用版本二 ID，禁止温和路线自动升级或 input 选择风险。
        id: PROCESS_TERMINATE_FORCE_V2,
        // 生命周期 mutation 使用关闭动作类别。
        action: CapabilityAction::Close,
        // 内核强制终止不激活窗口或发送桌面输入。
        execution: "background",
        // 固定为宿主后台执行域。
        execution_realm: ExecutionRealm::HostBackground,
        // 不需要前景激活同意。
        requires_foreground_consent: false,
        // mutation 确认由统一 Policy 强制。
        requires_upfront_foreground_consent: false,
        // 指向独立强制路线输入 schema。
        input_schema: "schema://process/termination-force/v2",
    },
    // 注册窗口发现 capability。
    CapabilityDefinition {
        // 限定到只读窗口 surface。
        surface: CapabilitySurface::Window,
        // 使用稳定版本化 ID。
        id: WINDOW_DISCOVER,
        // 声明发现动作。
        action: CapabilityAction::Discover,
        // Win32 顶层窗口枚举无需前台或 UIA。
        execution: "host-headless",
        // 窗口发现属于主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读枚举不需要前台确认。
        requires_foreground_consent: false,
        // 只读枚举不要求预先前台确认。
        requires_upfront_foreground_consent: false,
        // 指向窗口发现输入契约。
        input_schema: "schema://window/discover/v1",
    },
    // 登记 Linux AT-SPI partial exporter 窗口发现候选。
    CapabilityDefinition {
        surface: CapabilitySurface::Window,
        id: WINDOW_DISCOVER_V2,
        action: CapabilityAction::Discover,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/discover/v2",
    },
    // UIX Agent 只发现显式发布本机端点的协作式应用窗口。
    CapabilityDefinition {
        surface: CapabilitySurface::Window,
        id: WINDOW_DISCOVER_V3,
        action: CapabilityAction::Discover,
        execution: "same-session-no-focus",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/discover/v3",
    },
    // 注册窗口元数据读取 capability。
    CapabilityDefinition {
        // 限定到只读窗口 surface。
        surface: CapabilitySurface::Window,
        // 使用稳定版本化 ID。
        id: WINDOW_METADATA_READ,
        // 声明读取动作。
        action: CapabilityAction::Read,
        // 精确重新发现仍在 host-headless 域执行。
        execution: "host-headless",
        // 窗口元数据读取属于主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读读取不需要前台确认。
        requires_foreground_consent: false,
        // 只读读取不要求预先前台确认。
        requires_upfront_foreground_consent: false,
        // 指向窗口元数据读取输入契约。
        input_schema: "schema://window/metadata-read/v1",
    },
    // 登记 Linux AT-SPI partial exporter 窗口元数据只读候选。
    CapabilityDefinition {
        surface: CapabilitySurface::Window,
        id: WINDOW_METADATA_READ_V2,
        action: CapabilityAction::Read,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/metadata-read/v2",
    },
    // UIX Agent 元数据读取在使用时重新认证端点和窗口代际。
    CapabilityDefinition {
        surface: CapabilitySurface::Window,
        id: WINDOW_METADATA_READ_V3,
        action: CapabilityAction::Read,
        execution: "same-session-no-focus",
        execution_realm: ExecutionRealm::SameSessionNoFocus,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://window/metadata-read/v3",
    },
    // 注册精确窗口零帧捕获预检 capability。
    CapabilityDefinition {
        // 限定到只读窗口 surface。
        surface: CapabilitySurface::Window,
        // 使用 C++ 兼容基线的稳定版本化 ID。
        id: WINDOW_CAPTURE_PREFLIGHT,
        // 声明只读动作。
        action: CapabilityAction::Read,
        // 预检只查询窗口、DWM 与 WGC item 元数据。
        execution: "host-headless",
        // 不创建帧池或捕获 session，因此属于主机无头域。
        execution_realm: ExecutionRealm::HostHeadless,
        // 只读预检不需要前台同意。
        requires_foreground_consent: false,
        // 只读预检不要求预先前台同意。
        requires_upfront_foreground_consent: false,
        // 指向既有精确窗口预检输入契约。
        input_schema: "schema://window/capture-preflight/v1",
    },
    // 注册精确窗口首帧元数据探针 capability。
    CapabilityDefinition {
        // 限定到窗口 surface。
        surface: CapabilitySurface::Window,
        // 使用稳定版本化 ID。
        id: WINDOW_CAPTURE_FRAME_PROBE,
        // 声明敏感只读动作。
        action: CapabilityAction::Read,
        // 首帧探针只允许在隔离 worker 内执行。
        execution: "isolated-worker",
        // 使用严格隔离执行域。
        execution_realm: ExecutionRealm::IsolatedWorker,
        // 后台捕获不要求前台同意。
        requires_foreground_consent: false,
        // 不要求前台同意但要求逐操作确认。
        requires_upfront_foreground_consent: false,
        // 指向首帧探针输入契约。
        input_schema: "schema://window/capture-frame-probe/v1",
    },
    // 三项 MPRIS v2 作为协调候选登记，生产始终没有 execution realm。
    CapabilityDefinition {
        surface: CapabilitySurface::Media,
        id: MEDIA_SESSION_DISCOVER_V2,
        action: CapabilityAction::Discover,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/session-discover/v2",
    },
    CapabilityDefinition {
        surface: CapabilitySurface::Media,
        id: MEDIA_PLAYBACK_STATE_READ_V2,
        action: CapabilityAction::Read,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/playback-state/v2",
    },
    CapabilityDefinition {
        surface: CapabilitySurface::Media,
        id: MEDIA_PLAYBACK_CONTROL_V2,
        action: CapabilityAction::Apply,
        execution: "none",
        execution_realm: ExecutionRealm::None,
        requires_foreground_consent: false,
        requires_upfront_foreground_consent: false,
        input_schema: "schema://media/playback-control/v2",
    },
];
