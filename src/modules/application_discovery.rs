//! 只读 host 到应用、进程与窗口关系图 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "application_discovery_error.rs"]
mod error_code;

// 导入稳定关系映射集合。
use std::collections::HashMap;

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入窄 Windows Component、名称规范化与结构化错误。
use crate::{
    // 只组合只读来源，不直接执行领域外写操作。
    adapters::{
        // 读取传统卸载源与公开 Shell AppsFolder。
        installed_applications::{
            InstalledApplicationInventory, InstalledApplicationRecord,
            enumerate_installed_applications,
        },
        // 读取进程、窗口、主机与前景事实。
        windows::{
            IntegrityRelation, ProcessInventory, ProcessMetadataAccess, ProcessRecord,
            WindowRecord, current_host_session_id, enumerate_process_inventory, enumerate_windows,
            foreground_hwnd, opaque_window_session_id,
        },
    },
    // 使用与 C++ 对照一致的 ASCII 保守规范化规则。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind, normalized_name},
    // 返回语言中立结构化结果。
    domain::AppResult,
};

// 导入当前 Module 私有封闭错误码。
use error_code::ApplicationDiscoveryErrorCode;

// 复用 application.open 的单一动态认证条件。
use super::application_launch::is_launchable;

// 固定发现纵切允许的最大单类记录数。
const MAXIMUM_DISCOVERY_ITEMS: usize = 4096;

// 表示 assess 重新发现后可安全公开的目标可用性分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AssessmentAvailability {
    // 表示只读事实足以继续进行 capability 决策。
    Available,
    // 表示 Windows 拒绝了目标元数据读取。
    PermissionBlocked,
    // 表示目标存在，但当前 provider 事实不完整。
    Unavailable,
}

// 保存 assess 所需的最小 provider-neutral 目标事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AssessmentTarget {
    // 保存重新解析出的稳定目标类别。
    pub(crate) kind: OpaqueTargetKind,
    // 保存目标当前只读可用性。
    pub(crate) availability: AssessmentAvailability,
}

// 保存一次同调用只读快照中的全部私有与公开事实。
struct InventorySnapshot {
    // 保存 canonical 主机目标。
    host_session_id: String,
    // 保存已安装应用事实。
    applications: Vec<InstalledApplicationRecord>,
    // 保存运行进程事实。
    processes: Vec<ProcessRecord>,
    // 保存可见有标题窗口事实。
    windows: Vec<WindowRecord>,
    // 标记传统卸载注册源可访问。
    registry_source_available: bool,
    // 标记公开 Shell AppsFolder 可访问。
    shell_source_available: bool,
    // 标记固定 Start Menu Programs 来源可访问。
    start_menu_source_available: bool,
    // 标记应用来源自然完整且未截断。
    applications_complete: bool,
    // 标记进程来源自然完整且未截断。
    processes_complete: bool,
    // 标记窗口来源未触及边界。
    windows_complete: bool,
    // 标记观察期间前景未变化。
    foreground_unchanged: bool,
}

// 保存应用到进程及进程到应用的双向保守关系。
#[derive(Default)]
struct Relations {
    // 保存应用目标到运行进程目标集合。
    application_to_process: HashMap<String, Vec<String>>,
    // 保存进程目标到已安装应用目标集合。
    process_to_application: HashMap<String, Vec<String>>,
}

// 验证单类发现上限处于公开契约边界。
fn validate_limit(value: usize, name: &str) -> AppResult<()> {
    // 只接受 1..4096，避免无界枚举或空边界歧义。
    if !(1..=MAXIMUM_DISCOVERY_ITEMS).contains(&value) {
        // 返回稳定参数错误且不接触系统来源。
        return Err(ApplicationDiscoveryErrorCode::InvalidArgument.error(
            // 明确指出失败选项与允许边界。
            format!("{name} must be within 1..={MAXIMUM_DISCOVERY_ITEMS}."),
        ));
    }
    // 合法边界继续执行。
    Ok(())
}

