//! 实现 `act/observation-worker/v1` 的 Rust companion worker。

// 导入有界树遍历与标准流接口。
use std::{
    // 导入广度优先队列。
    collections::VecDeque,
    // 导入标准输入输出读写 trait。
    io::{Read, Write},
};

// 导入协议反序列化。
use serde::Deserialize;
// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 把错误码实现保留为 Observation Worker 协议边界的普通私有类型。
mod error_code;
// 导入当前 Worker 私有封闭错误码。
use error_code::ObservationWorkerErrorCode;
// 把语义定位实现保留为 observation worker 私有 Component。
mod location;

// 导入 Windows COM 与只读 UIA 接口。
use windows::Win32::{
    // 导入通用 HRESULT 常量与窗口句柄。
    Foundation::{E_ACCESSDENIED, HWND, RPC_E_CHANGED_MODE},
    // 导入 COM apartment 与对象创建接口。
    System::Com::{
        // 导入进程内 server 上下文。
        CLSCTX_INPROC_SERVER,
        // 导入多线程 apartment 标志。
        COINIT_MULTITHREADED,
        // 导入 COM 实例创建函数。
        CoCreateInstance,
        // 导入 COM apartment 初始化函数。
        CoInitializeEx,
        // 导入 COM apartment 释放函数。
        CoUninitialize,
    },
    // 导入只读 UI Automation 接口与错误常量。
    UI::Accessibility::{
        // 导入完整 element 模式，使缓存快照仍可继续树遍历。
        AutomationElementMode_Full,
        // 导入系统 UIA coclass。
        CUIAutomation,
        // 导入 automation 接口。
        IUIAutomation,
        // 导入 cache request 接口。
        IUIAutomationCacheRequest,
        // 导入 element 接口。
        IUIAutomationElement,
        // 导入单 element 缓存范围。
        TreeScope_Element,
        // 导入树节点允许公开的缓存属性标识。
        UIA_AutomationIdPropertyId,
        // 导入类名缓存属性标识。
        UIA_ClassNamePropertyId,
        // 导入 control type 缓存属性标识。
        UIA_ControlTypePropertyId,
        // 导入 element stale HRESULT。
        UIA_E_ELEMENTNOTAVAILABLE,
        // 导入 framework 缓存属性标识。
        UIA_FrameworkIdPropertyId,
        // 导入 enabled 缓存属性标识。
        UIA_IsEnabledPropertyId,
        // 导入 offscreen 缓存属性标识。
        UIA_IsOffscreenPropertyId,
        // 导入名称缓存属性标识。
        UIA_NamePropertyId,
    },
};

// 导入 opaque 目标解析、窗口重新发现与统一错误类型。
use crate::{
    // 导入窗口当前快照读取。
    adapters::windows::{enumerate_windows, opaque_window_session_id},
    // 导入共享 selector 与 fail-closed opaque 目标原语。
    components::{
        // 导入 provider-neutral 语义 selector。
        accessibility_selector::AccessibilitySelector,
        // 导入 opaque 目标原语。
        opaque_id::{
            // 导入目标 ID 生成与解析。
            OpaqueTargetId,
            // 导入目标类别。
            OpaqueTargetKind,
            // 导入唯一匹配结果。
            OpaqueTargetMatch,
            // 导入唯一匹配函数。
            match_opaque_target,
        },
    },
    // 导入结构化错误。
    domain::{AppControlError, AppResult},
};

// 固定内部协议版本。
const CONTRACT_VERSION: &str = "act/observation-worker/v1";
// 限制 worker 请求总字节数。
const MAXIMUM_REQUEST_BYTES: u64 = 64 * 1024;
// 限制 worker 内窗口重新发现数量。
const MAXIMUM_WINDOWS: usize = 4096;

