//! 组合窗口重解析与隔离 worker，实现 provider-neutral 元素定位 Query。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "element_location_error.rs"]
mod error_code;

// 导入严格输入反序列化。
use serde::Deserialize;
// 导入 JSON 对象、值与构造宏。
use serde_json::{Map, Value, json};

// 导入窗口观察、capability、selector 与 Accessibility worker 边界。
use crate::{
    // 导入安全窗口重新发现和前景验证。
    adapters::{
        // 导入窗口领域只读原语。
        window::{
            // 导入可见有标题窗口快照。
            capture_visible_titled_windows,
            // 导入前景不变验证。
            ensure_foreground_unchanged,
            // 导入安全窗口投影。
            public_window_observation,
            // 导入 fail-closed 精确目标解析。
            resolve_window,
        },
        // 导入前景只读 token。
        windows::foreground_hwnd,
    },
    // 导入 capability 单一注册表。
    capabilities,
    // 导入共享 selector。
    components::accessibility_selector::{AccessibilitySelector, AccessibilitySelectorError},
    // 导入统一结果类型。
    domain::AppResult,
    // 导入现有 Job-bounded Accessibility worker 客户端。
    modules::accessibility,
};

// 导入当前 Module 私有错误类型。
use error_code::ElementLocationErrorCode;

// 固定 observation worker 协议版本。
const WORKER_CONTRACT: &str = "act/observation-worker/v1";
// 固定默认 worker deadline。
const DEFAULT_TIMEOUT_MS: u32 = 2_000;
// 固定默认查询深度。
const DEFAULT_MAXIMUM_DEPTH: usize = 8;
// 固定默认查询节点数。
const DEFAULT_MAXIMUM_ITEMS: usize = 1_024;
// 限制定位 worker 输出。
const LOCATION_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

// 返回默认 worker deadline。
const fn default_timeout_ms() -> u32 {
    // 使用公开默认值。
    DEFAULT_TIMEOUT_MS
}

// 返回默认查询深度。
const fn default_maximum_depth() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_DEPTH
}

// 返回默认查询节点数。
const fn default_maximum_items() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_ITEMS
}

// 返回默认 ControlView。
fn default_view() -> String {
    // 创建独立字符串所有权。
    "control".to_owned()
}

// 保存完整版本一定位输入。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElementLocationInput {
    // 保存语义 selector。
    selector: AccessibilitySelector,
    // 保存查询深度边界。
    #[serde(default = "default_maximum_depth")]
    maximum_depth: usize,
    // 保存查询节点边界。
    #[serde(default = "default_maximum_items")]
    maximum_items: usize,
    // 保存 UIA view。
    #[serde(default = "default_view")]
    view: String,
    // 保存 worker deadline。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

// 严格解析并验证公开输入。
fn parse_input(value: &Value) -> AppResult<ElementLocationInput> {
    // 按 deny_unknown_fields 契约反序列化。
    let input = serde_json::from_value::<ElementLocationInput>(value.clone()).map_err(|_| {
        // 不回显潜在敏感 selector。
        ElementLocationErrorCode::InvalidArgument.error(
            // 返回固定 schema 诊断。
            "Element location input violates schema://ui/element-locate/v1.",
        )
    })?;
    // 把共享 selector 错误映射为定位契约消息。
    if let Err(error) = input.selector.validate() {
        // 使用不回显 selector 的封闭消息。
        let message = match error {
            // 说明至少需要一个字段。
            AccessibilitySelectorError::Empty => {
                // 返回稳定诊断。
                "Element location selector requires at least one semantic field."
            }
            // 说明字符串边界。
            AccessibilitySelectorError::InvalidString => {
                // 返回稳定诊断。
                "Element location selector strings must contain 1..=512 UTF-8 bytes."
            }
            // 说明 control type 边界。
            AccessibilitySelectorError::InvalidControlType => {
                // 返回稳定诊断。
                "Element location selector controlType must be positive."
            }
        };
        // 返回定位 Module 自有参数错误。
        return Err(ElementLocationErrorCode::InvalidArgument.error(message));
    }
    // 验证全部有界查询参数。
    if input.maximum_depth > 20
        // 数量保持硬边界。
        || !(1..=4_096).contains(&input.maximum_items)
        // 只允许既有 UIA view。
        || !matches!(input.view.as_str(), "control" | "raw")
        // worker deadline 保持统一边界。
        || !(1..=30_000).contains(&input.timeout_ms)
    {
        // 返回统一有界参数错误。
        return Err(ElementLocationErrorCode::InvalidArgument.error(
            // 说明全部封闭范围。
            "Element location requires maximumDepth 0..20, maximumItems 1..4096, view control|raw, and timeoutMs 1..30000.",
        ));
    }
    // 返回已验证输入。
    Ok(input)
}

