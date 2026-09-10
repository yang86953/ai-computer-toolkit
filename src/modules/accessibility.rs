//! 组合窗口重新发现与 Job-bounded worker，提供只读可访问性领域行为。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "accessibility_error.rs"]
mod error_code;

// 导入路径与 deadline 类型。
use std::{
    // 导入 companion 路径。
    path::PathBuf,
    // 导入 worker deadline。
    time::Duration,
};

// 导入 JSON 构造与值类型。
use serde_json::{Map, Value, json};

// 导入窗口只读边界、capability 与 worker Component。
use crate::{
    // 导入当前窗口重新发现与安全公开投影。
    adapters::{
        // 导入窗口 Module 辅助边界。
        window::{
            // 导入可见有标题窗口快照。
            capture_visible_titled_windows,
            // 导入前景不变验证。
            ensure_foreground_unchanged,
            // 导入安全窗口投影。
            public_window_observation,
            // 导入 fail-closed 目标解析。
            resolve_window,
        },
        // 导入前景只读 token。
        windows::foreground_hwnd,
    },
    // 导入 capability 单一注册表。
    capabilities,
    // 导入取消与 Job worker Component。
    components::{
        // 导入进程取消状态。
        cancellation,
        // 导入 worker 运行结果与入口。
        worker_process::{WorkerOutput, run_companion, sibling_companion_path},
    },
    // 导入统一错误类型。
    domain::{AppControlError, AppResult},
};

// 导入当前 Module 私有封闭错误码与公开映射。
use error_code::AccessibilityErrorCode;

// 固定 companion worker 协议版本。
const WORKER_CONTRACT: &str = "act/observation-worker/v1";
// 固定 Rust companion 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-observation-worker.exe";
// 限制 root-only 输出。
const ROOT_OUTPUT_BYTES: usize = 1024 * 1024;
// 限制最大树输出。
const TREE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

// 定位与当前 Rust 主程序同目录的 companion worker。
fn worker_path() -> AppResult<PathBuf> {
    // 复用唯一 sibling companion 定位 Component。
    sibling_companion_path(
        // 只允许固定 Rust observation worker 文件名。
        WORKER_FILE_NAME,
        // 使用不含本机路径的安全描述。
        "Rust observation",
    )
}

// 从 worker 失败 envelope 读取安全字符串字段。
fn required_string<'value>(
    // 接收 JSON 对象。
    object: &'value Map<String, Value>,
    // 接收字段名。
    field: &str,
) -> AppResult<&'value str> {
    // 只接受非空字符串。
    object
        // 读取字段。
        .get(field)
        // 要求字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 映射协议错误。
        .ok_or_else(|| {
            // 不回显 worker 原始输出。
            AccessibilityErrorCode::WorkerProtocolFailed.error(
                // 指明字段形状错误。
                format!("Worker field '{field}' is missing or invalid."),
            )
        })
}