// 从进程文件名生成与 C++ 对照一致的保守精确提示。
fn process_stem(process_name: &str) -> String {
    // 仅删除最后一个扩展名片段，不解析或公开路径。
    let stem = process_name
        // 查找最后一个点号。
        .rsplit_once('.')
        // 有扩展名时保留点号前部分。
        .map_or(process_name, |(stem, _)| stem);
    // 只保留 ASCII 字母数字并转小写。
    normalized_name(stem)
}

// 给进程附加同一快照内的 canonical 可见窗口关系。
fn attach_windows(processes: &mut ProcessInventory, windows: &[WindowRecord]) {
    // 建立当前快照私有 PID 到进程记录位置的映射。
    let process_by_native_id = processes
        // 遍历进程记录。
        .records
        // 取得迭代器。
        .iter()
        // 保留位置。
        .enumerate()
        // 映射私有 PID。
        .map(|(index, process)| (process.process_id, index))
        // 收集到函数私有映射。
        .collect::<HashMap<_, _>>();
    // 遍历可发布窗口事实。
    for window in windows {
        // 找到同 PID 的当前进程记录。
        let Some(index) = process_by_native_id.get(&window.process_id).copied() else {
            // 缺失进程时保持窗口关系为空，不猜测。
            continue;
        };
        // 只附加从私有窗口事实重新生成的 opaque 目标。
        processes.records[index]
            // 访问关系集合。
            .window_session_ids
            // 保存 canonical s2:w。
            .push(opaque_window_session_id(window));
    }
    // 规范化全部进程关系顺序。
    for process in &mut processes.records {
        // 稳定排序。
        process.window_session_ids.sort();
        // 去除重复窗口关系。
        process.window_session_ids.dedup();
    }
}

// 捕获一次有界、只读、前景不变的关系图事实快照。
fn capture(
    // 接收应用上限。
    maximum_applications: usize,
    // 接收进程上限。
    maximum_processes: usize,
    // 接收窗口上限。
    maximum_windows: usize,
) -> AppResult<InventorySnapshot> {
    // 记录任何只读来源调用前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 枚举并合并已安装应用来源。
    let applications = enumerate_installed_applications(maximum_applications);
    // 枚举有界 ToolHelp 进程事实。
    let mut processes = enumerate_process_inventory(maximum_processes)?;
    // 枚举当前顶层窗口事实。
    let mut windows = enumerate_windows()?;
    // 只保留契约允许发布的可见有标题窗口。
    windows.retain(|window| window.visible && !window.title.is_empty());
    // 在截断前保存窗口总量。
    let visible_window_count = windows.len();
    // 对窗口应用调用方边界。
    windows.truncate(maximum_windows);
    // 将窗口关系附加到同一快照中的进程。
    attach_windows(&mut processes, &windows);
    // 记录只读观察后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 解构应用清单以保留逐源覆盖信息。
    let InstalledApplicationInventory {
        // 移交应用记录。
        records,
        // 保存 Registry 可用性。
        registry_source_available,
        // 保存 Shell 可用性。
        shell_source_available,
        // 保存固定 Start Menu 可用性。
        start_menu_source_available,
        // 保存应用完整性。
        complete: applications_complete,
    } = applications;
    // 返回组合快照。
    Ok(InventorySnapshot {
        // 从当前登录会话生成 canonical host 目标。
        host_session_id: current_host_session_id(),
        // 保存应用记录。
        applications: records,
        // 保存进程记录。
        processes: processes.records,
        // 保存窗口记录。
        windows,
        // 保存 Registry 覆盖。
        registry_source_available,
        // 保存 Shell 覆盖。
        shell_source_available,
        // 保存固定 Start Menu 覆盖。
        start_menu_source_available,
        // 保存应用完整性。
        applications_complete,
        // 保存进程完整性。
        processes_complete: processes.complete,
        // 与 C++ 对照一致，触及上限时不能证明窗口完整。
        windows_complete: visible_window_count < maximum_windows,
        // 只比较 opaque 私有前景令牌，不公开 HWND。
        foreground_unchanged: foreground_before == foreground_after,
    })
}