// 要求对象字段集合精确匹配。
fn exact_fields(
    // 接收对象。
    object: &Map<String, Value>,
    // 接收固定字段列表。
    fields: &[&str],
) -> bool {
    // 数量与逐字段存在性必须同时满足。
    object.len() == fields.len()
        // 每个固定字段都必须存在。
        && fields.iter().all(|field| object.contains_key(*field))
}

// 验证 worker bounds 并返回安全副本。
fn validate_bounds(value: &Value) -> AppResult<Value> {
    // 要求 bounds 对象。
    let bounds = value.as_object().ok_or_else(|| {
        // 返回 worker 协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker bounds must be an object.")
    })?;
    // 读取封闭状态。
    let state = bounds.get("state").and_then(Value::as_str).ok_or_else(|| {
        // 返回 worker 协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker bounds state is invalid.")
    })?;
    // 不可用 bounds 只允许状态和原因。
    if state == "unavailable" {
        // 读取封闭不可用原因。
        let reason = bounds.get("reason").and_then(Value::as_str);
        // 核对精确字段与 worker 允许产生的原因。
        if !exact_fields(bounds, &["state", "reason"])
            // 拒绝未知或 provider 注入的原因文本。
            || !matches!(reason, Some("provider-did-not-return-bounds" | "empty-bounds"))
        {
            // 拒绝额外原生字段。
            return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
                // 不回显 worker 数据。
                "Worker unavailable bounds violate the privacy contract.",
            ));
        }
        // 返回已验证副本。
        return Ok(value.clone());
    }
    // 可用 bounds 固定完整字段集合。
    if state != "available"
        // 核对精确字段。
        || !exact_fields(
            // 传入 bounds 对象。
            bounds,
            // 固定允许字段。
            &[
                "state",
                "coordinateSpace",
                "unit",
                "left",
                "top",
                "right",
                "bottom",
                "width",
                "height",
            ],
        )
        // 固定坐标空间。
        || bounds.get("coordinateSpace").and_then(Value::as_str) != Some("virtual-desktop")
        // 固定物理像素单位。
        || bounds.get("unit").and_then(Value::as_str) != Some("physical-screen-px")
    {
        // 拒绝未知形状。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显 worker 数据。
            "Worker available bounds violate the coordinate contract.",
        ));
    }
    // 读取四个边界与尺寸。
    let left = bounds.get("left").and_then(Value::as_i64);
    // 读取上边界。
    let top = bounds.get("top").and_then(Value::as_i64);
    // 读取右边界。
    let right = bounds.get("right").and_then(Value::as_i64);
    // 读取下边界。
    let bottom = bounds.get("bottom").and_then(Value::as_i64);
    // 读取宽度。
    let width = bounds.get("width").and_then(Value::as_i64);
    // 读取高度。
    let height = bounds.get("height").and_then(Value::as_i64);
    // 验证坐标关系与正尺寸。
    if left.zip(right).is_none_or(|(left, right)| right <= left)
        // 验证垂直关系。
        || top.zip(bottom).is_none_or(|(top, bottom)| bottom <= top)
        // 验证宽度与边界一致。
        || left.zip(right).zip(width).is_none_or(|((left, right), width)| {
            // 要求无溢出精确差值。
            width != right - left || width <= 0
        })
        // 验证高度与边界一致。
        || top.zip(bottom).zip(height).is_none_or(|((top, bottom), height)| {
            // 要求无溢出精确差值。
            height != bottom - top || height <= 0
        })
    {
        // 返回坐标协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显坐标。
            "Worker bounds values are invalid.",
        ));
    }
    // 返回已验证副本。
    Ok(value.clone())
}