// 解析 worker envelope 并只返回成功 data。
fn worker_data(output: WorkerOutput) -> AppResult<Value> {
    // 要求顶层对象。
    let envelope = output.envelope.as_object().ok_or_else(|| {
        // 返回稳定协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker result must be an object.")
    })?;
    // 验证固定协议版本。
    if envelope.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT) {
        // 拒绝未知 worker 版本。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不协商未知版本。
            "Worker result contractVersion is invalid.",
        ));
    }
    // 读取成功标志。
    let ok = envelope.get("ok").and_then(Value::as_bool).ok_or_else(|| {
        // 返回稳定协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker result ok flag is invalid.")
    })?;
    // 成功 envelope 必须对应零退出码。
    if ok {
        // 拒绝退出码与 envelope 矛盾。
        if output.exit_code != 0 {
            // 返回协议失败。
            return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
                // 不公开平台退出码。
                "Worker reported success with a nonzero exit code.",
            ));
        }
        // 要求 data 存在并复制出借用对象。
        return envelope.get("data").cloned().ok_or_else(|| {
            // 返回缺失 data 错误。
            AccessibilityErrorCode::WorkerProtocolFailed.error("Worker success data is missing.")
        });
    }
    // 失败 envelope 必须对应非零退出码。
    if output.exit_code == 0 {
        // 拒绝退出码与 envelope 矛盾。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不公开原生退出码。
            "Worker reported failure with a zero exit code.",
        ));
    }
    // 要求结构化 error 对象。
    let error = envelope
        // 读取 error 字段。
        .get("error")
        // 要求对象。
        .and_then(Value::as_object)
        // 映射协议错误。
        .ok_or_else(|| {
            // 返回稳定协议错误。
            AccessibilityErrorCode::WorkerProtocolFailed.error("Worker error envelope is invalid.")
        })?;
    // 读取稳定错误码。
    let code = required_string(error, "code")?;
    // 读取安全错误消息。
    let message = required_string(error, "message")?.to_owned();
    // 只允许封闭 worker 失败白名单穿过稳定公共边界。
    let public_code = AccessibilityErrorCode::from_worker_code(code).ok_or_else(|| {
        // 未知错误码必须失败闭合且不回显原值。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker returned an unknown error code.")
    })?;
    // 返回 worker 报告的结构化错误。
    Err(public_code.error(message))
}

// 运行一次固定协议 worker 请求。
pub(super) fn run_worker(
    // 接收固定 worker 请求。
    request: Value,
    // 接收 worker deadline。
    timeout_ms: u32,
    // 接收最大输出字节数。
    maximum_output_bytes: usize,
) -> AppResult<Value> {
    // 验证 deadline 硬边界。
    if !(1..=30_000).contains(&timeout_ms) {
        // 返回参数错误。
        return Err(AccessibilityErrorCode::InvalidArgument.error(
            // 说明允许边界。
            "timeoutMs must be between 1 and 30000.",
        ));
    }
    // 定位精确 sibling worker。
    let worker = worker_path()?;
    // 在 Job 中运行 worker 并解析 envelope。
    let output = run_companion(
        // 传入 worker 路径。
        &worker,
        // 生产观察 worker 不接受命令行参数。
        &[],
        // 传入版本化请求。
        &request,
        // 传入 deadline。
        Duration::from_millis(u64::from(timeout_ms)),
        // 传入输出边界。
        maximum_output_bytes,
        // 传入控制台取消轮询。
        cancellation::is_cancelled,
    )?;
    // 验证并解包 data。
    worker_data(output)
}

// 验证 root data 只包含固定结构属性。
fn validate_root(data: &Value) -> AppResult<Value> {
    // 要求 root 对象。
    let root = data.as_object().ok_or_else(|| {
        // 返回协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker root data must be an object.")
    })?;
    // 固定允许字段列表。
    let allowed = [
        // 允许名称。
        "name",
        // 允许 automation ID。
        "automationId",
        // 允许类名。
        "className",
        // 允许 framework ID。
        "frameworkId",
        // 允许 control type。
        "controlType",
        // 允许 enabled。
        "enabled",
        // 允许 offscreen。
        "offscreen",
    ];
    // 要求字段集合精确匹配。
    if root.len() != allowed.len() || !allowed.iter().all(|field| root.contains_key(*field)) {
        // 拒绝额外隐私字段或缺失字段。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显原始数据。
            "Worker root fields violate the privacy contract.",
        ));
    }
    // 验证四个文本字段。
    if ["name", "automationId", "className", "frameworkId"]
        // 遍历文本字段。
        .iter()
        // 任一非字符串即失败。
        .any(|field| root.get(*field).and_then(Value::as_str).is_none())
        // 验证 control type 是 i64。
        || root.get("controlType").and_then(Value::as_i64).is_none()
        // 验证 enabled 是 bool。
        || root.get("enabled").and_then(Value::as_bool).is_none()
        // 验证 offscreen 是 bool。
        || root.get("offscreen").and_then(Value::as_bool).is_none()
    {
        // 返回字段类型错误。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显 provider 内容。
            "Worker root field types are invalid.",
        ));
    }
    // 返回已验证 root 副本。
    Ok(data.clone())
}

