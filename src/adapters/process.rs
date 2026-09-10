//! 只读进程发现与精确元数据观察 adapter。

// 导入按 PID 附加窗口关系所需的映射。
use std::collections::HashMap;

// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 引入 Process Adapter 私有封闭错误码实现。
#[path = "process_error.rs"]
mod error_code;

// 导入 Process Adapter 私有封闭错误码。
use error_code::AppProcessErrorCode;

// 导入进程观察所需的 facade、registry、opaque 匹配与领域类型。
use crate::{
    // 导入只读 Windows inventory 与前景观察组件。
    adapters::{
        // 导入公共 adapter trait。
        AppAdapter,
        // 导入进程、窗口和 opaque 身份原语。
        windows::{
            ProcessInventory, ProcessRecord, enumerate_process_inventory, enumerate_windows,
            foreground_hwnd, opaque_process_session_id, opaque_window_session_id,
        },
    },
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入 fail-closed opaque 目标匹配组件。
    components::opaque_id::{OpaqueTargetMatch, match_opaque_target},
    // 导入结构化结果与请求类型。
    domain::{AppResult, CommandRequest},
};

// 声明只读进程 surface adapter。
pub struct ProcessAdapter;

// 把同一观察流程中的可见有标题窗口附加到进程记录。
fn attach_visible_windows(inventory: &mut ProcessInventory) -> AppResult<()> {
    // 建立私有 PID 到进程记录索引的映射。
    let by_process_id = inventory
        // 遍历当前进程快照。
        .records
        // 创建索引迭代器。
        .iter()
        // 保留记录位置与私有 PID。
        .enumerate()
        // 将 PID 映射到记录位置。
        .map(|(index, process)| (process.process_id, index))
        // 收集为当前函数私有映射。
        .collect::<HashMap<_, _>>();
    // 枚举当前顶层窗口快照；关系源失败时保留进程并标记不完整。
    let windows = match enumerate_windows() {
        // 保存可用窗口快照。
        Ok(windows) => windows,
        // 窗口关系源失败不得把空关系误报为完整。
        Err(_) => {
            // 传播组合清单不完整状态。
            inventory.complete = false;
            // 保留已发现进程供调用方按 complete 判定。
            return Ok(());
        }
    };
    // 关联当前顶层窗口快照。
    for window in windows {
        // process observation 只关联可见且有标题的顶层窗口。
        if !window.visible || window.title.is_empty() {
            // 跳过不可见或无标题窗口。
            continue;
        }
        // 查找窗口所属进程记录。
        let Some(index) = by_process_id.get(&window.process_id).copied() else {
            // 当前进程快照没有对应记录时不猜测关联。
            continue;
        };
        // 生成 canonical s2:w 关系目标。
        let window_session_id = opaque_window_session_id(&window);
        // 将公开窗口关系附加到对应进程。
        inventory.records[index]
            // 访问窗口关系集合。
            .window_session_ids
            // 保存 opaque 目标。
            .push(window_session_id);
    }
    // 为每个进程规范化窗口关系顺序并去重。
    for process in &mut inventory.records {
        // 排序保证输出稳定。
        process.window_session_ids.sort();
        // 去除可能的重复窗口身份。
        process.window_session_ids.dedup();
    }
    // 完成关系附加。
    Ok(())
}

// 捕获一次进程与窗口关系观察。
fn capture_process_inventory() -> AppResult<ProcessInventory> {
    // 先完整枚举进程，截断由公开 adapter 在过滤后处理。
    let mut inventory = enumerate_process_inventory(usize::MAX)?;
    // 附加同一调用期间重新生成的 opaque 窗口关系。
    attach_visible_windows(&mut inventory)?;
    // 返回组合后的观察清单。
    Ok(inventory)
}