// 从同一快照建立保守精确名称关系。
fn build_relations(snapshot: &InventorySnapshot) -> Relations {
    // 按规范化进程 stem 建立运行进程索引。
    let mut processes_by_hint = HashMap::<String, Vec<String>>::new();
    // 遍历全部有界进程。
    for process in &snapshot.processes {
        // 生成私有匹配提示。
        let hint = process_stem(&process.process_name);
        // 空提示禁止关联。
        if hint.is_empty() {
            // 继续下一进程。
            continue;
        }
        // 保存同提示的全部进程实例。
        processes_by_hint
            // 取得或创建集合。
            .entry(hint)
            // 创建空集合。
            .or_default()
            // 添加 opaque 进程目标。
            .push(process.session_id.clone());
    }
    // 初始化双向关系。
    let mut relations = Relations::default();
    // 遍历全部已安装应用。
    for application in &snapshot.applications {
        // 遍历 Registry/Shell 推导出的私有提示。
        for hint in &application.process_match_hints {
            // 查找精确提示命中的进程集合。
            let Some(process_ids) = processes_by_hint.get(hint) else {
                // 没有精确命中时不猜测。
                continue;
            };
            // 为每个运行实例建立双向 opaque 关系。
            for process_id in process_ids {
                // 保存应用到进程关系。
                relations
                    // 取得应用集合。
                    .application_to_process
                    // 按 opaque 应用目标索引。
                    .entry(application.session_id.clone())
                    // 创建空集合。
                    .or_default()
                    // 保存 opaque 进程目标。
                    .push(process_id.clone());
                // 保存进程到应用关系。
                relations
                    // 取得进程集合。
                    .process_to_application
                    // 按 opaque 进程目标索引。
                    .entry(process_id.clone())
                    // 创建空集合。
                    .or_default()
                    // 保存 opaque 应用目标。
                    .push(application.session_id.clone());
            }
        }
    }
    // 规范化双向关系，避免来源顺序影响输出。
    for values in relations
        // 链接两侧可变集合。
        .application_to_process
        // 遍历应用侧集合。
        .values_mut()
        // 追加进程侧集合。
        .chain(relations.process_to_application.values_mut())
    {
        // 稳定排序。
        values.sort();
        // 去除重复提示形成的重复关系。
        values.dedup();
    }
    // 返回保守关系。
    relations
}

// 投影完整进程观察并附加应用关系。
fn public_process(process: &ProcessRecord, related_application_ids: &[String]) -> Value {
    // 只构造 schema 允许的语言中立字段。
    json!({
        // 输出 canonical s2:p。
        "sessionId": process.session_id,
        // 输出稳定类别。
        "targetKind": "running-process",
        // 只输出可执行文件名。
        "processName": process.process_name,
        // 当前快照记录固定为运行中。
        "state": "running",
        // 创建时间可用时绑定进程生命周期。
        "identityFreshness": if process.identity_reliable {
            // 可靠生命周期。
            "process-lifetime"
        } else {
            // 权限或退出导致的当前快照身份。
            "best-effort-current-snapshot"
        },
        // 输出封闭元数据访问分类。
        "metadataAccess": process.metadata_access.as_str(),
        // 只输出相对完整性，不公开 RID。
        "integrityRelation": process.integrity_relation.as_str(),
        // 从 opaque 关系判断当前窗口可见性。
        "hasVisibleWindow": !process.window_session_ids.is_empty(),
        // 输出封闭窗口分类。
        "windowVisibility": if process.window_session_ids.is_empty() {
            // 当前没有可见有标题窗口。
            "no-visible-titled-window"
        } else {
            // 当前至少一个可见有标题窗口。
            "visible-titled-window"
        },
        // 只读观察不需要前景。
        "foregroundRequiredForObservation": false,
        // 输出 canonical 窗口关系。
        "windowSessionIds": process.window_session_ids,
        // 输出 canonical 应用关系。
        "relatedApplicationIds": related_application_ids,
        // 无关系时显式标记，不猜测。
        "relationshipStatus": if related_application_ids.is_empty() {
            // 无保守证据。
            "unassociated"
        } else {
            // 至少一个精确提示命中。
            "matched"
        },
    })
}

