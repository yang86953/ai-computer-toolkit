//! 实现 observation worker 私有的有界语义定位与命中区域读取。

// 导入有界广度优先队列。
use std::collections::VecDeque;

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入只读 UIA、DPI 与窗口状态接口。
use windows::Win32::{
    // 导入物理屏幕点和矩形。
    Foundation::{E_ACCESSDENIED, HWND, POINT, RECT},
    // 导入 UIA cache、element 与属性标识。
    UI::Accessibility::{
        // 保留完整 element 供有界树遍历与唯一候选读取。
        AutomationElementMode_Full,
        // 导入 UIA client 与 cache request 接口。
        IUIAutomation,
        IUIAutomationCacheRequest,
        IUIAutomationElement,
        // cache 只覆盖单个 element。
        TreeScope_Element,
        // 导入定位允许读取的八项属性标识。
        UIA_AutomationIdPropertyId,
        UIA_BoundingRectanglePropertyId,
        UIA_ClassNamePropertyId,
        UIA_ControlTypePropertyId,
        UIA_E_ELEMENTNOTAVAILABLE,
        UIA_E_NOCLICKABLEPOINT,
        UIA_FrameworkIdPropertyId,
        UIA_IsEnabledPropertyId,
        UIA_IsOffscreenPropertyId,
        UIA_NamePropertyId,
    },
    // 导入线程级 Per-Monitor-V2 DPI 上下文。
    UI::HiDpi::{
        // 导入上下文设置函数与强类型句柄。
        DPI_AWARENESS_CONTEXT,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        SetThreadDpiAwarenessContext,
    },
    // 导入只读最小化状态查询。
    UI::WindowsAndMessaging::IsIconic,
};

// 导入共享 selector、统一结果与父 worker 原语。
use crate::{
    // 导入 provider-neutral selector。
    components::accessibility_selector::AccessibilitySelector,
    // 导入统一结果类型。
    domain::{AppControlError, AppResult},
};

// 导入父 worker 已拥有的 COM、错误与节点投影实现。
use super::{
    // 导入 COM apartment 守卫。
    ComApartment,
    // 导入 worker 私有错误类型。
    ObservationWorkerErrorCode,
    // 导入有界遍历节点。
    PendingNode,
    // 导入 UIA 错误收敛函数。
    accessibility_error,
    // 导入 UIA client 构造器。
    create_automation,
    // 导入共享节点投影。
    node_json,
};

// 保存定位期间替换的线程 DPI 上下文。
pub(super) struct DpiAwarenessGuard {
    // 保存进入前上下文供 Drop 恢复。
    previous: DPI_AWARENESS_CONTEXT,
}

// 提供定位专用 DPI 生命周期。
impl DpiAwarenessGuard {
    // 把当前 worker 线程切换到 Per-Monitor-V2。
    pub(super) fn enter() -> AppResult<Self> {
        // 设置 UIA 物理坐标所需的线程上下文。
        let previous = unsafe {
            // 只影响当前隔离 worker 线程。
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        };
        // 空或无效旧上下文表示切换失败。
        if previous.is_invalid() {
            // 返回结构化 provider 可用性错误。
            return Err(ObservationWorkerErrorCode::AccessibilityUnavailable.error(
                // 不公开平台 last-error。
                "The observation worker could not establish Per-Monitor-V2 coordinate semantics.",
            ));
        }
        // 返回拥有恢复责任的守卫。
        Ok(Self { previous })
    }
}

// 恢复进入定位前的线程 DPI 上下文。
impl Drop for DpiAwarenessGuard {
    // 执行成对恢复。
    fn drop(&mut self) {
        // 恢复失败不能在 Drop 中伪造成功或 panic。
        let _ = unsafe {
            // 只恢复当前 worker 线程。
            SetThreadDpiAwarenessContext(self.previous)
        };
    }
}