// 验证单个树节点并添加 snapshot freshness。
fn validate_node(node: &Value, maximum_depth: usize) -> AppResult<Value> {
    // 要求节点对象。
    let node = node.as_object().ok_or_else(|| {
        // 返回协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker tree node must be an object.")
    })?;
    // 固定允许字段列表。
    let allowed = [
        // 允许 opaque node ID。
        "nodeId",
        // 允许深度。
        "depth",
        // 允许名称。
        "name",
        // 允许 automation ID。
        "automationId",
        // 允许类名。
        "className",
        // 允许 framework ID。
        "frameworkId",
        // 允许 control type。
        "controlType",
        // 允许 enabled。
        "enabled",
        // 允许 offscreen。
        "offscreen",
        // 允许属性读取完整性。
        "propertyReadComplete",
    ];
    // 要求字段集合精确匹配。
    if node.len() != allowed.len() || !allowed.iter().all(|field| node.contains_key(*field)) {
        // 拒绝额外隐私字段或缺失字段。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显节点内容。
            "Worker tree node fields violate the privacy contract.",
        ));
    }
    // 读取并严格解析 node ID。
    let node_id = node
        .get("nodeId")
        .and_then(Value::as_str)
        .and_then(|value| {
            // 解析 canonical opaque ID。
            crate::components::opaque_id::OpaqueTargetId::parse(value)
        });
    // 只接受 snapshot element 类别。
    if node_id.map(crate::components::opaque_id::OpaqueTargetId::kind)
        != Some(crate::components::opaque_id::OpaqueTargetKind::Element)
    {
        // 拒绝 native 或错误类别节点 ID。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显目标值。
            "Worker nodeId is not a canonical s2:e target.",
        ));
    }
    // 读取深度。
    let depth = node.get("depth").and_then(Value::as_u64).ok_or_else(|| {
        // 返回字段类型错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker node depth is invalid.")
    })?;
    // 深度不得超过调用方边界。
    if depth > u64::try_from(maximum_depth).unwrap_or(u64::MAX) {
        // 返回边界错误。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不公开节点内容。
            "Worker node exceeded maximumDepth.",
        ));
    }
    // 验证四个文本字段。
    if ["name", "automationId", "className", "frameworkId"]
        // 遍历文本字段。
        .iter()
        // 任一非字符串即失败。
        .any(|field| node.get(*field).and_then(Value::as_str).is_none())
        // 验证 control type。
        || node.get("controlType").and_then(Value::as_i64).is_none()
        // 验证 enabled。
        || node.get("enabled").and_then(Value::as_bool).is_none()
        // 验证 offscreen。
        || node.get("offscreen").and_then(Value::as_bool).is_none()
        // 验证 propertyReadComplete。
        || node
            .get("propertyReadComplete")
            .and_then(Value::as_bool)
            .is_none()
    {
        // 返回类型错误。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显节点内容。
            "Worker node field types are invalid.",
        ));
    }
    // 复制已验证节点。
    let mut public = node.clone();
    // 由主 Module 明确添加快照生命周期。
    public.insert(
        // 使用兼容字段名。
        "identityFreshness".to_owned(),
        // 使用固定契约值。
        Value::String("inspection-snapshot".to_owned()),
    );
    // 返回安全公开节点。
    Ok(Value::Object(public))
}

