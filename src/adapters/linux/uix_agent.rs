//! Linux UIX Agent v1 本机发现与认证 JSON Lines Adapter。

use std::{
    collections::BTreeSet,
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{Mode, OFlags},
    io::Errno,
    net::{
        AddressFamily, SocketAddrUnix, SocketFlags, SocketType, connect, socket_with,
        sockopt::{socket_error, socket_peercred},
    },
    process::geteuid,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::procfs,
    components::{
        opaque_id::{OpaqueTargetId, OpaqueTargetKind, OpaqueTargetMatch, match_opaque_target},
        uix_semantic_action_contract::UixSemanticAction,
        window_revision_wait_contract::WindowRevisionWaitCondition,
    },
};

const PROTOCOL_SCHEMA: &str = "uix.agent.v1";
const DISCOVERY_PREFIX: &str = "uix-agent";
const MAX_DESCRIPTOR_BYTES: u64 = 16 * 1024;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENDPOINTS: usize = 256;
const MAX_WINDOWS: usize = 4096;
const MAX_NODES: usize = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const INVENTORY_DEADLINE: Duration = Duration::from_secs(3);

#[path = "uix_agent_wait.rs"]
mod uix_agent_wait;

#[path = "uix_agent_semantic_action.rs"]
mod uix_agent_semantic_action;

#[path = "uix_agent_window_action.rs"]
mod uix_agent_window_action;

#[path = "uix_agent_window_close_transition.rs"]
mod uix_agent_window_close_transition;

#[path = "uix_agent_window_activation_transition.rs"]
mod uix_agent_window_activation_transition;

#[path = "uix_agent_window_lifecycle_sequence.rs"]
mod uix_agent_window_lifecycle_sequence;

#[path = "uix_agent_window_lifecycle_sequence_transition.rs"]
mod uix_agent_window_lifecycle_sequence_transition;

#[path = "uix_agent_pointer_drag.rs"]
mod uix_agent_pointer_drag;

#[path = "uix_agent_pointer_drag_transition.rs"]
mod uix_agent_pointer_drag_transition;

#[path = "uix_agent_pointer_click_sequence.rs"]
mod uix_agent_pointer_click_sequence;

#[path = "uix_agent_pointer_click_sequence_transition.rs"]
mod uix_agent_pointer_click_sequence_transition;

#[path = "uix_agent_pointer_click_transition.rs"]
mod uix_agent_pointer_click_transition;

#[path = "uix_agent_pointer_move_transition.rs"]
mod uix_agent_pointer_move_transition;

#[path = "uix_agent_pointer_sequence.rs"]
mod uix_agent_pointer_sequence;

#[path = "uix_agent_pointer_sequence_transition.rs"]
mod uix_agent_pointer_sequence_transition;

#[path = "uix_agent_pointer_move_sequence.rs"]
mod uix_agent_pointer_move_sequence;

#[path = "uix_agent_pointer_move_sequence_transition.rs"]
mod uix_agent_pointer_move_sequence_transition;

#[path = "uix_agent_window_state_wait.rs"]
mod uix_agent_window_state_wait;

#[path = "uix_agent_window_lifecycle_transition.rs"]
mod uix_agent_window_lifecycle_transition;

#[path = "uix_agent_element_wait.rs"]
mod uix_agent_element_wait;

#[path = "uix_agent_element_transition.rs"]
mod uix_agent_element_transition;

#[path = "uix_agent_key_sequence.rs"]
mod uix_agent_key_sequence;

#[path = "uix_agent_key_sequence_transition.rs"]
mod uix_agent_key_sequence_transition;

#[path = "uix_agent_key_transition.rs"]
mod uix_agent_key_transition;

#[path = "uix_agent_input_sequence.rs"]
mod uix_agent_input_sequence;

#[path = "uix_agent_input_sequence_transition.rs"]
mod uix_agent_input_sequence_transition;

#[path = "uix_agent_screenshot.rs"]
mod uix_agent_screenshot;