// 保存唯一匹配的快照节点与 UIA element。
struct LocatedMatch {
    // 保存 UIA element 供 bounds 与 clickable point 读取。
    element: IUIAutomationElement,
    // 保存 provider-neutral 节点投影。
    node: Value,
}

// 创建只覆盖定位允许属性的 UIA cache request。
fn create_location_cache_request(
    // 接收 worker 私有 UIA client。
    automation: &IUIAutomation,
) -> AppResult<IUIAutomationCacheRequest> {
    // 创建独立 cache request。
    let cache_request = unsafe { automation.CreateCacheRequest() }
        // 映射 cache request 创建失败。
        .map_err(|error| accessibility_error("IUIAutomation::CreateCacheRequest", &error))?;
    // 只缓存当前 element，避免 provider 无界预取。
    unsafe { cache_request.SetTreeScope(TreeScope_Element) }
        // 映射 scope 配置失败。
        .map_err(|error| accessibility_error("IUIAutomationCacheRequest::SetTreeScope", &error))?;
    // 保留完整 element 供有界 walker 和唯一候选读取。
    unsafe { cache_request.SetAutomationElementMode(AutomationElementMode_Full) }
        // 映射 element mode 配置失败。
        .map_err(|error| {
            // 返回统一结构化错误。
            accessibility_error(
                "IUIAutomationCacheRequest::SetAutomationElementMode",
                &error,
            )
        })?;
    // 固定定位允许读取的属性集合。
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
        // 仅定位 operation 额外缓存物理屏幕矩形。
        UIA_BoundingRectanglePropertyId,
    ];
    // 逐项加入同一个 cache request。
    for property in properties {
        // 配置允许属性。
        unsafe { cache_request.AddProperty(property) }
            // 映射属性配置失败。
            .map_err(|error| {
                // 不公开原生属性标识。
                accessibility_error("IUIAutomationCacheRequest::AddProperty", &error)
            })?;
    }
    // 返回定位专用 cache request。
    Ok(cache_request)
}

// 把 windows-rs 的 S_OK/null 解释为树自然结束。
fn optional_element(
    // 接收可能包含 null COM 指针的结果。
    result: windows::core::Result<IUIAutomationElement>,
) -> Result<Option<IUIAutomationElement>, windows::core::Error> {
    // 区分实际 element、自然结束和真实 provider 错误。
    match result {
        // 非空 element 继续遍历。
        Ok(element) => Ok(Some(element)),
        // COM 成功但返回 null 时 windows-rs 使用 code 0 的空错误。
        Err(error) if error.code().is_ok() => Ok(None),
        // 真实失败交给调用方分类。
        Err(error) => Err(error),
    }
}

// 判断位置属性错误是否必须保持权限或 stale 分类。
fn mandatory_location_error(
    // 接收 provider 错误。
    error: &windows::core::Error,
    // 接收安全操作名。
    operation: &str,
) -> Option<AppControlError> {
    // 读取 HRESULT 位模式。
    let status = error.code();
    // 权限和 element 过期不得降级为普通不可用。
    (status == E_ACCESSDENIED || status.0 as u32 == UIA_E_ELEMENTNOTAVAILABLE)
        // 复用 worker 封闭分类。
        .then(|| accessibility_error(operation, error))
}