// 将私有快照投影为 application-inventory 契约数据。
fn render(snapshot: InventorySnapshot) -> AppResult<Value> {
    // 前景发生变化时禁止把受干扰快照作为成功返回。
    if !snapshot.foreground_unchanged {
        // 返回稳定干扰错误。
        return Err(
            ApplicationDiscoveryErrorCode::HostInterferenceDetected.error(
                // 描述只读观察期间的前景变化。
                "The foreground target changed during read-only application inventory discovery.",
            ),
        );
    }
    // 从同一快照建立全部关系。
    let relations = build_relations(&snapshot);
    // 统计运行中的已安装应用。
    let mut running_application_count = 0_usize;
    // 投影应用集合。
    let applications = snapshot
        // 遍历应用记录。
        .applications
        // 取得迭代器。
        .iter()
        // 构造公开对象。
        .map(|application| {
            // 查找应用的全部运行进程。
            let process_ids = relations
                // 访问应用侧关系。
                .application_to_process
                // 按 opaque 应用目标查找。
                .get(&application.session_id)
                // 转为切片。
                .map_or(&[][..], Vec::as_slice);
            // 非空关系表示当前运行。
            if !process_ids.is_empty() {
                // 增加运行应用计数。
                running_application_count += 1;
            }
            // 只公开 schema 字段。
            json!({
                // 输出 canonical s2:a。
                "sessionId": application.session_id,
                // 输出稳定类别。
                "targetKind": "installed-application",
                // 输出 Registry/Shell 显示名。
                "displayName": application.display_name,
                // 输出可用公开版本。
                "version": application.version,
                // 输出可用公开发布者。
                "publisher": application.publisher,
                // 根据精确关系声明当前状态。
                "state": if process_ids.is_empty() { "installed" } else { "running" },
                // 输出 canonical 进程关系。
                "runningProcessSessionIds": process_ids,
                // 输出公开来源，不公开 Registry 路径或 AUMID。
                "discoverySources": application.discovery_sources,
                // 只有公开 Shell parsing identity 存在时声明未认证候选。
                "launchCapability": if is_launchable(application) {
                    // 精确 Shell 目标发布逐操作确认型启动能力。
                    "available-confirmed"
                } else {
                    // 没有认证 Shell identity 时保持不可用。
                    "unavailable"
                },
                // 只声明精确提示证据。
                "relationshipEvidence": if process_ids.is_empty() {
                    // 当前无关系证据。
                    "none"
                } else {
                    // 精确规范化名称或图标文件名。
                    "exact-normalized-name"
                },
            })
        })
        // 收集稳定数组。
        .collect::<Vec<_>>();
    // 初始化进程统计。
    let mut no_window_process_count = 0_usize;
    // 初始化无关联统计。
    let mut unassociated_process_count = 0_usize;
    // 初始化权限阻塞统计。
    let mut permission_blocked_process_count = 0_usize;
    // 初始化高完整性统计。
    let mut higher_integrity_process_count = 0_usize;
    // 投影进程集合。
    let processes = snapshot
        // 遍历进程记录。
        .processes
        // 取得迭代器。
        .iter()
        // 构造公开对象并累计统计。
        .map(|process| {
            // 无窗口关系时增加计数。
            if process.window_session_ids.is_empty() {
                // 增加无窗口进程计数。
                no_window_process_count += 1;
            }
            // 读取进程到应用关系。
            let application_ids = relations
                // 访问进程侧关系。
                .process_to_application
                // 按 opaque 进程目标查找。
                .get(&process.session_id)
                // 转为切片。
                .map_or(&[][..], Vec::as_slice);
            // 无关系时显式计数。
            if application_ids.is_empty() {
                // 增加未关联进程计数。
                unassociated_process_count += 1;
            }
            // 权限阻塞保持可观察计数。
            if process.metadata_access == ProcessMetadataAccess::PermissionBlocked {
                // 增加权限阻塞进程计数。
                permission_blocked_process_count += 1;
            }
            // 高完整性只做相对统计。
            if process.integrity_relation == IntegrityRelation::Higher {
                // 增加高完整性进程计数。
                higher_integrity_process_count += 1;
            }
            // 投影公开进程对象。
            public_process(process, application_ids)
        })
        // 收集公开数组。
        .collect::<Vec<_>>();
    // 建立私有 PID 到 canonical 进程目标映射。
    let process_session_by_native = snapshot
        // 遍历进程记录。
        .processes
        // 取得迭代器。
        .iter()
        // 仅在函数内映射私有 PID。
        .map(|process| (process.process_id, process.session_id.as_str()))
        // 收集函数私有映射。
        .collect::<HashMap<_, _>>();
    // 投影窗口集合。
    let windows = snapshot
        // 遍历窗口记录。
        .windows
        // 取得迭代器。
        .iter()
        // 构造公开对象。
        .map(|window| {
            // 只从同一快照按私有 PID 解析 opaque 进程关系。
            let process_session_id = process_session_by_native
                // 查找私有 PID。
                .get(&window.process_id)
                // 转成可序列化字符串。
                .copied();
            // 只公开 schema 允许的窗口事实。
            json!({
                // 从私有当前事实生成 canonical s2:w。
                "sessionId": opaque_window_session_id(window),
                // 输出稳定类别。
                "targetKind": "application-window",
                // 只输出进程文件名或非空哨兵。
                "applicationName": window.process_name.as_deref().unwrap_or("unavailable"),
                // 输出当前非空标题。
                "title": window.title,
                // 本集合固定为可见窗口。
                "visible": true,
                // 输出封闭可见性状态。
                "visibilityState": "visible",
                // 只读观察不要求前景。
                "foregroundRequiredForObservation": false,
                // 输出 canonical 进程关系或 null。
                "processSessionId": process_session_id,
            })
        })
        // 收集公开数组。
        .collect::<Vec<_>>();
    // 返回与 C++ 对照一致的版本化 envelope。
    Ok(json!({
        // 标记成功。
        "ok": true,
        // 固定语言中立协议版本。
        "contractVersion": "act/control/v1",
        // 声明当前主实现语言。
        "implementation": "rust",
        // 输出关系图数据。
        "data": {
            // 输出 canonical host 目标。
            "hostTargetId": snapshot.host_session_id,
            // 声明全流程只读。
            "readOnly": true,
            // 成功路径已通过前景门禁。
            "foregroundUnchanged": true,
            // 保持产品承诺常量。
            "productPromise": "broad-general-control-with-capability-degradation",
            // 输出逐来源覆盖事实。
            "coverage": {
                // ToolHelp 来源可用。
                "runningProcesses": "available",
                // EnumWindows 来源可用。
                "visibleTitledWindows": "available",
                // Registry 只覆盖传统卸载视图。
                "traditionalInstalledApplications": if snapshot.registry_source_available {
                    // 至少一个视图可读。
                    "partial-registry-uninstall"
                } else {
                    // 所有视图不可读。
                    "unavailable"
                },
                // Shell AppsFolder 可用性。
                "shellApplications": if snapshot.shell_source_available {
                    // 公开 AppsFolder 已枚举。
                    "available-shell-apps-folder"
                } else {
                    // Shell 来源不可用。
                    "unavailable"
                },
                // 固定 Start Menu Programs 快捷方式来源可用性。
                "startMenuApplications": if snapshot.start_menu_source_available {
                    // 两个固定 Known Folder 已完成认证枚举。
                    "available-shell-start-menu"
                } else {
                    // 固定 Start Menu 来源不可用。
                    "unavailable"
                },
                // Store/UWP 只由 AppsFolder 提供部分覆盖。
                "storeAndUwpPackages": if snapshot.shell_source_available {
                    // 声明部分覆盖。
                    "partial-shell-apps-folder"
                } else {
                    // Shell 来源不可用。
                    "unavailable"
                },
                // 进程清单保留无窗口进程。
                "noWindowProcessesIncluded": true,
                // 公开保守关系策略。
                "relationshipPolicy": "conservative-exact-normalized-display-or-icon-name",
            },
            // 输出逐集合完整性。
            "complete": {
                // 应用来源与边界完整性。
                "applications": snapshot.applications_complete,
                // ToolHelp 与边界完整性。
                "processes": snapshot.processes_complete,
                // EnumWindows 与边界完整性。
                "windows": snapshot.windows_complete,
            },
            // 输出安全聚合计数。
            "counts": {
                // 已安装应用数。
                "installedApplications": applications.len(),
                // 当前运行应用数。
                "runningInstalledApplications": running_application_count,
                // 当前运行进程数。
                "runningProcesses": processes.len(),
                // 当前无可见标题窗口进程数。
                "noWindowProcesses": no_window_process_count,
                // 无精确应用关系的进程数。
                "unassociatedProcesses": unassociated_process_count,
                // 最小元数据访问被拒绝的进程数。
                "permissionBlockedProcesses": permission_blocked_process_count,
                // 相对当前工具更高完整性的进程数。
                "higherIntegrityProcesses": higher_integrity_process_count,
                // 可见有标题窗口数。
                "visibleWindows": windows.len(),
            },
            // 输出应用集合。
            "applications": applications,
            // 输出进程集合。
            "processes": processes,
            // 输出窗口集合。
            "windows": windows,
        },
    }))
}