// 表示固定 worker request。
#[derive(Debug, Deserialize)]
// 对齐 JSON camelCase 字段并拒绝协议外输入。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerRequest {
    // 保存协议版本。
    contract_version: String,
    // 保存只读 operation。
    operation: String,
    // 保存 canonical s2:w 目标。
    session_id: String,
    // 保存树深度上限。
    #[serde(default)]
    maximum_depth: Option<usize>,
    // 保存节点数量上限。
    #[serde(default)]
    maximum_items: Option<usize>,
    // 保存 UIA view。
    #[serde(default)]
    view: Option<String>,
    // 保存定位 operation 的 provider-neutral selector。
    #[serde(default)]
    selector: Option<AccessibilitySelector>,
}

// 保存成功初始化的 COM apartment。
struct ComApartment {
    // 标记当前 worker 是否需要执行 CoUninitialize。
    should_uninitialize: bool,
}

// 提供 worker COM 生命周期管理。
impl ComApartment {
    // 初始化 MTA 或接受既有 apartment。
    fn initialize() -> AppResult<Self> {
        // 尝试初始化多线程 apartment。
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // 成功时必须成对释放。
        if status.is_ok() {
            // 返回拥有 apartment 的守卫。
            return Ok(Self {
                // 记录释放责任。
                should_uninitialize: true,
            });
        }
        // 已有不同 apartment 时与 C++ 对照实现一致地继续。
        if status == RPC_E_CHANGED_MODE {
            // 返回不拥有既有 apartment 的守卫。
            return Ok(Self {
                // 不释放并非由本函数初始化的 apartment。
                should_uninitialize: false,
            });
        }
        // 其他 COM 初始化失败结构化分类。
        Err(accessibility_status_error("CoInitializeEx", status))
    }
}

// 成对释放由当前 worker 初始化的 COM apartment。
impl Drop for ComApartment {
    // 执行资源回收。
    fn drop(&mut self) {
        // 只释放当前函数拥有的 apartment 初始化。
        if self.should_uninitialize {
            // 与成功 CoInitializeEx 成对调用。
            unsafe { CoUninitialize() };
        }
    }
}

// 保存树遍历中的 element 与深度。
struct PendingNode {
    // 保存 COM element 智能指针。
    element: IUIAutomationElement,
    // 保存当前节点深度。
    depth: usize,
}

// 把 UIA 错误收敛为稳定公开分类。
fn accessibility_error(operation: &str, error: &windows::core::Error) -> AppControlError {
    // 委托 HRESULT 分类函数。
    accessibility_status_error(operation, error.code())
}

// 把 HRESULT 收敛为稳定公开分类。
fn accessibility_status_error(
    // 接收公开操作名。
    operation: &str,
    // 接收 provider HRESULT。
    status: windows::core::HRESULT,
) -> AppControlError {
    // 读取 HRESULT 的无符号位模式。
    let status_bits = status.0 as u32;
    // 明确权限拒绝保持独立分类。
    let code = if status == E_ACCESSDENIED {
        // 选择权限拒绝类别。
        ObservationWorkerErrorCode::PermissionDenied
    // element provider 明确报告目标过期。
    } else if status_bits == UIA_E_ELEMENTNOTAVAILABLE {
        // 选择目标过期类别。
        ObservationWorkerErrorCode::StaleSession
    } else {
        // 选择 UIA 不可用类别。
        ObservationWorkerErrorCode::AccessibilityUnavailable
    };
    // 返回不含 HWND、PID 或 provider identity 的错误。
    code.error(
        // 仅保留操作名与 HRESULT。
        format!("{operation} failed (HRESULT 0x{status_bits:08x})."),
    )
}

// 构造系统 UI Automation 对象。
fn create_automation() -> AppResult<IUIAutomation> {
    // 创建进程内只读 UIA client。
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        // 映射 provider 可用性错误。
        .map_err(|error| accessibility_error("CoCreateInstance(CUIAutomation)", &error))
}

