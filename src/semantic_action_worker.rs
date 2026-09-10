//! 实现 `act/semantic-action-worker/v1` 的单次写入 companion worker。

// 导入有界队列与标准流接口。
use std::{
    // 导入广度优先队列。
    collections::VecDeque,
    // 导入标准输入输出 trait。
    io::{Read, Write},
};

// 导入严格协议反序列化。
use serde::Deserialize;
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入 Windows COM、UIA 与窗口接口。
use windows::{
    // 导入所需 Win32 命名空间。
    Win32::{
        // 导入权限状态与窗口句柄。
        Foundation::{E_ACCESSDENIED, HWND, RPC_E_CHANGED_MODE},
        // 导入 COM apartment 与对象创建。
        System::Com::{
            // 导入进程内 server 上下文。
            CLSCTX_INPROC_SERVER,
            // 导入多线程 apartment 标志。
            COINIT_MULTITHREADED,
            // 导入 COM 实例创建函数。
            CoCreateInstance,
            // 导入 COM 初始化函数。
            CoInitializeEx,
            // 导入 COM 释放函数。
            CoUninitialize,
        },
        // 导入 UIA client、元素与五种模式。
        UI::Accessibility::{
            // 导入系统 UIA coclass。
            CUIAutomation,
            // 导入 UIA client。
            IUIAutomation,
            // 导入 UIA element。
            IUIAutomationElement,
            // 导入 InvokePattern。
            IUIAutomationInvokePattern,
            // 导入 ScrollPattern。
            IUIAutomationScrollPattern,
            // 导入 SelectionItemPattern。
            IUIAutomationSelectionItemPattern,
            // 导入 TogglePattern。
            IUIAutomationTogglePattern,
            // 导入 ValuePattern。
            IUIAutomationValuePattern,
            // 导入 UIA 相对滚动量类型与常量。
            ScrollAmount,
            ScrollAmount_LargeDecrement,
            ScrollAmount_LargeIncrement,
            ScrollAmount_NoAmount,
            ScrollAmount_SmallDecrement,
            ScrollAmount_SmallIncrement,
            // 导入 element stale HRESULT。
            UIA_E_ELEMENTNOTAVAILABLE,
            // 导入五种模式标识。
            UIA_InvokePatternId,
            UIA_ScrollPatternId,
            UIA_SelectionItemPatternId,
            UIA_TogglePatternId,
            UIA_ValuePatternId,
        },
    },
    // 导入 Windows 核心自动释放字符串。
    core::BSTR,
};

// 把错误集合保留为 Worker 私有类型。
mod error_code;
// 导入 Worker 私有错误类型。
use error_code::SemanticActionWorkerErrorCode;

// 导入共享 selector、动作契约、opaque 解析与统一错误。
use crate::{
    // 导入窗口私有发现与 opaque ID 生成。
    adapters::windows::{enumerate_windows, opaque_window_session_id},
    // 导入共享 Component。
    components::{
        // 导入 provider-neutral selector。
        accessibility_selector::AccessibilitySelector,
        // 导入 opaque 目标原语。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind, OpaqueTargetMatch, match_opaque_target},
        // 导入封闭动作与滚动量。
        semantic_action_contract::{SemanticAction, SemanticScrollAmount},
    },
    // 导入统一结果类型。
    domain::{AppControlError, AppResult},
};

// 固定内部协议版本。
const CONTRACT_VERSION: &str = "act/semantic-action-worker/v1";
// 固定唯一写 operation。
const OPERATION: &str = "semantic-element-action";
// 限制 worker 请求总字节数。
const MAXIMUM_REQUEST_BYTES: u64 = 128 * 1024;
// 限制 worker 内窗口重新发现数量。
const MAXIMUM_WINDOWS: usize = 4_096;