pub(crate) use self::uix_agent_element_transition::{
    ElementTransitionFailure, ElementTransitionFailureSource, ElementTransitionOutcome,
    perform_element_transition,
};
pub(crate) use self::uix_agent_element_wait::{
    ElementWaitFailure, ElementWaitOutcome, ambiguous_details, perform_element_wait,
};
pub(crate) use self::uix_agent_input_sequence::{
    InputSequenceFailure, InputSequenceOutcome, perform_input_sequence,
};
pub(crate) use self::uix_agent_input_sequence_transition::{
    InputSequenceTransitionFailure, InputSequenceTransitionFailureSource,
    InputSequenceTransitionOutcome, perform_input_sequence_transition,
};
pub(crate) use self::uix_agent_key_sequence::{
    KeySequenceFailure, KeySequenceOutcome, perform_key_sequence,
};
pub(crate) use self::uix_agent_key_sequence_transition::{
    KeySequenceTransitionFailure, KeySequenceTransitionFailureSource, KeySequenceTransitionOutcome,
    perform_key_sequence_transition,
};
pub(crate) use self::uix_agent_key_transition::{
    KeyTransitionFailure, KeyTransitionFailureSource, KeyTransitionOutcome, perform_key_transition,
};
pub(crate) use self::uix_agent_pointer_click_sequence::{
    PointerClickSequenceFailure, PointerClickSequenceOutcome, perform_pointer_click_sequence,
};
pub(crate) use self::uix_agent_pointer_click_sequence_transition::{
    PointerClickSequenceTransitionFailure, PointerClickSequenceTransitionFailureSource,
    PointerClickSequenceTransitionOutcome, perform_pointer_click_sequence_transition,
};
pub(crate) use self::uix_agent_pointer_click_transition::{
    PointerClickTransitionFailure, PointerClickTransitionFailureSource,
    PointerClickTransitionOutcome, perform_pointer_click_transition,
};
pub(crate) use self::uix_agent_pointer_drag::{
    PointerDragFailure, PointerDragOutcome, perform_pointer_drag,
};
pub(crate) use self::uix_agent_pointer_drag_transition::{
    PointerDragTransitionFailure, PointerDragTransitionFailureSource, PointerDragTransitionOutcome,
    perform_pointer_drag_transition,
};
pub(crate) use self::uix_agent_pointer_move_sequence::{
    PointerMoveSequenceFailure, PointerMoveSequenceOutcome, perform_pointer_move_sequence,
};
pub(crate) use self::uix_agent_pointer_move_sequence_transition::{
    PointerMoveSequenceTransitionFailure, PointerMoveSequenceTransitionFailureSource,
    PointerMoveSequenceTransitionOutcome, perform_pointer_move_sequence_transition,
};
pub(crate) use self::uix_agent_pointer_move_transition::{
    PointerMoveTransitionFailure, PointerMoveTransitionFailureSource, PointerMoveTransitionOutcome,
    perform_pointer_move_transition,
};
pub(crate) use self::uix_agent_pointer_sequence::{
    PointerSequenceFailure, PointerSequenceOutcome, perform_pointer_sequence,
};
pub(crate) use self::uix_agent_pointer_sequence_transition::{
    PointerSequenceTransitionFailure, PointerSequenceTransitionFailureSource,
    PointerSequenceTransitionOutcome, perform_pointer_sequence_transition,
};
pub(crate) use self::uix_agent_screenshot::{ScreenshotRecord, capture as capture_screenshot};
#[cfg(test)]
use self::uix_agent_semantic_action::action_request_failure;
pub(crate) use self::uix_agent_wait::wait_closed;
#[cfg(test)]
use self::uix_agent_wait::wait_outcome;
pub(crate) use self::uix_agent_window_action::{
    WindowActionFailure, perform_activation, perform_close, perform_key, perform_pointer,
    perform_window,
};
#[cfg(test)]
pub(crate) use self::uix_agent_window_activation_transition::WindowFocusObservation;
pub(crate) use self::uix_agent_window_activation_transition::{
    WindowActivationTransitionFailure, WindowActivationTransitionOutcome,
    perform_window_activation_transition,
};
pub(crate) use self::uix_agent_window_close_transition::{
    WindowCloseTransitionFailure, WindowCloseTransitionFailureSource, WindowCloseTransitionOutcome,
    perform_window_close_transition,
};
pub(crate) use self::uix_agent_window_lifecycle_sequence::{
    WindowLifecycleSequenceFailure, WindowLifecycleSequenceOutcome,
    perform_window_lifecycle_sequence,
};
pub(crate) use self::uix_agent_window_lifecycle_sequence_transition::{
    WindowLifecycleSequenceTransitionFailure, WindowLifecycleSequenceTransitionFailureSource,
    WindowLifecycleSequenceTransitionOutcome, perform_window_lifecycle_sequence_transition,
};
pub(crate) use self::uix_agent_window_lifecycle_transition::{
    WindowLifecycleTransitionFailure, WindowLifecycleTransitionOutcome,
    perform_window_lifecycle_transition,
};
pub(crate) use self::uix_agent_window_state_wait::{
    WindowStateWaitObservation, perform_window_state_wait,
};