// 将内部进程记录投影为 C++ 等价的隐私安全公开对象。
fn public_process_observation(process: &ProcessRecord) -> Value {
    // 只序列化语言中立观察字段。
    json!({
        // 输出 canonical s2:p。
        "sessionId": process.session_id,
        // 输出稳定目标类别。
        "targetKind": "running-process",
        // 只输出 ToolHelp 公开进程名。
        "processName": process.process_name,
        // 当前快照中的进程状态固定为运行中。
        "state": "running",
        // 根据创建时间可用性声明身份新鲜度。
        "identityFreshness": if process.identity_reliable {
            // 创建时间可读时绑定进程生命周期。
            "process-lifetime"
        } else {
            // 权限或退出缺口时仅保证当前快照 best effort。
            "best-effort-current-snapshot"
        },
        // 输出结构化元数据访问分类。
        "metadataAccess": process.metadata_access.as_str(),
        // 只输出相对完整性，不公开任何 RID。
        "integrityRelation": process.integrity_relation.as_str(),
        // 根据 opaque 窗口关系判断是否有可见窗口。
        "hasVisibleWindow": !process.window_session_ids.is_empty(),
        // 输出封闭窗口可见性分类。
        "windowVisibility": if process.window_session_ids.is_empty() {
            // 当前没有可见有标题窗口。
            "no-visible-titled-window"
        } else {
            // 当前至少有一个可见有标题窗口。
            "visible-titled-window"
        },
        // 只读 ToolHelp/Win32 查询不需要前台。
        "foregroundRequiredForObservation": false,
        // 只输出 canonical s2:w 关系。
        "windowSessionIds": process.window_session_ids,
    })
}

// 读取 inspect 请求中的 canonical sessionId。
fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    // 从目标对象读取非空字符串。
    request
        // 访问目标字段。
        .target
        // 读取 sessionId。
        .get("sessionId")
        // 要求字符串类型。
        .and_then(Value::as_str)
        // 拒绝空目标。
        .filter(|value| !value.is_empty())
        // 缺失时返回稳定参数错误。
        .ok_or_else(|| {
            // 由 Process Adapter 私有类型选择参数错误码。
            AppProcessErrorCode::InvalidArgument.error("target.sessionId is required.")
        })
}

// 在当前进程 inventory 中唯一重新解析 canonical s2:p。
fn resolve_process<'inventory>(
    session_id: &str,
    records: &'inventory [ProcessRecord],
) -> AppResult<&'inventory ProcessRecord> {
    // 使用私有 PID、创建时间和名称重新生成每个候选目标。
    match match_opaque_target(session_id, records, |process| {
        // 返回从当前私有事实生成的 canonical s2:p。
        Some(opaque_process_session_id(
            process.process_id,
            process.process_creation_time,
            &process.process_name,
        ))
    }) {
        // 唯一命中后才返回当前记录。
        OpaqueTargetMatch::Unique(process) => Ok(process),
        // 零命中表示目标过期或输入非 canonical。
        OpaqueTargetMatch::Missing => Err(AppProcessErrorCode::StaleSession.error(
            // 明确进程目标已不存在或代际变化。
            "The running process target no longer exists.",
        )),
        // 指纹碰撞必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(AppProcessErrorCode::AmbiguousTarget.error(
            // 明确不会任取一个进程。
            "The opaque process session matched more than one process.",
        )),
    }
}

// 检查只读观察期间前景窗口未变化。
fn ensure_foreground_unchanged(before: isize, after: isize) -> AppResult<()> {
    // 相同前景直接成功。
    if before == after {
        // 返回无副作用结果。
        return Ok(());
    }
    // 前景变化必须结构化失败而不是隐藏 host interference。
    Err(AppProcessErrorCode::HostInterferenceDetected.error(
        // 明确进程观察不应改变前景。
        "Foreground changed during process observation.",
    ))
}