// 表示固定 worker request。
#[derive(Debug, Deserialize)]
// 对齐 JSON camelCase 字段并拒绝协议外输入。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerRequest {
    // 保存协议版本。
    contract_version: String,
    // 保存固定 operation。
    operation: String,
    // 保存 canonical s2:w 目标。
    session_id: String,
    // 保存 provider-neutral selector。
    selector: AccessibilitySelector,
    // 保存五种封闭动作。
    action: SemanticAction,
    // 保存树深度上限。
    maximum_depth: usize,
    // 保存节点数量上限。
    maximum_items: usize,
    // 保存 UIA view。
    view: String,
}

// 保存成功初始化的 COM apartment。
struct ComApartment {
    // 标记当前 worker 是否拥有释放责任。
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
        // 已有不同 apartment 时继续使用既有上下文。
        if status == RPC_E_CHANGED_MODE {
            // 返回不拥有 apartment 的守卫。
            return Ok(Self {
                // 禁止释放外部初始化。
                should_uninitialize: false,
            });
        }
        // 其他 COM 初始化失败结构化分类。
        Err(accessibility_status_error("CoInitializeEx", status))
    }
}

// 成对释放当前 worker 拥有的 COM apartment。
impl Drop for ComApartment {
    // 执行资源回收。
    fn drop(&mut self) {
        // 只释放当前守卫拥有的初始化。
        if self.should_uninitialize {
            // 与成功初始化成对调用。
            unsafe { CoUninitialize() };
        }
    }
}

// 保存树遍历 element 与深度。
struct PendingNode {
    // 保存 COM element 智能指针。
    element: IUIAutomationElement,
    // 保存当前深度。
    depth: usize,
}

// 保存完整搜索得到的唯一 element 与访问计数。
struct UniqueMatch {
    // 保存待执行动作的唯一 element。
    element: IUIAutomationElement,
    // 保存有界搜索实际访问数量。
    visited: usize,
}

// 把 UIA 错误收敛为稳定分类。
fn accessibility_error(operation: &str, error: &windows::core::Error) -> AppControlError {
    // 委托 HRESULT 分类函数。
    accessibility_status_error(operation, error.code())
}

// 把 HRESULT 收敛为权限、stale 或 provider 不可用。
fn accessibility_status_error(
    // 接收安全操作名。
    operation: &str,
    // 接收 provider HRESULT。
    status: windows::core::HRESULT,
) -> AppControlError {
    // 读取 HRESULT 位模式。
    let status_bits = status.0 as u32;
    // 明确权限拒绝保持独立分类。
    let code = if status == E_ACCESSDENIED {
        // 选择权限拒绝。
        SemanticActionWorkerErrorCode::PermissionDenied
    // element provider 明确报告目标过期。
    } else if status_bits == UIA_E_ELEMENTNOTAVAILABLE {
        // 选择 stale 分类。
        SemanticActionWorkerErrorCode::StaleSession
    } else {
        // 其他调用前失败视为 provider 不可用。
        SemanticActionWorkerErrorCode::AccessibilityUnavailable
    };
    // 返回不含原生身份的安全错误。
    code.error(format!("{operation} failed (HRESULT 0x{status_bits:08x})."))
}

// 把写调用后的任何 provider 异常收敛为不可重试结果。
fn outcome_unknown(action: &SemanticAction) -> AppControlError {
    // 返回 D3/R0 保守证据。
    SemanticActionWorkerErrorCode::OutcomeUnknown.with_details(
        // 不公开 HRESULT 或 provider identity。
        "The semantic action provider did not return a trustworthy completion outcome.",
        // 固定不确定结果协议。
        json!({
            // 回显公开动作类别。
            "action": action.as_str(),
            // 明确结果未知。
            "outcome": "unknown",
            // 说明 provider 调用已经开始。
            "dispatchState": "accepted-may-have-occurred",
            // 保守承认外部状态可能已改变。
            "acceptedMayHaveOccurred": true,
            // 禁止自动重试。
            "automaticRetryProhibited": true,
            // 明确没有安全重试证明。
            "retrySafe": false,
            // 明确没有静默指针降级。
            "pointerFallbackUsed": false,
        }),
    )
}