/// Adapter 内部稳定失败分类；公共错误由 Module 统一投影。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    Unavailable,
    PermissionDenied,
    Timeout,
    Protocol,
    Stale,
    Ambiguous,
}

/// 已越过通用 transport 后的语义动作封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionFailure {
    Transport(Failure),
    StaleRevision,
    ElementNotFound,
    AmbiguousTarget,
    UnsupportedAction,
    Forbidden,
    ConfirmationRejected,
    ConfirmationNotFound,
    InvalidValue,
    NotInteractable,
    Blocked,
    DidNotSettle,
    NotPresentable,
    OutcomeUnknown,
}

/// UIX 已明确完成动作后允许跨 Adapter 的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) application_confirmation_performed: bool,
}

/// 激活请求成功后的可信 Agent 完成事实与尽力焦点观察。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowActivationOutcome {
    pub(crate) focus_observed_after_dispatch: Option<bool>,
    pub(crate) target_generation_current_after_dispatch: Option<bool>,
}

/// UIX 等待完成后允许跨 Adapter 的封闭结果类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RevisionWaitOutcomeKind {
    Changed,
    Presented,
    Closed,
}

/// 不泄露窗口原生身份的修订等待结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RevisionWaitOutcome {
    pub(crate) kind: RevisionWaitOutcomeKind,
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) closed: bool,
}

/// 不泄露端点、token、PID 或 UIX 原生 ID 的中立窗口记录。
#[derive(Clone, Debug)]
pub(crate) struct WindowRecord {
    pub(crate) session_id: String,
    /// 只保存重新解析后的中立进程代际，不把 UIX descriptor PID 泄露给 Module。
    pub(crate) owner_process_session_id: Option<String>,
    pub(crate) title: String,
    pub(crate) visible: bool,
    pub(crate) presentable: bool,
    pub(crate) focused: Option<bool>,
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) state: Option<WindowStateRecord>,
    pub(crate) screenshot_supported: bool,
    pub(crate) activation_supported: bool,
    pub(crate) pointer_drag_supported: bool,
    endpoint: EndpointDescriptor,
    window_id: u64,
    generation: u64,
}

/// UIX Agent 协商后公开的跨平台窗口当前状态；不代表 compositor 已确认终态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowStateRecord {
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) fullscreen: bool,
}

/// 一次协作式 UIX 窗口清单。
#[derive(Debug)]
pub(crate) struct Inventory {
    pub(crate) windows: Vec<WindowRecord>,
    pub(crate) total: Option<usize>,
    pub(crate) complete: bool,
    pub(crate) warnings: Vec<&'static str>,
}

/// UIX 语义快照的最小中立节点；只允许应用客户区 logical 几何跨 Adapter，敏感值、宿主坐标和选择内容不跨越该边界。
#[derive(Clone, Debug)]
pub(crate) struct NodeRecord {
    pub(crate) native_id: String,
    pub(crate) parent_native_id: Option<String>,
    pub(crate) automation_id: Option<String>,
    pub(crate) focused: bool,
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) actions: Vec<String>,
    pub(crate) frame: RectRecord,
    pub(crate) visible_bounds: Option<RectRecord>,
}

/// UIX 应用客户区中的中立 logical 矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RectRecord {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// 已通过端点认证和窗口代际校验的语义快照。
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) session_id: String,
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) nodes: Vec<NodeRecord>,
}