// 验证 worker hit region 并返回安全副本。
fn validate_hit_region(value: &Value) -> AppResult<Value> {
    // 要求对象。
    let hit = value.as_object().ok_or_else(|| {
        // 返回 worker 协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker hitRegion must be an object.")
    })?;
    // 读取状态。
    let state = hit.get("state").and_then(Value::as_str).ok_or_else(|| {
        // 返回 worker 协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker hitRegion state is invalid.")
    })?;
    // 不可用命中区域只允许四个固定字段。
    if state == "unavailable" {
        // 读取封闭不可用原因。
        let reason = hit.get("reason").and_then(Value::as_str);
        // 核对字段、原因和遮挡未知语义。
        if !exact_fields(hit, &["state", "reason", "occlusion"])
            // 只接受 worker 当前实现能够产生的原因。
            || !matches!(
                // 对比封闭原因集合。
                reason,
                // 枚举所有 provider-neutral 不可用原因。
                Some(
                    "window-minimized"
                        | "element-offscreen"
                        | "bounds-unavailable"
                        | "no-clickable-point"
                        | "provider-point-outside-bounds"
                        | "provider-did-not-return-clickable-point"
                )
            )
            // 不可用时禁止猜测遮挡。
            || hit.get("occlusion").and_then(Value::as_str) != Some("unknown")
        {
            // 拒绝额外原生字段。
            return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
                // 不回显 worker 数据。
                "Worker unavailable hitRegion violates the privacy contract.",
            ));
        }
        // 返回已验证副本。
        return Ok(value.clone());
    }
    // 可用命中点固定完整字段集合和值。
    if state != "available"
        // 核对精确字段。
        || !exact_fields(
            // 传入命中区域对象。
            hit,
            // 固定允许字段。
            &[
                "state",
                "source",
                "coordinateSpace",
                "unit",
                "x",
                "y",
                "occlusion",
            ],
        )
        // 只接受 UIA provider 给出的点。
        || hit.get("source").and_then(Value::as_str) != Some("uia-clickable-point")
        // 固定虚拟桌面坐标空间。
        || hit.get("coordinateSpace").and_then(Value::as_str) != Some("virtual-desktop")
        // 固定物理像素单位。
        || hit.get("unit").and_then(Value::as_str) != Some("physical-screen-px")
        // 横坐标必须为整数。
        || hit.get("x").and_then(Value::as_i64).is_none()
        // 纵坐标必须为整数。
        || hit.get("y").and_then(Value::as_i64).is_none()
        // 不扩大为视觉遮挡证明。
        || hit.get("occlusion").and_then(Value::as_str)
            != Some("provider-clickable-point-available")
    {
        // 返回命中区域协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显坐标。
            "Worker available hitRegion violates the coordinate contract.",
        ));
    }
    // 返回已验证副本。
    Ok(value.clone())
}