// 严格解析并验证 worker 请求。
fn parse_request(text: &str) -> AppResult<WorkerRequest> {
    // 要求恰好一个 JSON value。
    let request = serde_json::from_str::<WorkerRequest>(text.trim()).map_err(|_| {
        // 不回显潜在敏感 selector 或 Value 文本。
        SemanticActionWorkerErrorCode::InvalidArgument.error(
            // 返回固定协议诊断。
            "The semantic action worker request violates protocol v1.",
        )
    })?;
    // 验证固定协议版本与 operation。
    if request.contract_version != CONTRACT_VERSION || request.operation != OPERATION {
        // 拒绝未知版本或扩展操作。
        return Err(SemanticActionWorkerErrorCode::InvalidArgument.error(
            // 不协商未实现协议。
            "The semantic action worker request violates protocol v1.",
        ));
    }
    // 只允许 canonical s2:w 目标。
    if OpaqueTargetId::parse(&request.session_id).map(OpaqueTargetId::kind)
        != Some(OpaqueTargetKind::Window)
    {
        // 拒绝 native、旧版本和 element snapshot ID。
        return Err(SemanticActionWorkerErrorCode::InvalidArgument.error(
            // 指明只接受窗口目标。
            "The semantic action worker requires a canonical s2:w target.",
        ));
    }
    // selector 必须满足共享精确匹配契约。
    if request.selector.validate().is_err()
        // 搜索边界必须封闭。
        || request.maximum_depth > 20
        // 数量必须保持硬上限。
        || !(1..=4_096).contains(&request.maximum_items)
        // 只允许两种 UIA view。
        || !matches!(request.view.as_str(), "control" | "raw")
    {
        // 返回固定有界协议错误。
        return Err(SemanticActionWorkerErrorCode::InvalidArgument.error(
            // 不回显 selector。
            "The semantic action worker search input is invalid.",
        ));
    }
    // 返回已验证请求。
    Ok(request)
}

// 在 worker 当前快照中重新发现唯一窗口。
fn resolve_window(session_id: &str) -> AppResult<isize> {
    // 枚举当前全部顶层窗口。
    let mut windows = enumerate_windows()?;
    // 只保留公开窗口 surface 的可见有标题候选。
    windows.retain(|window| window.visible && !window.title.is_empty());
    // 限制候选清单。
    windows.truncate(MAXIMUM_WINDOWS);
    // 使用私有事实重新生成 opaque ID 并完整扫描碰撞。
    match match_opaque_target(session_id, &windows, |window| {
        // 生成当前候选 canonical s2:w。
        Some(opaque_window_session_id(window))
    }) {
        // 唯一命中只返回 worker 私有 HWND。
        OpaqueTargetMatch::Unique(window) => Ok(window.hwnd),
        // 零命中表示目标已消失或代际变化。
        OpaqueTargetMatch::Missing => Err(SemanticActionWorkerErrorCode::StaleSession.error(
            // 不公开 native 目标。
            "The opaque application-window session no longer resolves.",
        )),
        // 多命中必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(SemanticActionWorkerErrorCode::AmbiguousTarget.error(
            // 明确不会选择任意候选。
            "The opaque application-window session resolves to multiple windows.",
        )),
    }
}

// 构造系统 UI Automation 对象。
fn create_automation() -> AppResult<IUIAutomation> {
    // 创建进程内 UIA client。
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        // 映射 provider 可用性错误。
        .map_err(|error| accessibility_error("CoCreateInstance(CUIAutomation)", &error))
}