// 从唯一 element 读取物理屏幕矩形。
fn bounds(
    // 接收唯一 UIA element。
    element: &IUIAutomationElement,
) -> AppResult<(Value, Option<RECT>)> {
    // 从定位专用缓存读取矩形。
    let rectangle = match unsafe { element.CachedBoundingRectangle() } {
        // 成功时保留矩形。
        Ok(rectangle) => rectangle,
        // 权限或 stale 必须传播。
        Err(error)
            if mandatory_location_error(
                // 传入原始 provider 错误。
                &error,
                // 使用安全操作名。
                "IUIAutomationElement::CachedBoundingRectangle",
            )
            .is_some() =>
        {
            // 提取已知存在的结构化错误。
            return Err(mandatory_location_error(
                // 再次借用同一错误，不执行 provider 调用。
                &error,
                // 保持相同操作名。
                "IUIAutomationElement::CachedBoundingRectangle",
            )
            // guard 已证明存在。
            .unwrap_or_else(|| unreachable!("mandatory location error must exist")));
        }
        // 其他 provider 缺口作为显式 bounds 不可用返回。
        Err(_) => {
            // 不公开 HRESULT。
            return Ok((
                // 返回封闭不可用形状。
                json!({
                    // 标记矩形不可用。
                    "state": "unavailable",
                    // 使用 provider-neutral 原因。
                    "reason": "provider-did-not-return-bounds",
                }),
                // 不保留原生矩形。
                None,
            ));
        }
    };
    // 使用 i64 防止极端虚拟桌面差值溢出。
    let width = i64::from(rectangle.right) - i64::from(rectangle.left);
    // 计算有符号高度。
    let height = i64::from(rectangle.bottom) - i64::from(rectangle.top);
    // 空或反向矩形不可命中。
    if width <= 0 || height <= 0 {
        // 返回显式空矩形状态。
        return Ok((
            // 不公开无效坐标值。
            json!({
                // 标记矩形不可用。
                "state": "unavailable",
                // 说明几何为空。
                "reason": "empty-bounds",
            }),
            // 不向命中点验证传递无效矩形。
            None,
        ));
    }
    // 返回 provider-neutral 物理屏幕矩形。
    Ok((
        // 输出稳定坐标结构。
        json!({
            // 标记矩形可用。
            "state": "available",
            // 使用虚拟桌面物理像素坐标。
            "coordinateSpace": "virtual-desktop",
            // 明确数值单位。
            "unit": "physical-screen-px",
            // 输出左边界。
            "left": rectangle.left,
            // 输出上边界。
            "top": rectangle.top,
            // 输出右边界。
            "right": rectangle.right,
            // 输出下边界。
            "bottom": rectangle.bottom,
            // 输出无溢出宽度。
            "width": width,
            // 输出无溢出高度。
            "height": height,
        }),
        // 保留原生矩形仅供 worker 内点验证。
        Some(rectangle),
    ))
}

// 判断物理屏幕点是否落在半开矩形中。
fn point_inside(point: POINT, bounds: RECT) -> bool {
    // 同时验证水平与垂直范围。
    point.x >= bounds.left
        // 右边界使用半开区间。
        && point.x < bounds.right
        // 验证上边界。
        && point.y >= bounds.top
        // 下边界使用半开区间。
        && point.y < bounds.bottom
}