// 实现进程观察 surface。
impl AppAdapter for ProcessAdapter {
    // 返回 legacy surface ID。
    fn app_id(&self) -> &'static str {
        // 保持 sessions process / inspect process 入口。
        "process"
    }

    // 报告只读进程观察能力。
    fn status(&self) -> AppResult<Value> {
        // 输出后台与 capability 状态。
        Ok(json!({
            // 标记调用成功。
            "ok": true,
            // 保持 legacy surface ID。
            "app": self.app_id(),
            // 声明只读 ToolHelp backend。
            "backend": "Win32 ToolHelp process snapshot",
            // 声明保证后台执行。
            "backgroundPolicy": "guaranteed",
            // 标记只读。
            "readOnly": true,
            // 公布已迁回 Rust 的版本化 capability。
            "capabilities": [
                // 进程发现。
                capabilities::PROCESS_DISCOVER,
                // 精确元数据读取。
                capabilities::PROCESS_METADATA_READ,
            ],
        }))
    }

    // 枚举版本化进程观察结果。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 新观察 surface 不接受原生 PID 过滤。
        if request.target.contains_key("processId") {
            // 返回结构化边界错误。
            return Err(AppProcessErrorCode::InvalidArgument.error(
                // 引导调用方使用公开名称过滤或 opaque inspect。
                "processId is private; filter by name or inspect an opaque sessionId.",
            ));
        }
        // 记录观察前前景窗口。
        let before = foreground_hwnd();
        // 捕获完整进程与窗口关系。
        let mut inventory = capture_process_inventory()?;
        // 兼容只读名称过滤，不接受路径或 PID。
        if let Some(name) = request.target.get("name").and_then(Value::as_str) {
            // 使用 Windows 文件名不区分大小写比较。
            inventory
                // 访问进程记录。
                .records
                // 只保留名称匹配项。
                .retain(|process| process.process_name.eq_ignore_ascii_case(name));
        }
        // 记录过滤后的总数。
        let total = inventory.records.len();
        // 按调用方有界输出截断。
        inventory.records.truncate(request.max_items);
        // 投影隐私安全公开对象。
        let sessions = inventory
            // 消费当前记录。
            .records
            // 创建迭代器。
            .iter()
            // 转换为公开 JSON。
            .map(public_process_observation)
            // 收集结果数组。
            .collect::<Vec<_>>();
        // 记录观察后前景窗口。
        let after = foreground_hwnd();
        // 前景变化必须失败。
        ensure_foreground_unchanged(before, after)?;
        // 输出 process-observation schema 数据。
        Ok(json!({
            // 标识版本化发现 capability。
            "capability": capabilities::PROCESS_DISCOVER,
            // 标记只读。
            "readOnly": true,
            // 声明 host-headless 执行域。
            "executionDomain": "host-headless",
            // 输出本次返回数量。
            "count": sessions.len(),
            // 输出过滤后的总数。
            "total": total,
            // 标记公开输出是否截断。
            "truncated": total > sessions.len(),
            // 传播 ToolHelp 快照完整性。
            "complete": inventory.complete,
            // 成功路径保证前景不变。
            "foregroundUnchanged": true,
            // 输出隐私安全进程观察。
            "sessions": sessions,
        }))
    }

    // 对 canonical s2:p 执行使用时重新发现的元数据读取。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 先读取调用方 opaque 目标。
        let session_id = required_session_id(request)?;
        // 记录观察前前景窗口。
        let before = foreground_hwnd();
        // 每次使用都重新捕获当前进程清单。
        let inventory = capture_process_inventory()?;
        // 要求当前 inventory 唯一命中。
        let process = resolve_process(session_id, &inventory.records)?;
        // 投影命中进程的公开元数据。
        let process = public_process_observation(process);
        // 记录观察后前景窗口。
        let after = foreground_hwnd();
        // 前景变化必须失败。
        ensure_foreground_unchanged(before, after)?;
        // 输出精确 metadata read 结果。
        Ok(json!({
            // 标识版本化读取 capability。
            "capability": capabilities::PROCESS_METADATA_READ,
            // 标记只读。
            "readOnly": true,
            // 声明 host-headless 执行域。
            "executionDomain": "host-headless",
            // 成功路径保证前景不变。
            "foregroundUnchanged": true,
            // 输出唯一当前进程观察。
            "process": process,
        }))
    }

    // 进程 surface 永不提供写操作。
    fn run(&self, _: &CommandRequest) -> AppResult<Value> {
        // 返回稳定后台能力缺口。
        Err(AppProcessErrorCode::BackgroundOperationUnavailable.error(
            // 明确该 surface 只读。
            "process 不提供后台写操作。",
        ))
    }
}