// 把 S_OK/null 解释为树自然结束。
fn optional_element(
    // 接收可能包含 null COM 指针的结果。
    result: windows::core::Result<IUIAutomationElement>,
) -> Result<Option<IUIAutomationElement>, windows::core::Error> {
    // 区分真实 element、自然结束与 provider 错误。
    match result {
        // 非空 element 继续遍历。
        Ok(element) => Ok(Some(element)),
        // COM 成功但返回 null 时 windows-rs 使用 code 0 空错误。
        Err(error) if error.code().is_ok() => Ok(None),
        // 真实失败交给调用方。
        Err(error) => Err(error),
    }
}

// 读取一个字符串属性；普通 provider 缺口返回 None。
fn string_property(
    // 接收延迟 UIA 读取。
    read: impl FnOnce() -> windows::core::Result<windows::core::BSTR>,
    // 接收安全操作名。
    operation: &str,
) -> AppResult<Option<String>> {
    // 执行单个只读属性调用。
    match read() {
        // 成功时转换为 Rust 字符串。
        Ok(value) => Ok(Some(value.to_string())),
        // 权限和 stale 必须传播。
        Err(error)
            if error.code() == E_ACCESSDENIED
                // 同时核对 element stale。
                || error.code().0 as u32 == UIA_E_ELEMENTNOTAVAILABLE =>
        {
            // 返回封闭错误。
            Err(accessibility_error(operation, &error))
        }
        // 其他属性缺口使搜索不完整。
        Err(_) => Ok(None),
    }
}

// 读取 selector 允许的五项属性。
fn semantic_node(element: &IUIAutomationElement) -> AppResult<Option<Value>> {
    // 读取公开名称。
    let Some(name) = string_property(
        // 延迟读取名称。
        || unsafe { element.CurrentName() },
        // 使用安全操作名。
        "IUIAutomationElement::CurrentName",
    )?
    else {
        // 普通 provider 缺口不能证明匹配。
        return Ok(None);
    };
    // 读取 automation ID。
    let Some(automation_id) = string_property(
        // 延迟读取 automation ID。
        || unsafe { element.CurrentAutomationId() },
        // 使用安全操作名。
        "IUIAutomationElement::CurrentAutomationId",
    )?
    else {
        // 缺口使本节点不完整。
        return Ok(None);
    };
    // 读取类名。
    let Some(class_name) = string_property(
        // 延迟读取类名。
        || unsafe { element.CurrentClassName() },
        // 使用安全操作名。
        "IUIAutomationElement::CurrentClassName",
    )?
    else {
        // 缺口使本节点不完整。
        return Ok(None);
    };
    // 读取 framework ID。
    let Some(framework_id) = string_property(
        // 延迟读取 framework ID。
        || unsafe { element.CurrentFrameworkId() },
        // 使用安全操作名。
        "IUIAutomationElement::CurrentFrameworkId",
    )?
    else {
        // 缺口使本节点不完整。
        return Ok(None);
    };
    // 读取 control type。
    let control_type = match unsafe { element.CurrentControlType() } {
        // 成功时保留公共数值。
        Ok(value) => value.0,
        // 权限和 stale 必须传播。
        Err(error)
            if error.code() == E_ACCESSDENIED
                // 同时核对 stale。
                || error.code().0 as u32 == UIA_E_ELEMENTNOTAVAILABLE =>
        {
            // 返回封闭错误。
            return Err(accessibility_error(
                // 使用安全操作名。
                "IUIAutomationElement::CurrentControlType",
                // 借用 provider 错误。
                &error,
            ));
        }
        // 其他缺口不能证明 selector 结论。
        Err(_) => return Ok(None),
    };
    // 返回仅含 selector 字段的临时节点。
    Ok(Some(json!({
        // 保存名称。
        "name": name,
        // 保存 automation ID。
        "automationId": automation_id,
        // 保存类名。
        "className": class_name,
        // 保存 framework ID。
        "frameworkId": framework_id,
        // 保存 control type。
        "controlType": control_type,
    })))
}