// 验证 worker 树 data 并返回公共节点。
fn validate_tree(
    // 接收 worker data。
    data: &Value,
    // 接收深度上限。
    maximum_depth: usize,
    // 接收数量上限。
    maximum_items: usize,
) -> AppResult<(usize, bool, Vec<Value>)> {
    // 要求树对象。
    let tree = data.as_object().ok_or_else(|| {
        // 返回协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker tree data must be an object.")
    })?;
    // 只允许 visited、truncated 与 nodes。
    if tree.len() != 3
        || !["visited", "truncated", "nodes"]
            // 遍历固定字段。
            .iter()
            // 要求每个字段存在。
            .all(|field| tree.contains_key(*field))
    {
        // 拒绝额外隐私字段。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显树内容。
            "Worker tree fields violate the privacy contract.",
        ));
    }
    // 读取 visited。
    let visited = tree.get("visited").and_then(Value::as_u64).ok_or_else(|| {
        // 返回字段错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker visited count is invalid.")
    })?;
    // 转换 visited。
    let visited = usize::try_from(visited).map_err(|_| {
        // 返回溢出错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker visited count overflowed.")
    })?;
    // 读取 truncated。
    let truncated = tree
        // 读取字段。
        .get("truncated")
        // 要求 bool。
        .and_then(Value::as_bool)
        // 映射字段错误。
        .ok_or_else(|| {
            // 返回协议错误。
            AccessibilityErrorCode::WorkerProtocolFailed.error("Worker truncated flag is invalid.")
        })?;
    // 读取节点数组。
    let nodes = tree.get("nodes").and_then(Value::as_array).ok_or_else(|| {
        // 返回协议错误。
        AccessibilityErrorCode::WorkerProtocolFailed.error("Worker nodes array is invalid.")
    })?;
    // 数量必须与 visited 一致且不超过边界。
    if nodes.len() != visited || nodes.len() > maximum_items {
        // 拒绝数量不一致。
        return Err(AccessibilityErrorCode::WorkerProtocolFailed.error(
            // 不回显数量细节。
            "Worker node counts violate maximumItems.",
        ));
    }
    // 验证每个节点并添加 freshness。
    let nodes = nodes
        // 遍历 worker 节点。
        .iter()
        // 验证单节点。
        .map(|node| validate_node(node, maximum_depth))
        // 收集并传播首个错误。
        .collect::<AppResult<Vec<_>>>()?;
    // 返回验证结果。
    Ok((visited, truncated, nodes))
}

// 对 canonical s2:w 执行 root-only 检查。
fn inspect_root_internal(session_id: &str, timeout_ms: u32) -> AppResult<Value> {
    // 记录主进程观察前前景。
    let before = foreground_hwnd();
    // 捕获当前可见有标题窗口。
    let windows = capture_visible_titled_windows()?;
    // 在启动 worker 前 fail-closed 重新解析。
    let window = resolve_window(session_id, &windows)?;
    // 复制安全公开 session，避免借用跨越 worker 调用。
    let session = public_window_observation(window);
    // 构造固定 root 请求。
    let request = json!({
        // 输出协议版本。
        "contractVersion": WORKER_CONTRACT,
        // 指定 root operation。
        "operation": "accessibility-root",
        // 只传 opaque 目标。
        "sessionId": session_id,
    });
    // 在隔离 worker 中读取 root。
    let root = run_worker(request, timeout_ms, ROOT_OUTPUT_BYTES)?;
    // 验证 root 隐私与类型边界。
    let root = validate_root(&root)?;
    // 记录观察后前景。
    let after = foreground_hwnd();
    // 前景变化必须 fail closed。
    ensure_foreground_unchanged(before, after)?;
    // 返回 C++ 兼容 root 结果。
    Ok(json!({
        // 输出安全窗口 session。
        "session": session,
        // 标记只读。
        "readOnly": true,
        // 成功路径保证前景不变。
        "foregroundUnchanged": true,
        // 输出 root-only 可访问性数据。
        "accessibility": {
            // 标记 scope。
            "scope": "root-only",
            // 展开已验证 root 字段。
            "name": root["name"],
            // 输出 automation ID。
            "automationId": root["automationId"],
            // 输出类名。
            "className": root["className"],
            // 输出 framework ID。
            "frameworkId": root["frameworkId"],
            // 输出 control type。
            "controlType": root["controlType"],
            // 输出 enabled。
            "enabled": root["enabled"],
            // 输出 offscreen。
            "offscreen": root["offscreen"],
        },
    }))
}