// 创建仅覆盖公开树节点属性的 UIA cache request。
fn create_tree_cache_request(
    // 接收 worker 私有 UIA client。
    automation: &IUIAutomation,
) -> AppResult<IUIAutomationCacheRequest> {
    // 创建独立 cache request。
    let cache_request = unsafe { automation.CreateCacheRequest() }
        // 映射 cache request 创建失败。
        .map_err(|error| accessibility_error("IUIAutomation::CreateCacheRequest", &error))?;
    // 只缓存当前 element，避免 provider 无界预取子树。
    unsafe { cache_request.SetTreeScope(TreeScope_Element) }
        // 映射 scope 配置失败。
        .map_err(|error| accessibility_error("IUIAutomationCacheRequest::SetTreeScope", &error))?;
    // 保留完整 element，使同一快照对象可继续 walker 遍历。
    unsafe { cache_request.SetAutomationElementMode(AutomationElementMode_Full) }
        // 映射 element mode 配置失败。
        .map_err(|error| {
            // 返回统一结构化错误。
            accessibility_error(
                "IUIAutomationCacheRequest::SetAutomationElementMode",
                &error,
            )
        })?;
    // 固定所有允许进入公共树节点的属性集合。
    let properties = [
        // 缓存公开名称。
        UIA_NamePropertyId,
        // 缓存公开 automation ID。
        UIA_AutomationIdPropertyId,
        // 缓存公开类名。
        UIA_ClassNamePropertyId,
        // 缓存公开 framework ID。
        UIA_FrameworkIdPropertyId,
        // 缓存 control type。
        UIA_ControlTypePropertyId,
        // 缓存 enabled 状态。
        UIA_IsEnabledPropertyId,
        // 缓存 offscreen 状态。
        UIA_IsOffscreenPropertyId,
    ];
    // 逐项配置一次 request，而不是逐节点跨 provider 读取。
    for property in properties {
        // 将允许属性加入同一个 cache request。
        unsafe { cache_request.AddProperty(property) }
            // 映射属性配置失败。
            .map_err(|error| {
                // 返回统一结构化错误且不暴露原生属性标识。
                accessibility_error("IUIAutomationCacheRequest::AddProperty", &error)
            })?;
    }
    // 返回可复用的有界 cache request。
    Ok(cache_request)
}

// 严格解析并验证 worker 请求。
fn parse_request(text: &str) -> AppResult<WorkerRequest> {
    // 要求恰好一个 JSON value，允许末尾换行。
    let request = serde_json::from_str::<WorkerRequest>(text.trim()).map_err(|_| {
        // 不回显潜在敏感输入。
        ObservationWorkerErrorCode::InvalidArgument.error(
            // 明确协议不匹配。
            "The observation worker request violates protocol v1.",
        )
    })?;
    // 验证固定协议版本。
    if request.contract_version != CONTRACT_VERSION {
        // 拒绝未知版本。
        return Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 不协商未实现协议。
            "The observation worker request violates protocol v1.",
        ));
    }
    // 严格解析 canonical opaque 目标。
    let target = OpaqueTargetId::parse(&request.session_id);
    // 只允许 s2:w 窗口目标。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧版本和其他目标类别。
        return Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 指明 worker 只接受 canonical 窗口目标。
            "The observation worker requires a canonical s2:w target.",
        ));
    }
    // root operation 不接受树参数。
    if request.operation == "accessibility-root" {
        // 拒绝混用树字段，保持协议固定。
        if request.maximum_depth.is_some()
            || request.maximum_items.is_some()
            || request.view.is_some()
            // root 不接受 selector。
            || request.selector.is_some()
        {
            // 返回稳定参数错误。
            return Err(ObservationWorkerErrorCode::InvalidArgument.error(
                // 指明 root 形状固定。
                "The accessibility-root request contains tree-only fields.",
            ));
        }
        // 返回已验证 root 请求。
        return Ok(request);
    }
    // 定位与树 operation 共用有界查询字段。
    let bounded_operation = matches!(
        // 读取 operation 文本。
        request.operation.as_str(),
        // 接受既有树读取与新增语义定位。
        "accessibility-tree" | "accessibility-element-locate"
    );
    // 未知 operation 作为 capability 缺口返回。
    if !bounded_operation {
        // 拒绝 worker 扩展为第二控制面。
        return Err(ObservationWorkerErrorCode::CapabilityGap.error(
            // 明确仅支持只读 UIA。
            "The isolated observation worker only supports read-only UIA.",
        ));
    }
    // 树读取不得携带定位 selector。
    if request.operation == "accessibility-tree" && request.selector.is_some() {
        // 返回稳定参数错误。
        return Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 指明树协议不接受定位字段。
            "The accessibility-tree request contains locate-only fields.",
        ));
    }
    // 定位必须携带合法 selector。
    if request.operation == "accessibility-element-locate"
        // 缺失或验证失败都拒绝。
        && request
            .selector
            .as_ref()
            .is_none_or(|selector| selector.validate().is_err())
    {
        // 返回稳定参数错误且不回显 selector。
        return Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 指明定位 selector 契约。
            "The accessibility-element-locate selector is invalid.",
        ));
    }
    // 验证有界查询的深度硬边界。
    if request.maximum_depth.is_none_or(|value| value > 20)
        // 验证树的数量硬边界。
        || request.maximum_items.is_none_or(|value| !(1..=4096).contains(&value))
        // 验证 view 封闭枚举。
        || request
            .view
            .as_deref()
            .is_none_or(|value| !matches!(value, "control" | "raw"))
    {
        // 返回稳定参数错误。
        return Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 与 C++ 对照实现保持消息语义。
            "The bounded accessibility-tree request is invalid.",
        ));
    }
    // 返回已验证树请求。
    Ok(request)
}