#[derive(Clone, Debug, Deserialize)]
struct EndpointDescriptor {
    schema: String,
    process_id: u32,
    endpoint: String,
    token: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct WireWindow {
    window_id: u64,
    generation: u64,
    title: String,
    visible: bool,
    presentable: bool,
    logical_width: Option<i32>,
    logical_height: Option<i32>,
    maximized: Option<bool>,
    minimized: Option<bool>,
    fullscreen: Option<bool>,
    focused: Option<bool>,
    revision: u64,
    presented_revision: u64,
    closed: bool,
}

#[derive(Debug, Deserialize)]
struct WireSnapshot {
    window_id: u64,
    generation: u64,
    revision: u64,
    presented_revision: u64,
    closed: bool,
    nodes: Vec<WireNode>,
}

#[derive(Debug, Deserialize)]
struct WireNode {
    node_id: String,
    automation_id: Option<String>,
    parent: Option<String>,
    focused: bool,
    role: String,
    name: Option<String>,
    frame: WireRect,
    visible_bounds: Option<WireRect>,
    state: WireState,
    actions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct WireRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Deserialize)]
struct WireState {
    disabled: bool,
}

#[derive(Debug, Deserialize)]
struct WirePerformed {
    window_id: u64,
    generation: u64,
    revision: u64,
    presented_revision: u64,
    settled: bool,
}

#[derive(Debug)]
enum RequestFailure {
    Transport(Failure),
    Remote {
        code: String,
        confirm_id: Option<u64>,
    },
}

impl From<Failure> for RequestFailure {
    fn from(value: Failure) -> Self {
        Self::Transport(value)
    }
}

/// 枚举当前用户显式发布的 UIX Agent 窗口，并保持部分失败可见。
pub(crate) fn discover(maximum_items: usize) -> Result<Inventory, Failure> {
    discover_from(&discovery_directory()?, maximum_items)
}

/// 使用当前重新发现清单唯一解析精确 UIX 窗口。
pub(crate) fn resolve(target: &str) -> Result<WindowRecord, Failure> {
    resolve_until(target, Instant::now() + INVENTORY_DEADLINE)
}

/// 在调用方总 deadline 内重新发现并唯一解析精确 UIX 窗口。
fn resolve_until(target: &str, deadline: Instant) -> Result<WindowRecord, Failure> {
    let inventory = discover_from_until(&discovery_directory()?, MAX_WINDOWS, deadline)?;
    if Instant::now() >= deadline {
        return Err(Failure::Timeout);
    }
    match match_opaque_target(target, &inventory.windows, |window| {
        Some(window.session_id.clone())
    }) {
        OpaqueTargetMatch::Unique(window) if inventory.complete => Ok(window.clone()),
        OpaqueTargetMatch::Unique(_) => Err(Failure::Unavailable),
        OpaqueTargetMatch::Ambiguous => Err(Failure::Ambiguous),
        OpaqueTargetMatch::Missing if inventory.complete => Err(Failure::Stale),
        OpaqueTargetMatch::Missing => Err(Failure::Unavailable),
    }
}