// 执行有界完整搜索并返回唯一 element。
fn find_unique_element(
    // 接收 UIA client。
    automation: &IUIAutomation,
    // 接收 worker 私有 HWND。
    hwnd: isize,
    // 接收已验证请求。
    request: &WorkerRequest,
) -> AppResult<UniqueMatch> {
    // 从 worker 私有 HWND 获得 root element。
    let root = unsafe {
        // 原生句柄只停留在 worker 内。
        automation.ElementFromHandle(HWND(hwnd as *mut std::ffi::c_void))
    }
    // 映射 stale、permission 或 provider 缺口。
    .map_err(|error| accessibility_error("IUIAutomation::ElementFromHandle", &error))?;
    // 按请求选择 UIA walker。
    let walker = unsafe {
        // raw 与 control 使用封闭分支。
        if request.view == "raw" {
            // 请求 RawView walker。
            automation.RawViewWalker()
        } else {
            // 请求 ControlView walker。
            automation.ControlViewWalker()
        }
    }
    // 映射 walker provider 缺口。
    .map_err(|error| accessibility_error("IUIAutomation::TreeWalker", &error))?;
    // 从 root 和深度零开始 BFS。
    let mut pending = VecDeque::from([PendingNode {
        // 保存 root element。
        element: root,
        // root 深度为零。
        depth: 0,
    }]);
    // 保存实际访问节点数。
    let mut visited = 0_usize;
    // 保存属性与遍历完整性。
    let mut complete = true;
    // 最多保存两个匹配证明歧义。
    let mut matches = Vec::with_capacity(2);
    // 逐项消费有界队列。
    while let Some(current) = pending.pop_front() {
        // 达到数量上限时恢复节点并退出。
        if visited >= request.maximum_items {
            // 恢复未处理节点。
            pending.push_front(current);
            // 标记搜索不完整。
            complete = false;
            // 退出遍历。
            break;
        }
        // 记录当前节点。
        visited = visited.saturating_add(1);
        // 读取完整语义属性。
        match semantic_node(&current.element)? {
            // 完整节点才可证明 selector 命中。
            Some(node) if request.selector.matches_node(&node) => {
                // 保存最多两个候选。
                matches.push(current.element.clone());
                // 两个匹配已经证明歧义。
                if matches.len() == 2 {
                    // 返回结构化歧义且不选择候选。
                    return Err(SemanticActionWorkerErrorCode::AmbiguousTarget.error(
                        // 不回显 selector。
                        "The element selector matched more than one accessibility element.",
                    ));
                }
            }
            // 完整但不匹配时继续。
            Some(_) => {}
            // 属性缺口使零或唯一结论不完整。
            None => complete = false,
        }
        // 深度边界到达时只探测是否仍有子节点。
        if current.depth >= request.maximum_depth {
            // 探测首个子节点。
            match optional_element(unsafe { walker.GetFirstChildElement(&current.element) }) {
                // 存在后代表示深度截断。
                Ok(Some(_)) => complete = false,
                // 无后代保持当前完整性。
                Ok(None) => {}
                // provider 缺口使搜索不完整。
                Err(_) => complete = false,
            }
            // 继续下一个待处理节点。
            continue;
        }
        // 读取首个子节点。
        let mut child = match optional_element(unsafe {
            // 使用当前 view walker。
            walker.GetFirstChildElement(&current.element)
        }) {
            // 保存自然可选 child。
            Ok(child) => child,
            // 失败时标记不完整。
            Err(_) => {
                // 记录 provider 缺口。
                complete = false;
                // 停止当前 child 链。
                None
            }
        };
        // 按 sibling 顺序加入 BFS 队列。
        while let Some(current_child) = child {
            // 队列与已访问数量共同受上限约束。
            if visited.saturating_add(pending.len()) >= request.maximum_items {
                // 尚有 child 未读取，结论不完整。
                complete = false;
                // 停止当前 sibling 链。
                break;
            }
            // 在移动 child 前读取下一个 sibling。
            let next = optional_element(unsafe {
                // 使用当前 view walker。
                walker.GetNextSiblingElement(&current_child)
            });
            // 把当前 child 加入队列。
            pending.push_back(PendingNode {
                // 移交 COM 智能指针。
                element: current_child,
                // 子节点深度加一。
                depth: current.depth.saturating_add(1),
            });
            // 解包下一个 sibling。
            child = match next {
                // 保存自然可选 sibling。
                Ok(next) => next,
                // provider 错误使搜索不完整。
                Err(_) => {
                    // 标记不完整。
                    complete = false;
                    // 停止 sibling 遍历。
                    None
                }
            };
        }
    }
    // 任何队列残留都表示搜索截断。
    complete &= pending.is_empty();
    // 零或唯一结论必须由完整搜索证明。
    if !complete {
        // 返回显式搜索不完整。
        return Err(SemanticActionWorkerErrorCode::SearchIncomplete.error(
            // 不猜测候选。
            "The bounded accessibility search could not prove a unique semantic action target.",
        ));
    }
    // 取出唯一候选或返回元素缺失。
    let element = matches.pop().ok_or_else(|| {
        // 完整零命中是稳定缺失结果。
        SemanticActionWorkerErrorCode::ElementNotFound.error(
            // 不回显 selector。
            "The element selector matched no accessibility element.",
        )
    })?;
    // 返回唯一 element 与有界搜索证据。
    Ok(UniqueMatch { element, visited })
}