// 在 worker 当前快照中重新发现唯一窗口。
fn resolve_window(session_id: &str) -> AppResult<isize> {
    // 枚举当前全部顶层窗口。
    let mut windows = enumerate_windows()?;
    // 只保留 C++ 契约中的可见有标题应用窗口。
    windows.retain(|window| window.visible && !window.title.is_empty());
    // 限制候选清单，避免无界 provider 输入。
    windows.truncate(MAXIMUM_WINDOWS);
    // 使用当前私有事实重新生成 opaque ID 并完整扫描碰撞。
    match match_opaque_target(session_id, &windows, |window| {
        // 生成当前候选 canonical s2:w。
        Some(opaque_window_session_id(window))
    }) {
        // 唯一命中时只把 HWND 留在 worker 内部。
        OpaqueTargetMatch::Unique(window) => Ok(window.hwnd),
        // 零命中表示目标已消失或代际变化。
        OpaqueTargetMatch::Missing => Err(ObservationWorkerErrorCode::StaleSession.error(
            // 不公开 native 目标。
            "The opaque application-window session no longer resolves.",
        )),
        // 多命中必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(ObservationWorkerErrorCode::AmbiguousTarget.error(
            // 明确不会选择任意候选。
            "The opaque application-window session resolves to multiple windows.",
        )),
    }
}

// 读取一个字符串 UIA 属性，失败时返回空字符串与 incomplete。
fn string_property(
    // 接收延迟属性读取闭包。
    read: impl FnOnce() -> windows::core::Result<windows::core::BSTR>,
) -> (String, bool) {
    // 执行单个只读属性调用。
    match read() {
        // 成功时转换为 Rust 字符串。
        Ok(value) => (value.to_string(), true),
        // 失败时不猜测值。
        Err(_) => (String::new(), false),
    }
}