// 对 canonical s2:w 执行有界树检查。
pub(super) fn inspect_tree_internal(
    // 接收 opaque 窗口目标。
    session_id: &str,
    // 接收深度上限。
    maximum_depth: usize,
    // 接收节点数量上限。
    maximum_items: usize,
    // 接收 view。
    view: &str,
    // 接收 worker deadline。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 验证公开树边界。
    if maximum_depth > 20
        // 验证数量边界。
        || !(1..=4096).contains(&maximum_items)
        // 验证 view。
        || !matches!(view, "control" | "raw")
    {
        // 返回参数错误。
        return Err(AccessibilityErrorCode::InvalidArgument.error(
            // 明确有界请求要求。
            "inspect-tree requires maxDepth 0..20, maxItems 1..4096, and view control|raw.",
        ));
    }
    // 记录主进程观察前前景。
    let before = foreground_hwnd();
    // 捕获当前可见有标题窗口。
    let windows = capture_visible_titled_windows()?;
    // 在启动 worker 前 fail-closed 重新解析。
    let window = resolve_window(session_id, &windows)?;
    // 复制安全公开 session。
    let session = public_window_observation(window);
    // 构造固定树请求。
    let request = json!({
        // 输出协议版本。
        "contractVersion": WORKER_CONTRACT,
        // 指定 tree operation。
        "operation": "accessibility-tree",
        // 只传 opaque 目标。
        "sessionId": session_id,
        // 输出深度边界。
        "maximumDepth": maximum_depth,
        // 输出数量边界。
        "maximumItems": maximum_items,
        // 输出 view。
        "view": view,
    });
    // 在隔离 worker 中读取树。
    let tree = run_worker(request, timeout_ms, TREE_OUTPUT_BYTES)?;
    // 验证树隐私、类型和边界。
    let (visited, truncated, nodes) = validate_tree(&tree, maximum_depth, maximum_items)?;
    // 记录观察后前景。
    let after = foreground_hwnd();
    // 前景变化必须 fail closed。
    ensure_foreground_unchanged(before, after)?;
    // 返回 C++ 兼容树结果。
    Ok(json!({
        // 标记版本化 capability。
        "capability": capabilities::ACCESSIBILITY_TREE_READ,
        // 输出安全窗口 session。
        "session": session,
        // 标记 bounded-tree scope。
        "scope": "bounded-tree",
        // 输出 view。
        "view": view,
        // 标记只读。
        "readOnly": true,
        // 成功路径保证前景不变。
        "foregroundUnchanged": true,
        // 输出深度边界。
        "maximumDepth": maximum_depth,
        // 输出数量边界。
        "maximumItems": maximum_items,
        // 输出访问数量。
        "visited": visited,
        // 输出截断状态。
        "truncated": truncated,
        // 输出安全节点。
        "nodes": nodes,
        // 输出可机器验证的安全声明。
        "safety": {
            // 未读取 Value 内容。
            "valueContentRead": false,
            // 未读取 Text 内容。
            "textContentRead": false,
            // 未查询写 pattern。
            "writePatternsQueried": false,
            // 未公开 bounds。
            "boundsExposed": false,
            // provider 位于 Job 隔离 worker。
            "providerTimeoutIsolation": "job-bounded-worker",
            // 输出实际 worker deadline。
            "workerTimeoutMs": timeout_ms,
            // 标记控制台可取消。
            "workerCancellable": true,
        },
    }))
}

// 把 Component 与 worker 私有错误收敛到公开错误契约。
pub(super) fn public_error(error: AppControlError) -> AppControlError {
    // 只转换不应跨越公开边界的内部错误码。
    match AccessibilityErrorCode::from_internal_code(error.code) {
        // 保留不含原生目标的安全消息并使用封闭公开分类。
        Some(public_code) => public_code.error(error.message),
        // 其他稳定错误已属于原所有者的公开契约。
        None => error,
    }
}