// 执行 Rust application.discover@1 完整只读纵切。
// 把当前快照中的命中数量转换为 fail-closed 解析结果。
fn require_unique_assessment_target(
    // 接收当前目标的命中数量。
    matches: usize,
    // 接收解析成功时的目标事实。
    target: AssessmentTarget,
) -> AppResult<AssessmentTarget> {
    // 按零、唯一和多命中分别返回稳定语义。
    match matches {
        // 唯一命中允许继续 assessment。
        1 => Ok(target),
        // 零命中表示目标已经过期或身份改变。
        0 => Err(ApplicationDiscoveryErrorCode::StaleSession.error(
            // 不公开任何用于重新发现的私有事实。
            "The opaque target no longer resolves in the current read-only inventory.",
        )),
        // 多命中表示公开指纹发生碰撞或内部清单重复。
        _ => Err(ApplicationDiscoveryErrorCode::AmbiguousTarget.error(
            // 仅说明 opaque 目标无法唯一解析。
            "The opaque target resolves to multiple current inventory records.",
        )),
    }
}

// 为 capability assessment 重新捕获完整关系图并解析精确目标。
pub(crate) fn resolve_assessment_target(session_id: &str) -> AppResult<AssessmentTarget> {
    // 先严格解析 canonical s2 外壳和目标类别。
    let parsed = OpaqueTargetId::parse(session_id).ok_or_else(|| {
        // 非 canonical 目标按使用时 stale 语义拒绝。
        ApplicationDiscoveryErrorCode::StaleSession.error(
            // 不接受旧版本或宽松别名。
            "The assessment target is not a current canonical opaque session.",
        )
    })?;
    // 捕获契约允许的完整有界只读清单。
    let snapshot = capture(
        // 使用每类公开硬上限，避免调用方截断造成伪 stale。
        MAXIMUM_DISCOVERY_ITEMS,
        // 使用完整进程硬上限。
        MAXIMUM_DISCOVERY_ITEMS,
        // 使用完整窗口硬上限。
        MAXIMUM_DISCOVERY_ITEMS,
    )?;
    // assessment 期间前景变化必须 fail closed。
    if !snapshot.foreground_unchanged {
        // 返回稳定宿主干扰错误。
        return Err(
            ApplicationDiscoveryErrorCode::HostInterferenceDetected.error(
                // 明确 assessment 没有继续执行。
                "The foreground target changed during capability assessment.",
            ),
        );
    }
    // 按 opaque 类别只在对应当前清单中解析。
    match parsed.kind() {
        // 主机目标每次重新计算当前 Windows 会话身份。
        OpaqueTargetKind::Host => require_unique_assessment_target(
            // 精确相等时唯一命中，否则零命中。
            usize::from(snapshot.host_session_id == session_id),
            // 主机只读来源可用时目标可继续评估。
            AssessmentTarget {
                // 标记主机类别。
                kind: OpaqueTargetKind::Host,
                // capture 成功即表示主机事实可用。
                availability: AssessmentAvailability::Available,
            },
        ),
        // 已安装应用只在当前应用清单中解析。
        OpaqueTargetKind::Application => require_unique_assessment_target(
            // 完整统计相同 canonical 应用身份。
            snapshot
                // 遍历已安装应用记录。
                .applications
                // 创建只读迭代器。
                .iter()
                // 只保留精确 session 命中。
                .filter(|application| application.session_id == session_id)
                // 统计命中以检测碰撞。
                .count(),
            // 应用清单命中后返回最小事实。
            AssessmentTarget {
                // 标记已安装应用类别。
                kind: OpaqueTargetKind::Application,
                // 枚举成功的应用目标可继续评估。
                availability: AssessmentAvailability::Available,
            },
        ),
        // 运行进程需要同时传播元数据权限分类。
        OpaqueTargetKind::Process => {
            // 收集全部精确命中，保留碰撞检测。
            let matches = snapshot
                // 遍历当前进程事实。
                .processes
                // 创建只读迭代器。
                .iter()
                // 只保留精确 canonical 身份。
                .filter(|process| process.session_id == session_id)
                // 收集引用以读取唯一命中的权限状态。
                .collect::<Vec<_>>();
            // 将唯一命中的元数据状态映射为公共分类。
            let availability = matches.first().map_or(
                // 零命中占位值不会越过数量门禁。
                AssessmentAvailability::Unavailable,
                // 唯一或多命中时读取第一项仅用于构造待门禁事实。
                |process| match process.metadata_access {
                    // 可读取元数据时继续 assessment。
                    ProcessMetadataAccess::Available => AssessmentAvailability::Available,
                    // Windows 拒绝读取时显式权限阻塞。
                    ProcessMetadataAccess::PermissionBlocked => {
                        // 返回不提权的权限分类。
                        AssessmentAvailability::PermissionBlocked
                    }
                    // 其他不可用状态保持 unavailable。
                    ProcessMetadataAccess::Unavailable => AssessmentAvailability::Unavailable,
                },
            );
            // 最终按命中数量执行 fail-closed 门禁。
            require_unique_assessment_target(
                // 传入完整命中数量。
                matches.len(),
                // 返回进程目标事实。
                AssessmentTarget {
                    // 标记运行进程类别。
                    kind: OpaqueTargetKind::Process,
                    // 传播元数据读取状态。
                    availability,
                },
            )
        }
        // 顶层窗口只在当前可见有标题清单中解析。
        OpaqueTargetKind::Window => require_unique_assessment_target(
            // 完整统计重新生成后相同的窗口身份。
            snapshot
                // 遍历当前窗口事实。
                .windows
                // 创建只读迭代器。
                .iter()
                // 从私有事实重新生成 opaque 身份。
                .filter(|window| opaque_window_session_id(window) == session_id)
                // 统计命中以检测碰撞。
                .count(),
            // 窗口枚举成功即返回可用事实。
            AssessmentTarget {
                // 标记应用窗口类别。
                kind: OpaqueTargetKind::Window,
                // Win32 只读枚举成功。
                availability: AssessmentAvailability::Available,
            },
        ),
        // 快照节点、控件、媒体与文档由各自 provider 重新解析。
        _ => Err(ApplicationDiscoveryErrorCode::StaleSession.error(
            // 指引上层尝试目标所属 provider，而不是猜测。
            "The target kind is not owned by the application discovery inventory.",
        )),
    }
}

pub(crate) fn discover(
    // 接收应用上限。
    maximum_applications: usize,
    // 接收进程上限。
    maximum_processes: usize,
    // 接收窗口上限。
    maximum_windows: usize,
) -> AppResult<Value> {
    // 在任何系统访问前验证应用边界。
    validate_limit(maximum_applications, "max-applications")?;
    // 在任何系统访问前验证进程边界。
    validate_limit(maximum_processes, "max-processes")?;
    // 在任何系统访问前验证窗口边界。
    validate_limit(maximum_windows, "max-windows")?;
    // 捕获一次组合快照。
    let snapshot = capture(maximum_applications, maximum_processes, maximum_windows)?;
    // 通过门禁后投影公开结果。
    render(snapshot)
}

// 声明无系统写操作的关系图 Module 测试。
#[cfg(test)]
#[path = "application_discovery_tests.rs"]
mod tests;