// 把一个 UIA element 投影为隐私安全节点。
fn node_json(
    // 接收 element 智能指针。
    element: &IUIAutomationElement,
    // 接收节点深度。
    depth: usize,
    // 接收父窗口 canonical ID。
    session_id: &str,
) -> AppResult<Value> {
    // 从已构建快照读取公开名称。
    let (name, name_ok) = string_property(|| unsafe { element.CachedName() });
    // 从已构建快照读取公开 automation ID。
    let (automation_id, automation_id_ok) =
        string_property(|| unsafe { element.CachedAutomationId() });
    // 从已构建快照读取公开类名。
    let (class_name, class_name_ok) = string_property(|| unsafe { element.CachedClassName() });
    // 从已构建快照读取公开 framework ID。
    let (framework_id, framework_id_ok) =
        string_property(|| unsafe { element.CachedFrameworkId() });
    // 从已构建快照读取 control type，失败时使用 0 哨兵。
    let (control_type, control_type_ok) = unsafe { element.CachedControlType() }
        // 映射成功值。
        .map(|value| (value.0, true))
        // 映射失败值。
        .unwrap_or((0, false));
    // 从已构建快照读取 enabled，失败时使用 false 哨兵。
    let (enabled, enabled_ok) = unsafe { element.CachedIsEnabled() }
        // 映射成功值。
        .map(|value| (value.as_bool(), true))
        // 映射失败值。
        .unwrap_or((false, false));
    // 从已构建快照读取 offscreen，失败时使用 false 哨兵。
    let (offscreen, offscreen_ok) = unsafe { element.CachedIsOffscreen() }
        // 映射成功值。
        .map(|value| (value.as_bool(), true))
        // 映射失败值。
        .unwrap_or((false, false));
    // 从 UIA RuntimeId 与精确父窗口生成不泄漏 provider 数据的稳定 element ID。
    let node_id = crate::components::uia_runtime_id::runtime_element_id(element, session_id)
        // RuntimeId 缺失或损坏时 fail closed，不回退到易漂移树路径。
        .map_err(|error| accessibility_error("IUIAutomationElement::GetRuntimeId", &error))?;
    // 只有全部允许属性均成功时标记完整。
    let property_read_complete = name_ok
        // 合并 automation ID 状态。
        && automation_id_ok
        // 合并类名状态。
        && class_name_ok
        // 合并 framework 状态。
        && framework_id_ok
        // 合并 control type 状态。
        && control_type_ok
        // 合并 enabled 状态。
        && enabled_ok
        // 合并 offscreen 状态。
        && offscreen_ok;
    // 只输出契约允许的结构属性。
    Ok(json!({
        // 输出 opaque 节点 ID。
        "nodeId": node_id,
        // 输出有界深度。
        "depth": depth,
        // 输出允许的结构文本。
        "name": name,
        // 输出允许的自动化 ID。
        "automationId": automation_id,
        // 输出允许的类名。
        "className": class_name,
        // 输出允许的 framework ID。
        "frameworkId": framework_id,
        // 输出数值 control type。
        "controlType": control_type,
        // 输出 enabled 结构状态。
        "enabled": enabled,
        // 输出 offscreen 结构状态。
        "offscreen": offscreen,
        // 输出允许属性读取完整性。
        "propertyReadComplete": property_read_complete,
    }))
}