// 验证唯一 worker element 并拆出语义与几何。
fn validate_element(
    // 接收 worker element。
    value: &Value,
    // 接收调用方深度边界。
    maximum_depth: usize,
) -> AppResult<(Value, Value, Value)> {
    // 要求对象。
    let element = value.as_object().ok_or_else(|| {
        // 返回 worker 协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker element must be an object.")
    })?;
    // 固定允许字段集合。
    let fields = [
        // 允许 opaque node ID。
        "nodeId",
        // 允许深度。
        "depth",
        // 允许语义名称。
        "name",
        // 允许 automation ID。
        "automationId",
        // 允许类名。
        "className",
        // 允许 framework ID。
        "frameworkId",
        // 允许 control type。
        "controlType",
        // 允许 enabled 状态。
        "enabled",
        // 允许 offscreen 状态。
        "offscreen",
        // 允许属性完整性。
        "propertyReadComplete",
        // 允许 bounds。
        "bounds",
        // 允许命中区域。
        "hitRegion",
    ];
    // 字段必须精确匹配。
    if !exact_fields(element, &fields) {
        // 拒绝原生或额外字段。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显 worker element。
            "Worker element fields violate the privacy contract.",
        ));
    }
    // 解析 opaque element ID。
    let node_id = element
        // 读取固定字段。
        .get("nodeId")
        // 要求字符串。
        .and_then(Value::as_str)
        // 解析 canonical ID。
        .and_then(crate::components::opaque_id::OpaqueTargetId::parse);
    // 只接受 snapshot element 类别。
    if node_id.map(crate::components::opaque_id::OpaqueTargetId::kind)
        != Some(crate::components::opaque_id::OpaqueTargetKind::Element)
    {
        // 拒绝 native 或错误类别身份。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显 ID。
            "Worker nodeId is not a canonical s2:e target.",
        ));
    }
    // 验证深度、文本、状态和完整性。
    if element
        // 读取深度。
        .get("depth")
        // 要求 u64。
        .and_then(Value::as_u64)
        // 不得超出调用方边界。
        .is_none_or(|depth| depth > u64::try_from(maximum_depth).unwrap_or(u64::MAX))
        // 验证四个文本字段。
        || ["name", "automationId", "className", "frameworkId"]
            // 遍历字段。
            .iter()
            // 任一非字符串即失败。
            .any(|field| element.get(*field).and_then(Value::as_str).is_none())
        // 验证 control type。
        || element.get("controlType").and_then(Value::as_i64).is_none()
        // 验证 enabled。
        || element.get("enabled").and_then(Value::as_bool).is_none()
        // 验证 offscreen。
        || element.get("offscreen").and_then(Value::as_bool).is_none()
        // 唯一成功 element 必须完整。
        || element.get("propertyReadComplete").and_then(Value::as_bool) != Some(true)
    {
        // 返回字段协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显 element。
            "Worker element semantic fields are invalid.",
        ));
    }
    // 验证 bounds。
    let bounds = validate_bounds(&element["bounds"])?;
    // 验证命中区域。
    let hit_region = validate_hit_region(&element["hitRegion"])?;
    // 复制语义 element 并移除几何字段。
    let mut semantic = element.clone();
    // bounds 在固定 geometry 下公开一次。
    semantic.remove("bounds");
    // hitRegion 在固定 geometry 下公开一次。
    semantic.remove("hitRegion");
    // 明确 element ID 只属于本次定位快照。
    semantic.insert(
        // 使用稳定 freshness 字段。
        "identityFreshness".to_owned(),
        // 禁止作为后续写目标。
        Value::String("location-snapshot".to_owned()),
    );
    // 返回语义 element 与几何。
    Ok((Value::Object(semantic), bounds, hit_region))
}