// 对 canonical s2:w 执行公开 root-only 检查。
pub(crate) fn inspect_root(session_id: &str, timeout_ms: u32) -> AppResult<Value> {
    // 执行内部流程并收敛私有错误码。
    inspect_root_internal(session_id, timeout_ms).map_err(public_error)
}

// 对 canonical s2:w 执行公开有界树检查。
pub(crate) fn inspect_tree(
    // 接收 opaque 窗口目标。
    session_id: &str,
    // 接收深度上限。
    maximum_depth: usize,
    // 接收节点数量上限。
    maximum_items: usize,
    // 接收 view。
    view: &str,
    // 接收 worker deadline。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 执行内部流程并收敛私有错误码。
    inspect_tree_internal(
        // 转发目标。
        session_id,
        // 转发深度边界。
        maximum_depth,
        // 转发数量边界。
        maximum_items,
        // 转发 view。
        view,
        // 转发 deadline。
        timeout_ms,
    )
    // 映射内部错误。
    .map_err(public_error)
}

// 验证 worker envelope 与隐私字段门禁。
#[cfg(test)]
mod tests {
    // 导入待测函数。
    use super::*;

    // 构造成功 worker 输出。
    fn output(data: Value) -> WorkerOutput {
        // 返回零退出码 envelope。
        WorkerOutput {
            // 模拟成功退出。
            exit_code: 0,
            // 构造协议 envelope。
            envelope: json!({
                // 标记成功。
                "ok": true,
                // 指定协议版本。
                "contractVersion": WORKER_CONTRACT,
                // 注入 data。
                "data": data,
            }),
        }
    }

    // 验证 worker 错误必须有非零退出码。
    #[test]
    fn envelope_exit_code_must_match_ok_flag() {
        // 构造矛盾失败 envelope。
        let result = worker_data(WorkerOutput {
            // 错误地使用零退出码。
            exit_code: 0,
            // 输出失败 envelope。
            envelope: json!({
                // 标记失败。
                "ok": false,
                // 指定协议版本。
                "contractVersion": WORKER_CONTRACT,
                // 输出结构化错误。
                "error": { "code": "STALE_SESSION", "message": "stale" },
            }),
        });
        // 必须拒绝矛盾状态。
        assert_eq!(
            result.err().map(|error| error.code),
            Some("WORKER_PROTOCOL_FAILED")
        );
    }