/// 重新解析窗口后读取一次有界 UIX 语义快照。
pub(crate) fn snapshot(target: &str) -> Result<Snapshot, Failure> {
    let window = resolve(target)?;
    let mut client = AgentClient::connect(&window.endpoint)?;
    let wire = client.snapshot(window.window_id, window.generation)?;
    if wire.window_id != window.window_id || wire.generation != window.generation || wire.closed {
        return Err(Failure::Stale);
    }
    if wire.revision < window.revision
        || wire.presented_revision < window.presented_revision
        || wire.presented_revision > wire.revision
    {
        return Err(Failure::Protocol);
    }
    if wire.nodes.len() > MAX_NODES {
        return Err(Failure::Protocol);
    }
    let nodes = wire
        .nodes
        .into_iter()
        .map(|node| {
            if node.node_id.is_empty()
                || node.node_id.len() > 128
                || node.parent.as_ref().is_some_and(|value| value.len() > 128)
                || node
                    .automation_id
                    .as_ref()
                    .is_some_and(|value| value.len() > 512)
                || node.role.is_empty()
                || node.role.len() > 64
                || node.actions.len() > 32
                || node
                    .actions
                    .iter()
                    .any(|action| action.is_empty() || action.len() > 64)
                || node.actions.iter().collect::<BTreeSet<_>>().len() != node.actions.len()
            {
                return Err(Failure::Protocol);
            }
            Ok(NodeRecord {
                native_id: node.node_id,
                parent_native_id: node.parent,
                automation_id: node.automation_id,
                focused: node.focused,
                role: node.role,
                name: truncate_utf8(node.name.as_deref().unwrap_or_default(), 256),
                enabled: !node.state.disabled,
                actions: node.actions,
                frame: rect_record(node.frame)?,
                visible_bounds: node.visible_bounds.map(rect_record).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Snapshot {
        session_id: window.session_id,
        revision: wire.revision,
        presented_revision: wire.presented_revision,
        nodes,
    })
}

fn rect_record(rect: WireRect) -> Result<RectRecord, Failure> {
    const MAXIMUM_LOGICAL_COORDINATE: f64 = 1_000_000_000.0;
    if ![rect.x, rect.y, rect.w, rect.h]
        .into_iter()
        .all(f64::is_finite)
        || rect.x.abs() > MAXIMUM_LOGICAL_COORDINATE
        || rect.y.abs() > MAXIMUM_LOGICAL_COORDINATE
        || !(0.0..=MAXIMUM_LOGICAL_COORDINATE).contains(&rect.w)
        || !(0.0..=MAXIMUM_LOGICAL_COORDINATE).contains(&rect.h)
    {
        return Err(Failure::Protocol);
    }
    Ok(RectRecord {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
    })
}

/// 重新解析窗口后，以当前 UIX 修订号执行一次精确节点动作。
pub(crate) fn perform(
    target: &str,
    native_node_id: &str,
    expected_revision: u64,
    action: &UixSemanticAction,
    timeout_ms: u32,
) -> Result<ActionOutcome, ActionFailure> {
    let window = resolve(target).map_err(ActionFailure::Transport)?;
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    let mut client =
        AgentClient::connect_until(&window.endpoint, deadline).map_err(ActionFailure::Transport)?;
    client.perform(
        window.window_id,
        window.generation,
        expected_revision,
        native_node_id,
        action,
    )
}

/// 在总 deadline 内重新解析窗口并等待精确代际的修订条件。
pub(crate) fn wait_revision(
    target: &str,
    condition: WindowRevisionWaitCondition,
    timeout_ms: u32,
) -> Result<RevisionWaitOutcome, Failure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    let window = resolve_until(target, deadline)?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)?;
    client.wait_revision(
        window.window_id,
        window.generation,
        window.revision,
        window.presented_revision,
        condition,
    )
}

fn discovery_directory() -> Result<PathBuf, Failure> {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    if !base.is_absolute() {
        return Err(Failure::PermissionDenied);
    }
    Ok(base.join(format!("{DISCOVERY_PREFIX}-{}", geteuid().as_raw())))
}

fn discover_from(directory: &Path, maximum_items: usize) -> Result<Inventory, Failure> {
    discover_from_until(
        directory,
        maximum_items,
        Instant::now() + INVENTORY_DEADLINE,
    )
}

fn discover_from_until(
    directory: &Path,
    maximum_items: usize,
    deadline: Instant,
) -> Result<Inventory, Failure> {
    remaining(deadline)?;
    let maximum_items = maximum_items.clamp(1, MAX_WINDOWS);
    let effective_uid = geteuid().as_raw();
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Inventory {
                windows: Vec::new(),
                total: Some(0),
                complete: true,
                warnings: Vec::new(),
            });
        }
        Err(_) => return Err(Failure::Unavailable),
    };
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(Failure::PermissionDenied);
    }

    let mut paths = Vec::new();
    let mut warnings = BTreeSet::new();
    let mut complete = true;
    for entry in fs::read_dir(directory).map_err(|_| Failure::Unavailable)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                warnings.insert("unsafe-or-invalid-entry");
                complete = false;
                continue;
            }
        };
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("uix-") && name.ends_with(".json"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.len() > MAX_ENDPOINTS {
        paths.truncate(MAX_ENDPOINTS);
        warnings.insert("endpoint-limit");
        complete = false;
    }

    let mut windows = Vec::new();
    let descriptor_count = paths.len();
    for (index, path) in paths.into_iter().enumerate() {
        if Instant::now() >= deadline {
            warnings.insert("deadline");
            complete = false;
            break;
        }
        let Some(descriptor) = read_descriptor(directory, &path, effective_uid) else {
            warnings.insert("unsafe-or-invalid-entry");
            complete = false;
            continue;
        };
        match list_endpoint(&descriptor, deadline) {
            Ok(mut endpoint_windows) => {
                if windows.len().saturating_add(endpoint_windows.len()) > MAX_WINDOWS {
                    let remaining = MAX_WINDOWS.saturating_sub(windows.len());
                    endpoint_windows.truncate(remaining);
                    warnings.insert("provider-item-limit");
                    complete = false;
                }
                windows.extend(endpoint_windows);
                if windows.len() == MAX_WINDOWS {
                    if index + 1 < descriptor_count {
                        warnings.insert("provider-item-limit");
                        complete = false;
                    }
                    break;
                }
            }
            Err(Failure::PermissionDenied) => {
                warnings.insert("authentication-rejected");
                complete = false;
            }
            Err(Failure::Timeout) => {
                warnings.insert("endpoint-timeout");
                complete = false;
            }
            Err(_) => {
                warnings.insert("unreachable-or-invalid-endpoint");
                complete = false;
            }
        }
    }
    windows.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let total = complete.then_some(windows.len());
    if windows.len() > maximum_items {
        windows.truncate(maximum_items);
        warnings.insert("item-limit");
    }
    Ok(Inventory {
        windows,
        total,
        complete,
        warnings: warnings.into_iter().collect(),
    })
}