// 验证 worker 定位 data 并投影公共结果。
fn public_result(
    // 接收 worker data。
    data: &Value,
    // 接收安全窗口投影。
    session: Value,
    // 接收原 canonical 窗口 ID。
    session_id: &str,
    // 接收已验证输入。
    input: &ElementLocationInput,
) -> AppResult<Value> {
    // 要求固定对象。
    let data = data.as_object().ok_or_else(|| {
        // 返回协议错误。
        ElementLocationErrorCode::WorkerProtocolFailed
            .error("Worker location data must be an object.")
    })?;
    // 固定允许字段。
    if !exact_fields(
        // 传入 worker 对象。
        data,
        // 拒绝额外原生字段。
        &[
            "visited",
            "truncated",
            "propertiesComplete",
            "matchCount",
            "windowState",
            "element",
        ],
    ) {
        // 返回协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显 worker data。
            "Worker location fields violate the privacy contract.",
        ));
    }
    // 读取访问数量。
    let visited = data.get("visited").and_then(Value::as_u64).ok_or_else(|| {
        // 返回字段错误。
        ElementLocationErrorCode::WorkerProtocolFailed.error("Worker visited count is invalid.")
    })?;
    // 数量不得超过调用方边界。
    if visited > u64::try_from(input.maximum_items).unwrap_or(u64::MAX) {
        // 返回边界错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显数量。
            "Worker location exceeded maximumItems.",
        ));
    }
    // 读取截断状态。
    let truncated = data
        .get("truncated")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            // 返回字段错误。
            ElementLocationErrorCode::WorkerProtocolFailed
                .error("Worker truncated flag is invalid.")
        })?;
    // 读取属性完整性。
    let properties_complete = data
        // 读取固定字段。
        .get("propertiesComplete")
        // 要求布尔值。
        .and_then(Value::as_bool)
        // 映射字段错误。
        .ok_or_else(|| {
            // 返回协议错误。
            ElementLocationErrorCode::WorkerProtocolFailed.error(
                // 不回显数据。
                "Worker property completeness flag is invalid.",
            )
        })?;
    // 读取封闭匹配计数。
    let match_count = data
        .get("matchCount")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            // 返回字段错误。
            ElementLocationErrorCode::WorkerProtocolFailed.error("Worker matchCount is invalid.")
        })?;
    // 只接受零、一、二封闭分类。
    if match_count > 2 {
        // 返回协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不公开实际数量。
            "Worker matchCount exceeded the closed zero-one-many classification.",
        ));
    }
    // 读取窗口状态。
    let window_state = data
        .get("windowState")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            // 返回字段错误。
            ElementLocationErrorCode::WorkerProtocolFailed.error("Worker windowState is invalid.")
        })?;
    // 只允许最小化与非最小化状态。
    if !matches!(window_state, "minimized" | "not-minimized") {
        // 返回协议错误。
        return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
            // 不回显原值。
            "Worker windowState is outside the closed contract.",
        ));
    }
    // 多匹配已经足以证明歧义。
    if match_count == 2 {
        // 多匹配不得返回任意 element。
        if !data.get("element").is_some_and(Value::is_null) {
            // 拒绝任意候选泄漏。
            return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
                // 不回显候选。
                "Worker ambiguous location returned an arbitrary element.",
            ));
        }
        // 返回稳定歧义错误。
        return Err(ElementLocationErrorCode::AmbiguousTarget.error(
            // 不回显 selector。
            "The element selector matched more than one accessibility element.",
        ));
    }
    // 零或唯一结论都要求完整搜索。
    if truncated || !properties_complete {
        // 返回搜索不完整错误。
        return Err(ElementLocationErrorCode::SearchIncomplete.error(
            // 不猜测零或唯一。
            "The bounded accessibility search could not prove zero or unique element location.",
        ));
    }
    // 按零或唯一构造公开语义和几何。
    let (match_state, element, bounds, hit_region) = if match_count == 0 {
        // 零匹配不得携带 element。
        if !data.get("element").is_some_and(Value::is_null) {
            // 返回协议错误。
            return Err(ElementLocationErrorCode::WorkerProtocolFailed.error(
                // 不回显 worker element。
                "Worker missing location returned an element.",
            ));
        }
        // 最小化与普通窗口使用不同显式原因。
        let reason = if window_state == "minimized" {
            // 保留最小化事实。
            "window-minimized"
        } else {
            // 普通零匹配表示 element 未定位。
            "element-not-located"
        };
        // 返回固定 missing 形状。
        (
            // 使用封闭匹配状态。
            "missing",
            // 没有 element。
            Value::Null,
            // bounds 显式不可用。
            json!({ "state": "unavailable", "reason": reason }),
            // hit region 显式不可用且不猜测遮挡。
            json!({ "state": "unavailable", "reason": reason, "occlusion": "unknown" }),
        )
    } else {
        // 唯一匹配必须携带 element。
        let worker_element = data.get("element").ok_or_else(|| {
            // 返回协议错误。
            ElementLocationErrorCode::WorkerProtocolFailed
                .error("Worker unique location omitted its element.")
        })?;
        // 验证并拆分语义与几何。
        let (element, bounds, hit_region) = validate_element(worker_element, input.maximum_depth)?;
        // 返回固定 unique 形状。
        ("unique", element, bounds, hit_region)
    };
    // 返回 provider-neutral 定位结果。
    Ok(json!({
        // 标记版本化 capability。
        "capability": capabilities::UI_ELEMENT_LOCATE,
        // 输出主进程实时安全窗口快照。
        "session": session,
        // 输出封闭匹配状态。
        "matchState": match_state,
        // 标记只读。
        "readOnly": true,
        // 成功路径保证前景不变。
        "foregroundUnchanged": true,
        // 输出窗口状态。
        "windowState": window_state,
        // 输出有界搜索证据。
        "search": {
            // selector 使用精确 AND 语义。
            "selectorSemantics": "exact-and",
            // 输出 view。
            "view": input.view,
            // 输出深度边界。
            "maximumDepth": input.maximum_depth,
            // 输出数量边界。
            "maximumItems": input.maximum_items,
            // 输出实际访问数量。
            "visited": visited,
            // 成功返回只允许完整搜索。
            "complete": true,
        },
        // 唯一匹配返回 snapshot element，missing 返回 null。
        "element": element,
        // 始终返回显式几何状态。
        "geometry": {
            // 输出 bounds 状态。
            "bounds": bounds,
            // 输出命中区域状态。
            "hitRegion": hit_region,
        },
        // 输出当前窗口身份材料与坐标快照证据。
        "coordinateSnapshot": {
            // 绑定调用方当前 opaque 窗口身份材料。
            "windowSessionId": session_id,
            // 主进程与 worker 均重新解析。
            "windowReResolved": true,
            // UIA 坐标使用虚拟桌面。
            "coordinateSpace": "virtual-desktop",
            // 坐标是物理屏幕像素。
            "unit": "physical-screen-px",
            // worker 使用 Per-Monitor-V2 线程上下文。
            "dpiAwareness": "per-monitor-v2-thread",
            // 允许负坐标和多显示器虚拟桌面。
            "multiMonitor": "virtual-desktop-signed-coordinates",
            // 窗口移动后必须重新定位。
            "validity": "snapshot-only-re-resolve-before-action",
        },
        // 输出可机器验证的安全声明。
        "safety": {
            // 未读取 Value 内容。
            "valueContentRead": false,
            // 未读取 Text 内容。
            "textContentRead": false,
            // 未查询任何写 pattern。
            "writePatternsQueried": false,
            // 未执行 UIA 写入。
            "writePerformed": false,
            // provider 位于 Job 隔离 worker。
            "providerTimeoutIsolation": "job-bounded-worker",
            // 输出实际 worker deadline。
            "workerTimeoutMs": input.timeout_ms,
            // snapshot element ID 不可作为后续写目标。
            "elementIdentityUse": "observation-only",
        },
    }))
}