// 把 provider-neutral 相对滚动量映射为 UIA 私有值。
const fn native_scroll_amount(amount: SemanticScrollAmount) -> ScrollAmount {
    // 穷举封闭输入。
    match amount {
        // 映射不滚动。
        SemanticScrollAmount::NoAmount => ScrollAmount_NoAmount,
        // 映射大幅负向滚动。
        SemanticScrollAmount::LargeDecrement => ScrollAmount_LargeDecrement,
        // 映射小幅负向滚动。
        SemanticScrollAmount::SmallDecrement => ScrollAmount_SmallDecrement,
        // 映射大幅正向滚动。
        SemanticScrollAmount::LargeIncrement => ScrollAmount_LargeIncrement,
        // 映射小幅正向滚动。
        SemanticScrollAmount::SmallIncrement => ScrollAmount_SmallIncrement,
    }
}

// 获取一个动作模式；失败发生在写调用前。
fn pattern<T: windows::core::Interface>(
    // 接收唯一 element。
    element: &IUIAutomationElement,
    // 接收 UIA 模式标识。
    id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
) -> AppResult<T> {
    // 请求当前模式接口。
    unsafe { element.GetCurrentPatternAs::<T>(id) }.map_err(|error| {
        // 权限和 stale 保持独立分类。
        if error.code() == E_ACCESSDENIED || error.code().0 as u32 == UIA_E_ELEMENTNOTAVAILABLE {
            // 返回稳定权限或 stale 错误。
            return accessibility_error("IUIAutomationElement::GetCurrentPatternAs", &error);
        }
        // 其他模式获取失败是显式动作缺口。
        SemanticActionWorkerErrorCode::ActionUnsupported.error(
            // 不泄漏 provider 或 COM 类型。
            "The unique accessibility element does not support the requested semantic action.",
        )
    })
}