// 从唯一 element 读取 provider 给出的可点击点。
fn hit_region(
    // 接收唯一 UIA element。
    element: &IUIAutomationElement,
    // 接收可选有效 bounds。
    bounds: Option<RECT>,
    // 接收公开 offscreen 状态。
    offscreen: bool,
    // 接收父窗口最小化状态。
    minimized: bool,
) -> AppResult<Value> {
    // 最小化窗口不发布当前桌面的点击候选。
    if minimized {
        // 返回显式最小化原因。
        return Ok(json!({
            // 标记命中区域不可用。
            "state": "unavailable",
            // 说明父窗口最小化。
            "reason": "window-minimized",
            // 不猜测遮挡状态。
            "occlusion": "unknown",
        }));
    }
    // UIA 明确 offscreen 时不发布点击候选。
    if offscreen {
        // 返回显式 offscreen 原因。
        return Ok(json!({
            // 标记命中区域不可用。
            "state": "unavailable",
            // 说明 element 不在屏幕上。
            "reason": "element-offscreen",
            // 不猜测遮挡状态。
            "occlusion": "unknown",
        }));
    }
    // 缺少有效 bounds 时不能验证 provider point 所属区域。
    let Some(bounds) = bounds else {
        // 返回显式 bounds 缺口。
        return Ok(json!({
            // 标记命中区域不可用。
            "state": "unavailable",
            // 说明缺少几何验证。
            "reason": "bounds-unavailable",
            // 不猜测遮挡状态。
            "occlusion": "unknown",
        }));
    };
    // 初始化 provider 输出点。
    let mut point = POINT::default();
    // 请求 UIA provider 给出实际可点击点。
    let got_clickable = match unsafe { element.GetClickablePoint(&mut point) } {
        // 保存 provider 成功布尔值。
        Ok(value) => value.as_bool(),
        // 无可点击点是正常的显式不可用状态。
        Err(error) if error.code().0 as u32 == UIA_E_NOCLICKABLEPOINT => false,
        // 权限或 stale 必须传播。
        Err(error)
            if mandatory_location_error(
                // 传入 provider 错误。
                &error,
                // 使用安全操作名。
                "IUIAutomationElement::GetClickablePoint",
            )
            .is_some() =>
        {
            // 返回已分类错误。
            return Err(mandatory_location_error(
                // 复用同一错误。
                &error,
                // 保持相同操作名。
                "IUIAutomationElement::GetClickablePoint",
            )
            // guard 已证明存在。
            .unwrap_or_else(|| unreachable!("mandatory location error must exist")));
        }
        // 其他 provider 缺口不伪造点击候选。
        Err(_) => {
            // 返回显式 provider 不可用。
            return Ok(json!({
                // 标记命中区域不可用。
                "state": "unavailable",
                // 说明 provider 未提供候选。
                "reason": "provider-did-not-return-clickable-point",
                // 不猜测遮挡状态。
                "occlusion": "unknown",
            }));
        }
    };
    // provider 明确没有可点击点。
    if !got_clickable {
        // 返回封闭不可用状态。
        return Ok(json!({
            // 标记命中区域不可用。
            "state": "unavailable",
            // 使用稳定原因。
            "reason": "no-clickable-point",
            // 该结果不能区分不可交互与遮挡。
            "occlusion": "unknown",
        }));
    }
    // provider 点必须位于同一快照矩形内。
    if !point_inside(point, bounds) {
        // 拒绝跨元素或漂移点。
        return Ok(json!({
            // 标记命中区域不可用。
            "state": "unavailable",
            // 说明 provider 点未通过几何核对。
            "reason": "provider-point-outside-bounds",
            // 不猜测遮挡状态。
            "occlusion": "unknown",
        }));
    }
    // 返回 provider 认证的物理屏幕点候选。
    Ok(json!({
        // 标记命中区域可用。
        "state": "available",
        // 明确来源不是矩形中心猜测。
        "source": "uia-clickable-point",
        // 使用虚拟桌面物理像素坐标。
        "coordinateSpace": "virtual-desktop",
        // 明确坐标单位。
        "unit": "physical-screen-px",
        // 输出横坐标。
        "x": point.x,
        // 输出纵坐标。
        "y": point.y,
        // 只说明 provider 给出了可点击点，不扩大为遮挡证明。
        "occlusion": "provider-clickable-point-available",
    }))
}

// 为唯一匹配补充 bounds 与命中区域。
fn project_unique_match(
    // 接收唯一匹配。
    located: LocatedMatch,
    // 接收窗口最小化状态。
    minimized: bool,
) -> AppResult<Value> {
    // 读取 provider-neutral offscreen 状态。
    let offscreen = located
        // 访问节点字段。
        .node
        // 读取固定状态。
        .get("offscreen")
        // 要求布尔值。
        .and_then(Value::as_bool)
        // node_json 已保证字段，防御性缺失视为 offscreen。
        .unwrap_or(true);
    // 读取物理矩形与 worker 私有验证值。
    let (public_bounds, native_bounds) = bounds(&located.element)?;
    // 读取 provider clickable point。
    let hit_region = hit_region(&located.element, native_bounds, offscreen, minimized)?;
    // 取得可修改节点对象。
    let mut node = located.node.as_object().cloned().ok_or_else(|| {
        // 返回内部协议失败。
        ObservationWorkerErrorCode::AccessibilityUnavailable.error(
            // 不公开节点内容。
            "The observation worker could not project the unique accessibility element.",
        )
    })?;
    // 加入公开 bounds 状态。
    node.insert("bounds".to_owned(), public_bounds);
    // 加入公开命中区域状态。
    node.insert("hitRegion".to_owned(), hit_region);
    // 返回扩展后的唯一节点。
    Ok(Value::Object(node))
}