// 声明无需外部 fixture 的进程观察契约测试。
#[cfg(test)]
mod tests {
    // 导入父模块全部纯辅助函数与类型。
    use super::*;
    // 导入仅用于构造契约夹具的进程枚举类型。
    use crate::adapters::windows::{IntegrityRelation, ProcessMetadataAccess};
    // 导入请求 verb。
    use crate::domain::Verb;
    // 导入公共 catalog 策略以防 inspect 路由回归。
    use crate::policy;

    // 构造稳定进程观察夹具。
    fn fixture_process() -> ProcessRecord {
        // 返回与 C++ identity 顺序一致的记录。
        ProcessRecord {
            // 使用独立计算的 FNV-1a golden。
            session_id: "s2:p:56b8e8da48e6cbb4".to_owned(),
            // 保存公开安全名称。
            process_name: "fixture.exe".to_owned(),
            // 标记创建时间可靠。
            identity_reliable: true,
            // 标记元数据可用。
            metadata_access: ProcessMetadataAccess::Available,
            // 标记相同完整性。
            integrity_relation: IntegrityRelation::Same,
            // 提供一个 opaque 窗口关系。
            window_session_ids: vec!["s2:w:0123456789abcdef".to_owned()],
            // 保存私有 PID。
            process_id: 42,
            // 保存私有创建时间。
            process_creation_time: 123,
        }
    }

    // 验证 Rust 与 C++ 使用相同进程身份布局。
    #[test]
    fn process_identity_matches_cpp_golden() {
        // 按 PID、FILETIME、名称生成 canonical 目标。
        let session_id = opaque_process_session_id(42, 123, "fixture.exe");
        // 对照独立 FNV-1a golden。
        assert_eq!(session_id, "s2:p:56b8e8da48e6cbb4");
        // 创建时间变化必须产生不同代际目标。
        assert_ne!(
            session_id,
            opaque_process_session_id(42, 124, "fixture.exe")
        );
    }

    // 验证进程重新解析保留 stale 与 ambiguous 的 fail-closed 语义。
    #[test]
    fn process_resolution_is_missing_unique_or_ambiguous() {
        // 构造唯一当前记录。
        let process = fixture_process();
        // 唯一目标必须解析到夹具。
        let resolved = match resolve_process(&process.session_id, std::slice::from_ref(&process)) {
            // 保存唯一命中。
            Ok(record) => record,
            // 唯一夹具不得失败。
            Err(error) => panic!("unique process fixture failed: {}", error.message),
        };
        // 验证唯一命中名称。
        assert_eq!(resolved.process_name, "fixture.exe");
        // 非 canonical 旧目标必须视为 stale。
        let stale = resolve_process("process:42", std::slice::from_ref(&process));
        // 提取 stale 错误供完整契约核对。
        let stale = stale
            // 成功表示旧目标门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("legacy process target must be stale"));
        // 验证 stale 错误码。
        assert_eq!(stale.code, "STALE_SESSION");
        // 验证既有 stale 消息逐字不变。
        assert_eq!(
            stale.message,
            "The running process target no longer exists."
        );
        // 构造两个生成同一 canonical 目标的碰撞候选。
        let duplicates = [process.clone(), process];
        // 多命中必须 fail closed。
        let ambiguous = resolve_process("s2:p:56b8e8da48e6cbb4", &duplicates);
        // 提取歧义错误供完整契约核对。
        let ambiguous = ambiguous
            // 成功表示唯一匹配门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("duplicate process target must be ambiguous"));
        // 验证歧义错误码。
        assert_eq!(ambiguous.code, "AMBIGUOUS_TARGET");
        // 验证既有歧义消息逐字不变。
        assert_eq!(
            ambiguous.message,
            "The opaque process session matched more than one process."
        );
    }