// 对唯一元素执行恰好一次动作调用。
fn dispatch(element: &IUIAutomationElement, action: &SemanticAction) -> AppResult<()> {
    // 在获取模式前核对元素当前启用状态。
    let enabled = unsafe { element.CurrentIsEnabled() }
        // 映射调用前 provider 错误。
        .map_err(|error| accessibility_error("IUIAutomationElement::CurrentIsEnabled", &error))?
        // 转换为 Rust bool。
        .as_bool();
    // 未启用元素不得尝试写调用。
    if !enabled {
        // 返回可安全修改 Workflow 的显式状态。
        return Err(SemanticActionWorkerErrorCode::ElementNotEnabled.error(
            // 不触发指针降级。
            "The unique accessibility element is not enabled.",
        ));
    }
    // 按五种封闭动作获取并调用恰好一个模式。
    match action {
        // 调用默认动作。
        SemanticAction::Invoke {} => {
            // 获取 InvokePattern。
            let pattern = pattern::<IUIAutomationInvokePattern>(element, UIA_InvokePatternId)?;
            // 调用后任何异常均为不确定结果。
            unsafe { pattern.Invoke() }.map_err(|_| outcome_unknown(action))?;
        }
        // 设置 ValuePattern 文本。
        SemanticAction::Value { .. } => {
            // 获取 ValuePattern。
            let pattern = pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)?;
            // 在写调用前核对只读状态。
            let read_only = unsafe { pattern.CurrentIsReadOnly() }
                // 映射调用前 provider 错误。
                .map_err(|error| {
                    accessibility_error("IUIAutomationValuePattern::CurrentIsReadOnly", &error)
                })?
                // 转换为 Rust bool。
                .as_bool();
            // 只读 ValuePattern 是显式动作缺口。
            if read_only {
                // 不尝试 SetValue。
                return Err(SemanticActionWorkerErrorCode::ActionUnsupported.error(
                    // 使用 provider-neutral 诊断。
                    "The unique accessibility element exposes a read-only value.",
                ));
            }
            // 读取共享契约已验证的文本。
            let value = action.value().ok_or_else(|| {
                // 防御性返回协议错误。
                SemanticActionWorkerErrorCode::InvalidArgument.error(
                    // 不回显 Value 文本。
                    "The semantic value action is missing its value.",
                )
            })?;
            // 编码为 UIA ValuePattern 所需的自动释放字符串。
            let encoded = BSTR::from(value);
            // 调用后任何异常均为不确定结果。
            unsafe { pattern.SetValue(&encoded) }
                // 禁止根据 HRESULT 自动重试。
                .map_err(|_| outcome_unknown(action))?;
        }
        // 切换元素状态。
        SemanticAction::Toggle {} => {
            // 获取 TogglePattern。
            let pattern = pattern::<IUIAutomationTogglePattern>(element, UIA_TogglePatternId)?;
            // 调用后任何异常均为不确定结果。
            unsafe { pattern.Toggle() }.map_err(|_| outcome_unknown(action))?;
        }
        // 选择元素。
        SemanticAction::Select {} => {
            // 获取 SelectionItemPattern。
            let pattern = pattern::<IUIAutomationSelectionItemPattern>(
                // 传入唯一元素。
                element,
                // 传入模式标识。
                UIA_SelectionItemPatternId,
            )?;
            // 调用后任何异常均为不确定结果。
            unsafe { pattern.Select() }.map_err(|_| outcome_unknown(action))?;
        }
        // 相对滚动容器。
        SemanticAction::Scroll { .. } => {
            // 获取 ScrollPattern。
            let pattern = pattern::<IUIAutomationScrollPattern>(element, UIA_ScrollPatternId)?;
            // 读取共享契约已验证的两个轴。
            let (horizontal, vertical) = action.scroll_amounts().ok_or_else(|| {
                // 防御性返回协议错误。
                SemanticActionWorkerErrorCode::InvalidArgument.error(
                    // 不回显输入。
                    "The semantic scroll action is missing its relative amounts.",
                )
            })?;
            // 调用后任何异常均为不确定结果。
            unsafe {
                // 只映射 provider-neutral 封闭量。
                pattern.Scroll(
                    // 映射水平量。
                    native_scroll_amount(horizontal),
                    // 映射垂直量。
                    native_scroll_amount(vertical),
                )
            }
            // 禁止自动重试可能已接受的滚动。
            .map_err(|_| outcome_unknown(action))?;
        }
    }
    // 一个模式调用明确返回成功。
    Ok(())
}