fn read_descriptor(
    directory: &Path,
    path: &Path,
    effective_uid: u32,
) -> Option<EndpointDescriptor> {
    let file_name = path.file_name()?.to_str()?;
    let process_text = file_name.strip_prefix("uix-")?.strip_suffix(".json")?;
    if process_text.is_empty() || !process_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let file_process_id = process_text.parse::<u32>().ok()?;
    let descriptor_fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .ok()?;
    let mut file = File::from(descriptor_fd);
    let before = file.metadata().ok()?;
    if !before.file_type().is_file()
        || before.uid() != effective_uid
        || before.mode() & 0o077 != 0
        || before.len() == 0
        || before.len() > MAX_DESCRIPTOR_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_DESCRIPTOR_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DESCRIPTOR_BYTES {
        return None;
    }
    let after = file.metadata().ok()?;
    if !same_file_generation(&before, &after) {
        return None;
    }
    let descriptor = serde_json::from_slice::<EndpointDescriptor>(&bytes).ok()?;
    if descriptor.schema != PROTOCOL_SCHEMA
        || descriptor.state != "ready"
        || descriptor.process_id != file_process_id
        || descriptor.process_id == 0
        || descriptor.process_id > i32::MAX as u32
        || descriptor.token.len() != 64
        || !descriptor
            .token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let endpoint = Path::new(&descriptor.endpoint);
    let expected_name = format!(
        "uix-{}-{}.sock",
        descriptor.process_id,
        &descriptor.token[..24]
    );
    if !endpoint.is_absolute()
        || endpoint.parent() != Some(directory)
        || endpoint.file_name()?.to_str()? != expected_name
    {
        return None;
    }
    let socket = fs::symlink_metadata(endpoint).ok()?;
    if !socket.file_type().is_socket()
        || socket.file_type().is_symlink()
        || socket.uid() != effective_uid
        || socket.mode() & 0o077 != 0
    {
        return None;
    }
    let process = fs::metadata(format!("/proc/{}", descriptor.process_id)).ok()?;
    (process.uid() == effective_uid).then_some(descriptor)
}