// 执行有界语义定位。
pub(super) fn observe(
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
    // 接收已验证 selector。
    selector: &AccessibilitySelector,
) -> AppResult<Value> {
    // 初始化 worker 私有 COM apartment。
    let _apartment = ComApartment::initialize()?;
    // 创建 UI Automation client。
    let automation = create_automation()?;
    // 创建定位专用 cache request。
    let cache_request = create_location_cache_request(&automation)?;
    // 从 worker 内部 HWND 获得带定位属性快照的 root。
    let root = unsafe {
        // 在发现 root 时一次构建缓存。
        automation.ElementFromHandleBuildCache(
            // 传入 worker 私有 HWND。
            HWND(hwnd as *mut std::ffi::c_void),
            // 传入定位 cache request。
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
    // 保存实际访问节点数。
    let mut visited = 0_usize;
    // 保存是否遇到属性缺失。
    let mut properties_complete = true;
    // 保存遍历是否完整覆盖请求范围。
    let mut traversal_complete = true;
    // 保存最多两个匹配以证明歧义。
    let mut matches = Vec::with_capacity(2);
    // 只在队列非空且未达到数量上限时继续。
    while let Some(current) = pending.pop_front() {
        // 数量上限到达时把当前节点放回。
        if visited >= maximum_items {
            // 恢复尚未处理节点。
            pending.push_front(current);
            // 退出有界遍历。
            break;
        }
        // 记录当前节点已访问。
        visited = visited.saturating_add(1);
        // 投影共享语义属性。
        let node = node_json(
            // 传入 element。
            &current.element,
            // 传入深度。
            current.depth,
            // 传入父窗口 opaque ID。
            session_id,
        )?;
        // 读取本节点属性完整性。
        let node_complete = node
            // 访问固定字段。
            .get("propertyReadComplete")
            // 要求布尔值。
            .and_then(Value::as_bool)
            // 缺失视为不完整。
            == Some(true);
        // 汇总完整性。
        properties_complete &= node_complete;
        // 只使用完整节点证明 selector 命中。
        if node_complete && selector.matches_node(&node) {
            // 最多保存两个匹配即可证明歧义。
            if matches.len() < 2 {
                // 保存 element 与 provider-neutral 节点。
                matches.push(LocatedMatch {
                    // 克隆轻量 COM 智能指针。
                    element: current.element.clone(),
                    // 移交节点投影。
                    node,
                });
            }
            // 两个完整匹配已经证明歧义，无需继续读取 provider 树。
            if matches.len() == 2 {
                // 提前退出并由主 Module 返回 AMBIGUOUS_TARGET。
                break;
            }
        }
        // 深度上限到达时只探测是否仍有子节点。
        if current.depth >= maximum_depth {
            // 探测首个子节点以证明深度是否截断。
            match optional_element(unsafe {
                // 使用同一 cache request，不读取整棵后代树。
                walker.GetFirstChildElementBuildCache(&current.element, &cache_request)
            }) {
                // 存在子节点表示深度边界截断。
                Ok(Some(_)) => traversal_complete = false,
                // 没有子节点表示当前分支完整。
                Ok(None) => {}
                // provider 错误表示遍历不完整。
                Err(_) => traversal_complete = false,
            }
            // 继续下一个待处理节点。
            continue;
        }
        // 读取首个带属性快照的子节点。
        let mut child = match optional_element(unsafe {
            // 在 child 发现时同时构建缓存。
            walker.GetFirstChildElementBuildCache(&current.element, &cache_request)
        }) {
            // 成功时保存可选 child。
            Ok(child) => child,
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
            // 发现队列与已访问节点共同受 maximumItems 约束。
            if visited.saturating_add(pending.len()) >= maximum_items {
                // 尚有当前 child 未读取，搜索完整性必须失败闭合。
                traversal_complete = false;
                // 禁止继续枚举并缓存无界 sibling。
                break;
            }
            // 在移动 current_child 前读取下一个 sibling。
            let next = optional_element(unsafe {
                // 在 sibling 发现时同时构建缓存。
                walker.GetNextSiblingElementBuildCache(&current_child, &cache_request)
            });
            // 把 child 加入 BFS 队列。
            pending.push_back(PendingNode {
                // 移交 child 智能指针。
                element: current_child,
                // 子节点深度加一。
                depth: current.depth.saturating_add(1),
            });
            // 解包下一个 sibling 或标记遍历不完整。
            child = match next {
                // 成功时保存可选 sibling。
                Ok(next) => next,
                // 失败时停止当前 sibling 链。
                Err(_) => {
                    // 标记 provider traversal 缺口。
                    traversal_complete = false;
                    // 停止 sibling 遍历。
                    None
                }
            };
        }
    }
    // 队列残留或 provider 缺口都表示截断。
    let truncated = !pending.is_empty() || !traversal_complete;
    // 只读查询父窗口最小化状态。
    let minimized = unsafe {
        // 不公开 HWND。
        IsIconic(HWND(hwnd as *mut std::ffi::c_void)).as_bool()
    };
    // 保存最多两个匹配的封闭计数。
    let match_count = matches.len();
    // 仅唯一匹配时读取 bounds 与命中区域。
    let element = if match_count == 1 {
        // 移出唯一候选。
        let located = matches
            // 取得首项。
            .pop()
            // 计数已经证明存在。
            .ok_or_else(|| {
                // 返回内部一致性错误。
                ObservationWorkerErrorCode::AccessibilityUnavailable.error(
                    // 不公开节点内容。
                    "The observation worker lost its unique accessibility match.",
                )
            })?;
        // 投影位置与命中区域。
        Some(project_unique_match(located, minimized)?)
    } else {
        // 零或多匹配不返回任意节点。
        None
    };
    // 返回协议 data。
    Ok(json!({
        // 输出实际访问数量。
        "visited": visited,
        // 输出搜索截断状态。
        "truncated": truncated,
        // 输出语义属性完整性。
        "propertiesComplete": properties_complete,
        // 输出封闭零一多计数。
        "matchCount": match_count,
        // 输出 provider-neutral 窗口状态。
        "windowState": if minimized { "minimized" } else { "not-minimized" },
        // 仅唯一匹配携带节点。
        "element": element,
    }))
}

// 验证纯几何和自然结束辅助语义。
#[cfg(test)]
mod tests {
    // 导入被测 helper。
    use super::*;

    // 验证虚拟桌面负坐标和半开矩形边界。
    #[test]
    fn physical_point_validation_supports_multi_monitor_coordinates() {
        // 构造跨零点的有效矩形。
        let bounds = RECT {
            // 允许左侧显示器负坐标。
            left: -1920,
            // 允许上方显示器负坐标。
            top: -200,
            // 固定右边界。
            right: -1600,
            // 固定下边界。
            bottom: 100,
        };
        // 内部负坐标点必须可命中。
        assert!(point_inside(POINT { x: -1800, y: 0 }, bounds));
        // 右边界属于半开区间外部。
        assert!(!point_inside(POINT { x: -1600, y: 0 }, bounds));
    }
}