// 执行已验证 worker 请求。
fn execute_request(request: WorkerRequest) -> AppResult<Value> {
    // 初始化 worker 私有 COM apartment。
    let _apartment = ComApartment::initialize()?;
    // 每次使用时重新解析 opaque 窗口。
    let hwnd = resolve_window(&request.session_id)?;
    // 创建 UI Automation client。
    let automation = create_automation()?;
    // 完整搜索并取得唯一 element。
    let unique = find_unique_element(&automation, hwnd, &request)?;
    // 对唯一 element 执行恰好一次动作。
    dispatch(&unique.element, &request.action)?;
    // 返回完成证据且不读取动作后状态。
    Ok(json!({
        // 回显公开动作类别。
        "action": request.action.as_str(),
        // 明确动作调用已完成。
        "outcome": "completed",
        // 明确 dispatch 已返回成功。
        "dispatchState": "completed",
        // mutation 已被 provider 接受。
        "acceptedMayHaveOccurred": true,
        // mutation 永不自动重试。
        "automaticRetryProhibited": true,
        // 没有幂等重试证明。
        "retrySafe": false,
        // 窗口在 worker 内重新解析。
        "windowReResolved": true,
        // element 从原 selector 重新解析。
        "elementReResolved": true,
        // selector 使用精确 AND 语义。
        "selectorSemantics": "exact-and",
        // 输出有界搜索数量。
        "visited": unique.visited,
        // 明确没有静默指针降级。
        "pointerFallbackUsed": false,
        // 明确执行了一个写调用。
        "writePerformed": true,
    }))
}

// 构造 worker 成功 envelope。
fn success_envelope(data: Value) -> Value {
    // 固定输出协议版本。
    json!({
        // 标记成功。
        "ok": true,
        // 输出协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出安全 data。
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
        // 输出稳定错误。
        "error": {
            // 输出错误码。
            "code": error.code,
            // 输出安全消息。
            "message": error.message,
            // 输出 provider-neutral 证据。
            "details": error.details,
        },
    })
}

// 从标准输入执行一次请求并写出单行 JSON。
pub fn run_stdio() -> i32 {
    // 创建请求缓冲区。
    let mut input = String::new();
    // 将读取硬限制为 128KiB 加一个探针字节。
    let read_result = std::io::stdin()
        // 限制输入字节。
        .take(MAXIMUM_REQUEST_BYTES.saturating_add(1))
        // 读取 UTF-8 文本。
        .read_to_string(&mut input);
    // 解析并执行或构造输入错误。
    let result = match read_result {
        // 超过硬上限时拒绝。
        Ok(_) if u64::try_from(input.len()).unwrap_or(u64::MAX) > MAXIMUM_REQUEST_BYTES => {
            // 返回不含输入的参数错误。
            Err(SemanticActionWorkerErrorCode::InvalidArgument.error(
                // 说明固定上限。
                "The semantic action worker request exceeded 128 KiB.",
            ))
        }
        // 成功读取后解析并执行。
        Ok(_) => parse_request(&input).and_then(execute_request),
        // 读取失败结构化返回。
        Err(_) => Err(SemanticActionWorkerErrorCode::InvalidArgument.error(
            // 不回显 I/O 细节。
            "The semantic action worker could not read its request.",
        )),
    };
    // 投影为 worker envelope 与退出码。
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
        // 极端序列化失败只以进程码报告。
        Err(_) => return 2,
    };
    // stdout 只允许结果 JSON。
    if writeln!(std::io::stdout(), "{text}").is_err() {
        // 管道关闭时返回失败。
        return 2;
    }
    // 返回协议对应退出码。
    exit_code
}

// 把纯协议与映射测试拆出生产文件。
#[cfg(test)]
mod tests;