fn same_file_generation(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.mode() == right.mode()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

fn list_endpoint(
    descriptor: &EndpointDescriptor,
    deadline: Instant,
) -> Result<Vec<WindowRecord>, Failure> {
    let mut client = AgentClient::connect_until(descriptor, deadline)?;
    let windows = client.list_windows()?;
    let window_state_supported = client.supports_window_state();
    let window_focus_supported = client.window_state_fields.contains("focused");
    let screenshot_supported = client.supports_screenshot();
    let activation_supported = client.supports_activation();
    let pointer_drag_supported = client.supports_pointer_drag();
    if windows.len() > MAX_WINDOWS {
        return Err(Failure::Protocol);
    }
    windows
        .into_iter()
        .filter(|window| !window.closed)
        .map(|window| {
            if window.title.len() > 64 * 1024 || window.presented_revision > window.revision {
                return Err(Failure::Protocol);
            }
            let state = window_state_record(&window, window_state_supported)?;
            let focused = if window_focus_supported {
                window.focused
            } else {
                None
            };
            let session_id = window_session_id(descriptor, &window);
            Ok(WindowRecord {
                session_id,
                owner_process_session_id: procfs::session_id_for_pid(descriptor.process_id),
                title: truncate_utf8(&window.title, 256),
                visible: window.visible,
                presentable: window.presentable,
                focused,
                revision: window.revision,
                presented_revision: window.presented_revision,
                state,
                screenshot_supported,
                activation_supported,
                pointer_drag_supported,
                endpoint: descriptor.clone(),
                window_id: window.window_id,
                generation: window.generation,
            })
        })
        .collect()
}

fn window_state_record(
    window: &WireWindow,
    negotiated: bool,
) -> Result<Option<WindowStateRecord>, Failure> {
    if !negotiated {
        return Ok(None);
    }
    let (
        Some(logical_width),
        Some(logical_height),
        Some(maximized),
        Some(minimized),
        Some(fullscreen),
    ) = (
        window.logical_width,
        window.logical_height,
        window.maximized,
        window.minimized,
        window.fullscreen,
    )
    else {
        return Err(Failure::Protocol);
    };
    if logical_width < 0 || logical_height < 0 {
        return Err(Failure::Protocol);
    }
    Ok(Some(WindowStateRecord {
        logical_width: logical_width as u32,
        logical_height: logical_height as u32,
        maximized,
        minimized,
        fullscreen,
    }))
}

fn window_session_id(descriptor: &EndpointDescriptor, window: &WireWindow) -> String {
    OpaqueTargetId::new(
        OpaqueTargetKind::Window,
        &format!(
            "uix-agent-v1-window\0{}\0{}\0{}\0{}",
            descriptor.process_id, descriptor.token, window.window_id, window.generation
        ),
    )
    .to_string()
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

// 将有状态协议客户端拆到独立 Adapter 文件，保持主文件规模受控。
include!("uix_agent_client.rs");
fn bounded_capability_names(values: &[Value]) -> Result<BTreeSet<String>, Failure> {
    let mut names = BTreeSet::new();
    for value in values {
        let name = value.as_str().ok_or(Failure::Protocol)?;
        if name.is_empty() || name.len() > 64 || !names.insert(name.to_owned()) {
            return Err(Failure::Protocol);
        }
    }
    Ok(names)
}

fn remaining(deadline: Instant) -> Result<Duration, Failure> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(Failure::Timeout)
    } else {
        Ok(remaining)
    }
}

fn read_bounded_line_with_limit(
    reader: &mut impl BufRead,
    maximum_bytes: usize,
) -> Result<Vec<u8>, Failure> {
    let mut output = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(io_failure)?;
        if available.is_empty() {
            return Err(Failure::Protocol);
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if output.len().saturating_add(take) > maximum_bytes {
            return Err(Failure::Protocol);
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if output.last() == Some(&b'\n') {
            output.pop();
            return Ok(output);
        }
    }
}

fn io_failure(error: std::io::Error) -> Failure {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        Failure::Timeout
    } else {
        Failure::Unavailable
    }
}

fn wire_error(object: &Map<String, Value>) -> RequestFailure {
    let Some(error) = object.get("error").and_then(Value::as_object) else {
        return RequestFailure::Transport(Failure::Protocol);
    };
    let Some(code) = error
        .get("code")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 64)
    else {
        return RequestFailure::Transport(Failure::Protocol);
    };
    RequestFailure::Remote {
        code: code.to_owned(),
        confirm_id: error.get("confirm_id").and_then(Value::as_u64),
    }
}

fn read_request_failure(error: RequestFailure) -> Failure {
    match error {
        RequestFailure::Transport(failure) => failure,
        RequestFailure::Remote { code, .. } => match code.as_str() {
            "unauthorized" | "forbidden" => Failure::PermissionDenied,
            "timeout" => Failure::Timeout,
            "window_not_found" | "stale_window" | "app_closed" => Failure::Stale,
            _ => Failure::Protocol,
        },
    }
}

#[cfg(test)]
#[path = "uix_agent_tests.rs"]
mod tests;