// 执行内部定位流程。
fn locate_internal(session_id: &str, value: &Value) -> AppResult<Value> {
    // 严格解析输入。
    let input = parse_input(value)?;
    // 记录主进程观察前前景。
    let before = foreground_hwnd();
    // 捕获当前可见有标题窗口。
    let windows = capture_visible_titled_windows()?;
    // 在启动 worker 前 fail closed 重新解析。
    let window = resolve_window(session_id, &windows)?;
    // 复制安全公开窗口快照。
    let session = public_window_observation(window);
    // 构造固定定位请求。
    let request = json!({
        // 输出协议版本。
        "contractVersion": WORKER_CONTRACT,
        // 指定只读定位 operation。
        "operation": "accessibility-element-locate",
        // 只传 opaque 窗口目标。
        "sessionId": session_id,
        // 输出深度边界。
        "maximumDepth": input.maximum_depth,
        // 输出数量边界。
        "maximumItems": input.maximum_items,
        // 输出 view。
        "view": input.view,
        // 输出 provider-neutral selector。
        "selector": value["selector"],
    });
    // 在隔离 worker 中执行有界定位。
    let data = accessibility::run_worker(
        // 传入固定请求。
        request,
        // 传入硬 deadline。
        input.timeout_ms,
        // 传入输出边界。
        LOCATION_OUTPUT_BYTES,
    )?;
    // 验证并投影公开结果。
    let result = public_result(&data, session, session_id, &input)?;
    // 记录观察后前景。
    let after = foreground_hwnd();
    // 前景变化必须 fail closed。
    ensure_foreground_unchanged(before, after)?;
    // 返回已验证结果。
    Ok(result)
}

// 对精确窗口公开执行只读元素定位。
pub(crate) fn locate(session_id: &str, input: &Value) -> AppResult<Value> {
    // 执行并收敛共享 worker 私有错误。
    locate_internal(session_id, input).map_err(accessibility::public_error)
}