    // 验证 worker 失败白名单在公开消息投影前失败闭合。
    #[test]
    fn worker_failure_codes_are_whitelisted_before_public_projection()
    -> Result<(), Box<dyn std::error::Error>> {
        // 构造允许的 provider 不可用失败 envelope。
        let known = worker_data(WorkerOutput {
            // 失败 envelope 必须使用非零退出码。
            exit_code: 1,
            // 输出已认证协议与安全消息。
            envelope: json!({
                // 标记失败。
                "ok": false,
                // 指定协议版本。
                "contractVersion": WORKER_CONTRACT,
                // 使用允许的 provider 缺口分类。
                "error": { "code": "ACCESSIBILITY_UNAVAILABLE", "message": "unavailable" },
            }),
        })
        // 提取预期的白名单失败。
        .err()
        // 把意外成功转换为明确测试错误。
        .ok_or_else(|| std::io::Error::other("known worker failure unexpectedly succeeded"))?;
        // provider 缺口必须转换为公共后台能力缺口。
        assert_eq!(known.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 允许的安全消息必须原样保留。
        assert_eq!(known.message, "unavailable");

        // 构造携带未知私有错误码的失败 envelope。
        let unknown = worker_data(WorkerOutput {
            // 失败 envelope 必须使用非零退出码。
            exit_code: 1,
            // 注入不允许穿透的私有错误码与消息。
            envelope: json!({
                // 标记失败。
                "ok": false,
                // 指定协议版本。
                "contractVersion": WORKER_CONTRACT,
                // 使用未知私有分类和敏感夹具消息。
                "error": { "code": "PRIVATE_NATIVE_FAILURE", "message": "native-secret" },
            }),
        })
        // 提取预期的协议失败。
        .err()
        // 把意外成功转换为明确测试错误。
        .ok_or_else(|| std::io::Error::other("unknown worker failure unexpectedly succeeded"))?;
        // 未知码必须失败闭合为稳定协议失败。
        assert_eq!(unknown.code, "WORKER_PROTOCOL_FAILED");
        // 未知 provider 消息不得穿透公开边界。
        assert_eq!(unknown.message, "Worker returned an unknown error code.");
        // 报告白名单与未知码门禁均通过。
        Ok(())
    }

    // 验证公开转换只处理 Accessibility Module 明确拥有的内部码。
    #[test]
    fn public_error_maps_owned_internal_codes_and_preserves_other_errors() {
        // 构造 worker 启动失败内部错误。
        let mapped = public_error(AppControlError::new("WORKER_START_FAILED", "worker start"));
        // 启动失败必须收敛为隔离 worker 不可用。
        assert_eq!(mapped.code, "ISOLATED_WORKER_UNAVAILABLE");
        // 安全内部消息必须保持不变。
        assert_eq!(mapped.message, "worker start");
        // 构造由目标会话所有者产生的稳定错误。
        let preserved = public_error(AppControlError::new("STALE_SESSION", "stale"));
        // 非当前 Module 转换范围的码必须原样传播。
        assert_eq!(preserved.code, "STALE_SESSION");
        // 原所有者消息必须保持不变。
        assert_eq!(preserved.message, "stale");
    }

    // 验证 root 额外 native 字段被拒绝。
    #[test]
    fn root_rejects_native_fields() {
        // 构造含 HWND 的 root。
        let data = json!({
            // 输出允许名称。
            "name": "fixture",
            // 输出允许 automation ID。
            "automationId": "id",
            // 输出允许类名。
            "className": "class",
            // 输出允许 framework ID。
            "frameworkId": "framework",
            // 输出允许 control type。
            "controlType": 50032,
            // 输出允许 enabled。
            "enabled": true,
            // 输出允许 offscreen。
            "offscreen": false,
            // 注入禁止字段。
            "hwnd": 123,
        });
        // 额外字段必须拒绝。
        assert_eq!(
            validate_root(&worker_data(output(data)).unwrap_or(Value::Null))
                // 取错误码。
                .err()
                // 映射稳定错误码。
                .map(|error| error.code),
            // 断言协议失败。
            Some("WORKER_PROTOCOL_FAILED")
        );
    }

    // 验证树节点添加 snapshot freshness 且不添加 native 字段。
    #[test]
    fn tree_node_projection_is_snapshot_scoped() {
        // 构造 canonical element ID。
        let node_id = crate::components::opaque_id::OpaqueTargetId::new(
            // 使用 element 类别。
            crate::components::opaque_id::OpaqueTargetKind::Element,
            // 使用稳定测试身份。
            "node",
        )
        // 转换为 canonical 字符串。
        .to_string();
        // 构造合法节点。
        let node = json!({
            // 输出 node ID。
            "nodeId": node_id,
            // 输出 depth。
            "depth": 0,
            // 输出 name。
            "name": "fixture",
            // 输出 automation ID。
            "automationId": "id",
            // 输出 class name。
            "className": "class",
            // 输出 framework ID。
            "frameworkId": "framework",
            // 输出 control type。
            "controlType": 50032,
            // 输出 enabled。
            "enabled": true,
            // 输出 offscreen。
            "offscreen": false,
            // 输出属性完整性。
            "propertyReadComplete": true,
        });
        // 验证并投影节点。
        let projected = validate_node(&node, 0).unwrap_or(Value::Null);
        // 必须添加快照生命周期。
        assert_eq!(projected["identityFreshness"], "inspection-snapshot");
        // 必须没有 bounds。
        assert!(projected.get("bounds").is_none());
        // 必须没有 Value。
        assert!(projected.get("value").is_none());
    }
}