// 读取 root-only UIA 结构事实。
fn observe_root(hwnd: isize) -> AppResult<Value> {
    // 初始化 worker 私有 COM apartment。
    let _apartment = ComApartment::initialize()?;
    // 创建 UI Automation client。
    let automation = create_automation()?;
    // 从 worker 内部 HWND 获得 root element。
    let root = unsafe { automation.ElementFromHandle(HWND(hwnd as *mut std::ffi::c_void)) }
        // 映射 stale、permission 或 provider 缺口。
        .map_err(|error| accessibility_error("IUIAutomation::ElementFromHandle", &error))?;
    // root-only 必须完整读取名称。
    let name = unsafe { root.CurrentName() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentName", &error))?
        // 转换为公共 UTF-8 字符串。
        .to_string();
    // 读取 automation ID。
    let automation_id = unsafe { root.CurrentAutomationId() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentAutomationId", &error))?
        // 转换为公共 UTF-8 字符串。
        .to_string();
    // 读取类名。
    let class_name = unsafe { root.CurrentClassName() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentClassName", &error))?
        // 转换为公共 UTF-8 字符串。
        .to_string();
    // 读取 framework ID。
    let framework_id = unsafe { root.CurrentFrameworkId() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentFrameworkId", &error))?
        // 转换为公共 UTF-8 字符串。
        .to_string();
    // 读取 control type。
    let control_type = unsafe { root.CurrentControlType() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentControlType", &error))?
        // 只取公开数值。
        .0;
    // 读取 enabled 状态。
    let enabled = unsafe { root.CurrentIsEnabled() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentIsEnabled", &error))?
        // 转换为 Rust bool。
        .as_bool();
    // 读取 offscreen 状态。
    let offscreen = unsafe { root.CurrentIsOffscreen() }
        // 映射属性错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentIsOffscreen", &error))?
        // 转换为 Rust bool。
        .as_bool();
    // 返回协议 data。
    Ok(json!({
        // 输出名称。
        "name": name,
        // 输出 automation ID。
        "automationId": automation_id,
        // 输出类名。
        "className": class_name,
        // 输出 framework ID。
        "frameworkId": framework_id,
        // 输出 control type。
        "controlType": control_type,
        // 输出 enabled。
        "enabled": enabled,
        // 输出 offscreen。
        "offscreen": offscreen,
    }))
}

// 读取有界 UIA 树。
fn observe_tree(
    // 接收 worker 私有 HWND。
    hwnd: isize,
    // 接收父窗口 canonical ID。
    session_id: &str,
    // 接收深度上限。
    maximum_depth: usize,
    // 接收节点数量上限。
    maximum_items: usize,
    // 接收 control/raw view。
    view: &str,
) -> AppResult<Value> {
    // 初始化 worker 私有 COM apartment。
    let _apartment = ComApartment::initialize()?;
    // 创建 UI Automation client。
    let automation = create_automation()?;
    // 创建只覆盖单节点公开属性的复用 cache request。
    let cache_request = create_tree_cache_request(&automation)?;
    // 从 worker 内部 HWND 获得带属性快照的 root element。
    let root = unsafe {
        // 在 root 发现调用中同时构建缓存。
        automation.ElementFromHandleBuildCache(
            // 传入 worker 私有 HWND。
            HWND(hwnd as *mut std::ffi::c_void),
            // 传入复用 cache request。
            &cache_request,
        )
    }
    // 映射 stale、permission 或 provider 缺口。
    .map_err(|error| {
        // 返回统一结构化错误。
        accessibility_error("IUIAutomation::ElementFromHandleBuildCache", &error)
    })?;
    // 按请求选择只读 walker。
    let walker = unsafe {
        // raw view 与 control view 使用封闭分支。
        if view == "raw" {
            // 请求 RawView walker。
            automation.RawViewWalker()
        } else {
            // 请求 ControlView walker。
            automation.ControlViewWalker()
        }
    }
    // 映射 walker provider 缺口。
    .map_err(|error| accessibility_error("IUIAutomation::TreeWalker", &error))?;
    // 从 root 和深度 0 开始 BFS。
    let mut pending = VecDeque::from([PendingNode {
        // 保存 root element。
        element: root,
        // root 深度为 0。
        depth: 0,
    }]);
    // 预分配有界节点数组。
    let mut nodes = Vec::with_capacity(maximum_items);
    // 保存实际访问节点数。
    let mut visited = 0_usize;
    // 保存遍历 API 是否完整。
    let mut traversal_complete = true;
    // 只在队列非空且未达到数量上限时继续。
    while let Some(current) = pending.pop_front() {
        // 数量上限到达时把当前节点放回，标记截断。
        if nodes.len() >= maximum_items {
            // 恢复尚未处理节点。
            pending.push_front(current);
            // 退出有界遍历。
            break;
        }
        // 记录当前节点已访问。
        visited = visited.saturating_add(1);
        // 投影当前节点的允许属性。
        nodes.push(node_json(
            // 传入 element。
            &current.element,
            // 传入深度。
            current.depth,
            // 传入父窗口 opaque ID。
            session_id,
        )?);
        // 深度上限到达时不读取子节点。
        if current.depth >= maximum_depth {
            // 继续下一个待处理节点。
            continue;
        }
        // 尝试读取首个带属性快照的子节点。
        let mut child = match unsafe {
            // 在 child 发现调用中同时构建缓存。
            walker.GetFirstChildElementBuildCache(&current.element, &cache_request)
        } {
            // 成功时保存可选 child。
            Ok(child) => Some(child),
            // 失败时标记遍历不完整。
            Err(_) => {
                // 记录 provider traversal 缺口。
                traversal_complete = false;
                // 当前节点没有可安全继续的 child。
                None
            }
        };
        // 按 sibling 顺序收集子节点。
        while let Some(current_child) = child {
            // 在移动 current_child 前读取带属性快照的下一个 sibling。
            let next = unsafe {
                // 在 sibling 发现调用中同时构建缓存。
                walker.GetNextSiblingElementBuildCache(&current_child, &cache_request)
            };
            // 把 child 加入 BFS 队列。
            pending.push_back(PendingNode {
                // 移交 child 智能指针。
                element: current_child,
                // 子节点深度加一。
                depth: current.depth.saturating_add(1),
            });
            // 解包下一个 sibling 或标记遍历不完整。
            child = match next {
                // 成功时保存下一个 sibling。
                Ok(next) => Some(next),
                // 失败时停止当前 sibling 链。
                Err(_) => {
                    // 标记 provider traversal 缏口。
                    traversal_complete = false;
                    // 停止 sibling 遍历。
                    None
                }
            };
        }
    }
    // 队列残留或 walker 缺口都表示截断。
    let truncated = !pending.is_empty() || !traversal_complete;
    // 返回协议 data。
    Ok(json!({
        // 输出实际访问数量。
        "visited": visited,
        // 输出截断状态。
        "truncated": truncated,
        // 输出有界节点数组。
        "nodes": nodes,
    }))
}

// 在 worker 内执行已经过协议验证的请求。
fn execute_request(request: WorkerRequest) -> AppResult<Value> {
    // 定位 operation 在任何坐标相关调用前切换线程 DPI 上下文。
    let _dpi_awareness = if request.operation == "accessibility-element-locate" {
        // 建立自动恢复的 Per-Monitor-V2 守卫。
        Some(location::DpiAwarenessGuard::enter()?)
    } else {
        // 其他只读 operation 保持既有上下文。
        None
    };
    // 每次使用时重新解析 opaque 窗口目标。
    let hwnd = resolve_window(&request.session_id)?;
    // root operation 只读 root 结构。
    if request.operation == "accessibility-root" {
        // 委托 root 观察。
        return observe_root(hwnd);
    }
    // 语义定位只读取边界、状态和 provider clickable point。
    if request.operation == "accessibility-element-locate" {
        // 解析阶段已保证定位字段完整。
        return location::observe(
            // 传入 worker 私有 HWND。
            hwnd,
            // 传入 canonical 父窗口 ID。
            &request.session_id,
            // 传入已验证深度。
            request.maximum_depth.ok_or_else(|| {
                // 防御性协议错误。
                ObservationWorkerErrorCode::InvalidArgument.error("maximumDepth is required.")
            })?,
            // 传入已验证数量。
            request.maximum_items.ok_or_else(|| {
                // 防御性协议错误。
                ObservationWorkerErrorCode::InvalidArgument.error("maximumItems is required.")
            })?,
            // 传入已验证 view。
            request.view.as_deref().ok_or_else(|| {
                // 防御性协议错误。
                ObservationWorkerErrorCode::InvalidArgument.error("view is required.")
            })?,
            // 传入已验证 selector。
            request.selector.as_ref().ok_or_else(|| {
                // 防御性协议错误。
                ObservationWorkerErrorCode::InvalidArgument.error("selector is required.")
            })?,
        );
    }
    // 解析阶段已保证树字段完整。
    let maximum_depth = request.maximum_depth.ok_or_else(|| {
        // 防御性返回协议错误。
        ObservationWorkerErrorCode::InvalidArgument.error("maximumDepth is required.")
    })?;
    // 读取节点上限。
    let maximum_items = request.maximum_items.ok_or_else(|| {
        // 防御性返回协议错误。
        ObservationWorkerErrorCode::InvalidArgument.error("maximumItems is required.")
    })?;
    // 读取 view。
    let view = request.view.ok_or_else(|| {
        // 防御性返回协议错误。
        ObservationWorkerErrorCode::InvalidArgument.error("view is required.")
    })?;
    // 委托有界树观察。
    observe_tree(
        // 传入 worker 私有 HWND。
        hwnd,
        // 传入 canonical 父窗口 ID。
        &request.session_id,
        // 传入深度边界。
        maximum_depth,
        // 传入数量边界。
        maximum_items,
        // 传入 view。
        &view,
    )
}

// 构造 worker 成功 envelope。
fn success_envelope(data: Value) -> Value {
    // 固定输出协议版本。
    json!({
        // 标记成功。
        "ok": true,
        // 输出协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出 operation data。
        "data": data,
    })
}

// 构造 worker 失败 envelope。
fn error_envelope(error: &AppControlError) -> Value {
    // 固定输出协议版本与结构化错误。
    json!({
        // 标记失败。
        "ok": false,
        // 输出协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出公开错误。
        "error": {
            // 输出稳定错误码。
            "code": error.code,
            // 输出安全消息。
            "message": error.message,
        },
    })
}

// 从标准输入执行一次协议请求并向标准输出写一行 JSON。
pub fn run_stdio() -> i32 {
    // 创建请求缓冲区。
    let mut input = String::new();
    // 将读取硬限制为 64KiB 加一个溢出探针字节。
    let read_result = std::io::stdin()
        // 限制输入字节。
        .take(MAXIMUM_REQUEST_BYTES.saturating_add(1))
        // 读取 UTF-8 文本。
        .read_to_string(&mut input);
    // 输入读取或 UTF-8 解码失败返回协议错误。
    let result = match read_result {
        // 超过硬上限时拒绝。
        Ok(_) if u64::try_from(input.len()).unwrap_or(u64::MAX) > MAXIMUM_REQUEST_BYTES => {
            // 构造输出过大错误。
            Err(ObservationWorkerErrorCode::InvalidArgument.error(
                // 不回显请求内容。
                "The observation worker request exceeded 64 KiB.",
            ))
        }
        // 成功读取后解析并执行。
        Ok(_) => parse_request(&input).and_then(execute_request),
        // 读取失败结构化返回。
        Err(_) => Err(ObservationWorkerErrorCode::InvalidArgument.error(
            // 不回显 I/O 细节。
            "The observation worker could not read its request.",
        )),
    };
    // 将结果投影为 worker envelope。
    let (envelope, exit_code) = match result {
        // 成功时返回 data。
        Ok(data) => (success_envelope(data), 0),
        // 失败时返回结构化错误。
        Err(error) => (error_envelope(&error), 2),
    };
    // 序列化单行 JSON。
    let text = match serde_json::to_string(&envelope) {
        // 保存成功文本。
        Ok(text) => text,
        // 极端序列化失败只能以进程码报告。
        Err(_) => return 2,
    };
    // 只向 stdout 写入结果，不使用 stderr。
    if writeln!(std::io::stdout(), "{text}").is_err() {
        // 管道关闭时返回失败码。
        return 2;
    }
    // 返回协议对应退出码。
    exit_code
}

// 验证协议解析、cache request 与隐私边界的纯单元测试。
#[cfg(test)]
// 把测试拆出生产 worker 文件以遵守文件规模边界。
// 声明 worker 私有测试模块。
mod tests;