// 验证输入、worker 形状与公开缺口语义。
#[cfg(test)]
mod tests {
    // 导入被测 helper。
    use super::*;

    // 构造最小合法输入。
    fn input() -> Value {
        // 返回严格 schema 对象。
        json!({
            // 使用单一 automation ID selector。
            "selector": { "automationId": "ready" },
            // 使用 root-only 搜索便于固定边界。
            "maximumDepth": 0,
            // 只允许一个节点。
            "maximumItems": 1,
            // 使用 control view。
            "view": "control",
            // 使用最小 worker deadline。
            "timeoutMs": 1,
        })
    }

    // 构造安全窗口投影。
    fn session() -> Value {
        // 只提供结果投影会透传的安全对象。
        json!({ "sessionId": "s2:w:0000000000000000", "kind": "window" })
    }

    // 验证公开输入严格拒绝空 selector 与未知字段。
    #[test]
    fn input_is_closed_and_bounded() {
        // 最小输入必须成功。
        assert!(parse_input(&input()).is_ok());
        // 清空 selector。
        let mut empty = input();
        // 注入空对象。
        empty["selector"] = json!({});
        // 空 selector 必须拒绝。
        assert!(parse_input(&empty).is_err());
        // 添加应用特定 selector。
        let mut unknown = input();
        // 注入 schema 外字段。
        unknown["selector"]["xpath"] = json!("//button");
        // 应用特定字段必须拒绝。
        assert!(parse_input(&unknown).is_err());
        // worker 不可用原因同样必须保持封闭。
        assert!(
            // 注入 schema 外原因文本。
            validate_bounds(&json!({ "state": "unavailable", "reason": "native-provider-gap" }))
                // 未知原因不得跨越主进程协议边界。
                .is_err()
        );
    }

    // 验证 zero、ambiguous 与 incomplete 分类互不混淆。
    #[test]
    fn worker_results_keep_zero_many_and_incomplete_distinct() -> AppResult<()> {
        // 解析固定输入。
        let input = parse_input(&input())?;
        // 构造完整零匹配。
        let missing = json!({
            // 访问一个节点。
            "visited": 1,
            // 搜索完整。
            "truncated": false,
            // 属性完整。
            "propertiesComplete": true,
            // 零匹配。
            "matchCount": 0,
            // 窗口非最小化。
            "windowState": "not-minimized",
            // 不返回 element。
            "element": null,
        });
        // 零匹配必须成功返回 missing。
        assert_eq!(
            public_result(&missing, session(), "s2:w:0000000000000000", &input)?["matchState"],
            // 对比封闭状态。
            "missing"
        );
        // 两个匹配必须返回歧义。
        let ambiguous = json!({
            // 访问一个节点的计数仅作为安全夹具。
            "visited": 1,
            // 歧义已证明时截断值不改变分类。
            "truncated": true,
            // 属性完整。
            "propertiesComplete": true,
            // 使用 many 哨兵。
            "matchCount": 2,
            // 窗口非最小化。
            "windowState": "not-minimized",
            // 不返回任意候选。
            "element": null,
        });
        // 歧义码必须稳定。
        assert_eq!(
            public_result(&ambiguous, session(), "s2:w:0000000000000000", &input)
                // 提取错误。
                .err()
                // 读取稳定码。
                .map(|error| error.code),
            // 对比歧义分类。
            Some("AMBIGUOUS_TARGET")
        );
        // 零匹配但截断必须返回搜索不完整。
        let incomplete = json!({
            // 访问一个节点。
            "visited": 1,
            // 搜索被边界截断。
            "truncated": true,
            // 属性完整。
            "propertiesComplete": true,
            // 尚未命中。
            "matchCount": 0,
            // 窗口非最小化。
            "windowState": "not-minimized",
            // 不返回 element。
            "element": null,
        });
        // 搜索不完整码必须稳定。
        assert_eq!(
            public_result(&incomplete, session(), "s2:w:0000000000000000", &input)
                // 提取错误。
                .err()
                // 读取稳定码。
                .map(|error| error.code),
            // 对比不完整分类。
            Some("SEARCH_INCOMPLETE")
        );
        // 返回测试成功。
        Ok(())
    }
}