    // 验证公开对象完整且不泄漏私有进程事实。
    #[test]
    fn public_process_observation_is_schema_shaped_and_private() {
        // 投影稳定夹具。
        let value = public_process_observation(&fixture_process());
        // 收集公开对象字段名。
        let mut keys = value
            // 要求对象形状。
            .as_object()
            // 测试夹具固定为对象。
            .into_iter()
            // 展开字段迭代器。
            .flat_map(|object| object.keys())
            // 复制字段名。
            .map(String::as_str)
            // 收集用于规范比较。
            .collect::<Vec<_>>();
        // 排序消除 serde_json map 实现差异。
        keys.sort_unstable();
        // 与 process-observation schema 的 additionalProperties=false 字段集对齐。
        assert_eq!(
            keys,
            vec![
                "foregroundRequiredForObservation",
                "hasVisibleWindow",
                "identityFreshness",
                "integrityRelation",
                "metadataAccess",
                "processName",
                "sessionId",
                "state",
                "targetKind",
                "windowSessionIds",
                "windowVisibility",
            ]
        );
        // 验证目标类别与生命周期。
        assert_eq!(value["targetKind"], "running-process");
        // 验证元数据分类。
        assert_eq!(value["metadataAccess"], "available");
        // 验证相对完整性。
        assert_eq!(value["integrityRelation"], "same");
        // 验证窗口关系。
        assert_eq!(value["hasVisibleWindow"], true);
        // 序列化用于禁止字段扫描。
        let text = value.to_string();
        // 禁止原生 PID 字段。
        assert!(!text.contains("processId"));
        // 禁止原生创建时间。
        assert!(!text.contains("creationTime"));
        // 禁止 token 与 SID。
        assert!(!text.contains("token") && !text.contains("sid"));
        // 禁止完整性 RID。
        assert!(!text.contains("rid"));
    }

    // 验证权限阻塞仍返回 best-effort 身份与未知相对关系。
    #[test]
    fn permission_blocked_process_is_explicit() {
        // 构造权限阻塞记录。
        let mut process = fixture_process();
        // 标记创建时间不可读。
        process.identity_reliable = false;
        // 标记权限阻塞。
        process.metadata_access = ProcessMetadataAccess::PermissionBlocked;
        // 标记相对关系未知。
        process.integrity_relation = IntegrityRelation::Unknown;
        // 投影公开对象。
        let value = public_process_observation(&process);
        // 验证访问分类。
        assert_eq!(value["metadataAccess"], "permission-blocked");
        // 验证身份新鲜度降级但不泄漏原生事实。
        assert_eq!(value["identityFreshness"], "best-effort-current-snapshot");
        // 验证相对关系未知。
        assert_eq!(value["integrityRelation"], "unknown");
    }

    // 验证 public sessions 拒绝原生 PID 过滤。
    #[test]
    fn process_sessions_reject_native_process_id_filter() {
        // 构造只读 sessions 请求。
        let mut request = CommandRequest::read(Verb::Sessions, "process");
        // 注入被禁止的原生 PID。
        request.target.insert("processId".to_owned(), json!(42));
        // 执行前置边界检查。
        let error = match ProcessAdapter.sessions(&request) {
            // 禁止误报成功。
            Ok(_) => panic!("native processId must be rejected"),
            // 保存结构化错误。
            Err(error) => error,
        };
        // 验证稳定参数错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 验证既有隐私边界消息逐字不变。
        assert_eq!(
            error.message,
            "processId is private; filter by name or inspect an opaque sessionId."
        );
    }

    // 验证参数、前景与只读门禁无需真实 mutation 即保持稳定错误。
    #[test]
    fn process_owned_gates_keep_stable_errors_without_mutation() {
        // 构造不含精确目标的 inspect 请求。
        let inspect_request = CommandRequest::read(Verb::Inspect, "process");
        // 缺失目标必须在 inventory 访问前失败。
        let missing_target = required_session_id(&inspect_request)
            // 成功表示参数门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing process target must fail"));
        // 保持稳定参数错误码。
        assert_eq!(missing_target.code, "INVALID_ARGUMENT");
        // 保持既有目标缺失消息。
        assert_eq!(missing_target.message, "target.sessionId is required.");

        // 使用合成句柄变化验证前景保护纯函数。
        let interference = ensure_foreground_unchanged(1, 2)
            // 前景变化不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("foreground change must fail"));
        // 保持稳定宿主干扰错误码。
        assert_eq!(interference.code, "HOST_INTERFERENCE_DETECTED");
        // 保持既有宿主干扰消息。
        assert_eq!(
            interference.message,
            "Foreground changed during process observation."
        );

        // 构造不含副作用参数的 run 请求。
        let run_request = CommandRequest::read(Verb::Run, "process");
        // 只读 surface 必须在任何 mutation 前拒绝。
        let unavailable = ProcessAdapter
            // 调用固定只读门禁。
            .run(&run_request)
            // 写操作不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("process mutation must remain unavailable"));
        // 保持稳定后台操作缺口码。
        assert_eq!(unavailable.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持既有只读消息。
        assert_eq!(unavailable.message, "process 不提供后台写操作。");
    }

    // 验证公共 catalog 允许 process inspect 到达 adapter。
    #[test]
    fn process_catalog_allows_opaque_metadata_inspect() -> AppResult<()> {
        // 构造精确只读 inspect 请求。
        let mut request = CommandRequest::read(Verb::Inspect, "process");
        // 提供 canonical 进程目标。
        request.target.insert(
            // 写入公开 sessionId 字段。
            "sessionId".to_owned(),
            // 使用稳定夹具目标。
            json!("s2:p:56b8e8da48e6cbb4"),
        );
        // catalog/policy 必须允许请求进入 adapter 的 stale/unique 门禁。
        policy::validate(&request)?;
        // 完成 catalog 回归测试。
        Ok(())
    }

    // 验证实时 Rust sessions 输出版本化且不泄漏 PID。
    #[test]
    fn live_process_sessions_are_versioned_and_private() -> AppResult<()> {
        // 构造有界只读请求。
        let mut request = CommandRequest::read(Verb::Sessions, "process");
        // 限制测试输出规模。
        request.max_items = 8;
        // 执行实时 ToolHelp 观察。
        let value = ProcessAdapter.sessions(&request)?;
        // 收集顶层 schema 字段。
        let mut keys = value
            // 要求对象结果。
            .as_object()
            // 展开对象字段。
            .into_iter()
            // 迭代字段名。
            .flat_map(|object| object.keys())
            // 转换为字符串切片。
            .map(String::as_str)
            // 收集规范比较列表。
            .collect::<Vec<_>>();
        // 排序保证稳定。
        keys.sort_unstable();
        // 与 process-observation schema 顶层字段集对齐。
        assert_eq!(
            keys,
            vec![
                "capability",
                "complete",
                "count",
                "executionDomain",
                "foregroundUnchanged",
                "readOnly",
                "sessions",
                "total",
                "truncated",
            ]
        );
        // 验证 capability ID。
        assert_eq!(value["capability"], capabilities::PROCESS_DISCOVER);
        // 验证 host-headless 执行域。
        assert_eq!(value["executionDomain"], "host-headless");
        // 验证返回边界。
        assert!(value["count"].as_u64().is_some_and(|count| count <= 8));
        // 扫描全部公开 JSON。
        let text = value.to_string();
        // 禁止 PID 与路径字段。
        assert!(!text.contains("processId") && !text.contains("path"));
        // 验证所有目标使用 canonical s2:p。
        for session in value["sessions"].as_array().into_iter().flatten() {
            // 读取 session ID。
            let session_id = session["sessionId"].as_str().unwrap_or_default();
            // 要求 canonical 进程目标前缀。
            assert!(session_id.starts_with("s2:p:"));
        }
        // 完成实时观察测试。
        Ok(())
    }
}
