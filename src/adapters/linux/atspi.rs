//! 私有 AT-SPI D-Bus Adapter；所有 zbus 类型都在本文件边界内终止。

use std::{
    collections::BTreeSet,
    future::Future,
    time::{Duration, Instant},
};

use futures_lite::future;
use zbus::{
    Connection, Proxy,
    connection::Builder,
    proxy::{Builder as ProxyBuilder, CacheProperties},
    zvariant::OwnedObjectPath,
};

use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

const SESSION_BUS_NAME: &str = "org.a11y.Bus";
const SESSION_BUS_PATH: &str = "/org/a11y/bus";
const SESSION_BUS_INTERFACE: &str = "org.a11y.Bus";
const REGISTRY_NAME: &str = "org.a11y.atspi.Registry";
const REGISTRY_ROOT: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE_INTERFACE: &str = "org.a11y.atspi.Accessible";
const MAXIMUM_NAME_BYTES: usize = 256;

/// 只由候选 worker 私有协议注入的两条 bus 地址与总 deadline。
#[derive(Clone, Debug)]
pub(crate) struct ConnectionConfig {
    pub(crate) session_bus_address: String,
    pub(crate) expected_accessibility_bus_address: String,
    pub(crate) timeout_ms: u32,
}

/// Adapter 对上层唯一暴露的封闭错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    AccessibilityUnavailable,
    PermissionDenied,
    Timeout,
    Protocol,
    Stale,
    Ambiguous,
}

/// 不含 bus 名称或 object path 的截断原因。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TruncationReason {
    DepthLimit,
    ItemLimit,
    CycleDetected,
    StaleDescendant,
    PropertyError,
    NameLimit,
}

impl TruncationReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::DepthLimit => "depth-limit",
            Self::ItemLimit => "item-limit",
            Self::CycleDetected => "cycle-detected",
            Self::StaleDescendant => "stale-descendant",
            Self::PropertyError => "property-error",
            Self::NameLimit => "name-limit",
        }
    }
}

/// 私有 owner/path 身份；只能在 Adapter 内参与重新解析。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct NativeReference {
    destination: String,
    owner: String,
    path: String,
    bus_generation: String,
}

/// 对 Module 暴露的中立窗口记录，不含任何 D-Bus 原生事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowRecord {
    pub(crate) session_id: String,
    pub(crate) role: &'static str,
    pub(crate) title: String,
    pub(crate) name_truncated: bool,
    native: NativeReference,
}

/// 一次 partial exporter inventory 的中立结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WindowInventory {
    pub(crate) windows: Vec<WindowRecord>,
    pub(crate) reasons: BTreeSet<TruncationReason>,
}

/// 中立 BFS 节点；不包含 toolkit 或原生接口字段。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeNode {
    pub(crate) snapshot_id: String,
    pub(crate) node_id: String,
    pub(crate) parent_node_id: Option<String>,
    pub(crate) depth: u8,
    pub(crate) role: &'static str,
    pub(crate) name: String,
    pub(crate) enabled: Option<bool>,
    pub(crate) child_count: u32,
    pub(crate) property_read_complete: bool,
}

/// 中立 BFS 结果。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeSnapshot {
    pub(crate) snapshot_id: String,
    pub(crate) nodes: Vec<TreeNode>,
    pub(crate) reasons: BTreeSet<TruncationReason>,
    pub(crate) audit: CallAudit,
}

/// Adapter 自有 API 计数；禁用接口没有可调用入口，计数恒为零。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CallAudit {
    pub(crate) child_count: u64,
    pub(crate) child_at_index: u64,
    pub(crate) name: u64,
    pub(crate) role: u64,
    pub(crate) state: u64,
    pub(crate) get_all: u64,
    pub(crate) get_children: u64,
    pub(crate) cache_get_items: u64,
    pub(crate) action: u64,
    pub(crate) editable_text: u64,
    pub(crate) selection: u64,
    pub(crate) value: u64,
    pub(crate) component_grab_focus: u64,
    pub(crate) get_application_bus_address: u64,
}

#[derive(Clone, Debug)]
struct AccessibleRecord {
    reference: NativeReference,
    role: u32,
    name: String,
    enabled: Option<bool>,
    visible: bool,
    showing: bool,
    child_count: u32,
    property_read_complete: bool,
    name_truncated: bool,
}

struct Deadline {
    expires: Instant,
}

impl Deadline {
    fn new(timeout_ms: u32) -> Result<Self, Failure> {
        if !(1..=30_000).contains(&timeout_ms) {
            return Err(Failure::Protocol);
        }
        Ok(Self {
            expires: Instant::now() + Duration::from_millis(u64::from(timeout_ms)),
        })
    }

    fn remaining(&self) -> Result<Duration, Failure> {
        self.expires
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or(Failure::Timeout)
    }
}

struct ConnectedBuses {
    accessibility: Connection,
    bus_generation: String,
}

/// 只接受显式地址的 connector；绝不读取环境或真实 org.a11y.Bus。
struct AtspiBusConnector;

impl AtspiBusConnector {
    async fn connect(
        config: &ConnectionConfig,
        deadline: &Deadline,
    ) -> Result<ConnectedBuses, Failure> {
        if config.session_bus_address.is_empty()
            || config.expected_accessibility_bus_address.is_empty()
        {
            return Err(Failure::Protocol);
        }
        let session_builder = Builder::address(config.session_bus_address.as_str())
            .map_err(map_error)?
            .method_timeout(deadline.remaining()?);
        let session = within(deadline, session_builder.build()).await?;
        let proxy = build_proxy(
            &session,
            SESSION_BUS_NAME,
            SESSION_BUS_PATH,
            SESSION_BUS_INTERFACE,
            deadline,
        )
        .await?;
        let returned: String = within(deadline, proxy.call("GetAddress", &())).await?;
        if returned != config.expected_accessibility_bus_address {
            return Err(Failure::Protocol);
        }
        let accessibility_builder =
            Builder::address(config.expected_accessibility_bus_address.as_str())
                .map_err(map_error)?
                .method_timeout(deadline.remaining()?);
        let accessibility = within(deadline, accessibility_builder.build()).await?;
        let dbus = build_proxy(
            &accessibility,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            deadline,
        )
        .await?;
        let bus_generation: String = within(deadline, dbus.call("GetId", &())).await?;
        if bus_generation.is_empty() {
            return Err(Failure::Protocol);
        }
        Ok(ConnectedBuses {
            accessibility,
            bus_generation,
        })
    }
}

/// 仅提供允许 API 的 accessible reader。
struct AccessibleReader<'a> {
    connection: &'a Connection,
    deadline: &'a Deadline,
    bus_generation: &'a str,
    audit: CallAudit,
}

impl<'a> AccessibleReader<'a> {
    fn new(connection: &'a Connection, deadline: &'a Deadline, bus_generation: &'a str) -> Self {
        Self {
            connection,
            deadline,
            bus_generation,
            audit: CallAudit::default(),
        }
    }

    async fn owner(&self, destination: &str) -> Result<String, Failure> {
        let dbus = build_proxy(
            self.connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            self.deadline,
        )
        .await?;
        within(self.deadline, dbus.call("GetNameOwner", &(destination,))).await
    }

    async fn reference(
        &self,
        destination: String,
        path: String,
    ) -> Result<NativeReference, Failure> {
        let owner = self.owner(&destination).await?;
        Ok(NativeReference {
            destination,
            owner,
            path,
            bus_generation: self.bus_generation.to_owned(),
        })
    }

    async fn child_count(&mut self, reference: &NativeReference) -> Result<u32, Failure> {
        self.audit.child_count += 1;
        let proxy = self.proxy(reference).await?;
        within(self.deadline, proxy.get_property("ChildCount")).await
    }

    async fn child_at(
        &mut self,
        reference: &NativeReference,
        index: u32,
    ) -> Result<NativeReference, Failure> {
        self.audit.child_at_index += 1;
        let proxy = self.proxy(reference).await?;
        let (destination, path): (String, OwnedObjectPath) =
            within(self.deadline, proxy.call("GetChildAtIndex", &(index,))).await?;
        self.reference(destination, path.to_string()).await
    }

    async fn read(
        &mut self,
        reference: &NativeReference,
        strict: bool,
    ) -> Result<AccessibleRecord, Failure> {
        let proxy = self.proxy(reference).await?;
        self.audit.child_count += 1;
        let child_count = within(self.deadline, proxy.get_property::<u32>("ChildCount")).await?;
        self.audit.name += 1;
        let name = within(self.deadline, proxy.get_property::<String>("Name")).await;
        self.audit.role += 1;
        let role = within(self.deadline, proxy.call::<_, _, u32>("GetRole", &())).await;
        self.audit.state += 1;
        let state = within(self.deadline, proxy.call::<_, _, Vec<u32>>("GetState", &())).await;
        if strict && (name.is_err() || role.is_err() || state.is_err()) {
            return Err(Failure::Stale);
        }
        let mut property_read_complete = true;
        let (name, name_truncated) = match name {
            Ok(value) => truncate_name(&value),
            Err(_) => {
                property_read_complete = false;
                (String::new(), false)
            }
        };
        let role = match role {
            Ok(value) => value,
            Err(_) => {
                property_read_complete = false;
                0
            }
        };
        let states = match state {
            Ok(value) => value,
            Err(_) => {
                property_read_complete = false;
                Vec::new()
            }
        };
        Ok(AccessibleRecord {
            reference: reference.clone(),
            role,
            name,
            enabled: property_read_complete.then(|| state_contains(&states, 8)),
            visible: property_read_complete && state_contains(&states, 30),
            showing: property_read_complete && state_contains(&states, 25),
            child_count,
            property_read_complete,
            name_truncated,
        })
    }

    async fn proxy(&self, reference: &NativeReference) -> Result<Proxy<'static>, Failure> {
        build_proxy(
            self.connection,
            reference.destination.as_str(),
            reference.path.as_str(),
            ACCESSIBLE_INTERFACE,
            self.deadline,
        )
        .await
    }
}

/// 发现主动导出的顶层 accessibility window；不声称全局窗口目录。
pub(crate) async fn discover(
    config: &ConnectionConfig,
    maximum_items: usize,
) -> Result<WindowInventory, Failure> {
    if !(1..=4096).contains(&maximum_items) {
        return Err(Failure::Protocol);
    }
    let deadline = Deadline::new(config.timeout_ms)?;
    let buses = AtspiBusConnector::connect(config, &deadline).await?;
    let mut reader = AccessibleReader::new(&buses.accessibility, &deadline, &buses.bus_generation);
    let root = reader
        .reference(REGISTRY_NAME.to_owned(), REGISTRY_ROOT.to_owned())
        .await?;
    let child_count = reader.child_count(&root).await?;
    let mut windows = Vec::new();
    let mut reasons = BTreeSet::new();
    let allowed = usize::try_from(child_count)
        .unwrap_or(usize::MAX)
        .min(maximum_items);
    if usize::try_from(child_count).unwrap_or(usize::MAX) > maximum_items {
        reasons.insert(TruncationReason::ItemLimit);
    }
    for index in 0..allowed {
        let child = match reader.child_at(&root, index as u32).await {
            Ok(value) => value,
            Err(Failure::Timeout) => return Err(Failure::Timeout),
            Err(_) => {
                reasons.insert(TruncationReason::StaleDescendant);
                continue;
            }
        };
        let record = match reader.read(&child, false).await {
            Ok(value) => value,
            Err(Failure::Timeout) => return Err(Failure::Timeout),
            Err(_) => {
                reasons.insert(TruncationReason::StaleDescendant);
                continue;
            }
        };
        let Some(role) = window_role(record.role) else {
            continue;
        };
        if !record.visible || !record.showing || record.name.is_empty() {
            continue;
        }
        if !record.property_read_complete {
            reasons.insert(TruncationReason::PropertyError);
            continue;
        }
        if record.name_truncated {
            reasons.insert(TruncationReason::NameLimit);
        }
        windows.push(WindowRecord {
            session_id: window_session_id(&record.reference),
            role,
            title: record.name,
            name_truncated: record.name_truncated,
            native: record.reference,
        });
    }
    Ok(WindowInventory { windows, reasons })
}

/// 使用当前 inventory 唯一解析窗口，owner/bus 变化会自然 stale。
pub(crate) async fn resolve(
    config: &ConnectionConfig,
    target: &str,
    maximum_items: usize,
) -> Result<WindowRecord, Failure> {
    let inventory = discover(config, maximum_items).await?;
    let mut matches = inventory
        .windows
        .into_iter()
        .filter(|window| window.session_id == target);
    let Some(first) = matches.next() else {
        return Err(Failure::Stale);
    };
    if matches.next().is_some() {
        return Err(Failure::Ambiguous);
    }
    Ok(first)
}

/// 从精确窗口执行有界 BFS；根失败时不返回 partial。
pub(crate) async fn read_tree(
    config: &ConnectionConfig,
    target: &str,
    maximum_depth: u8,
    maximum_items: usize,
) -> Result<TreeSnapshot, Failure> {
    if maximum_depth > 20 || !(1..=4096).contains(&maximum_items) {
        return Err(Failure::Protocol);
    }
    let deadline = Deadline::new(config.timeout_ms)?;
    let buses = AtspiBusConnector::connect(config, &deadline).await?;
    let mut reader = AccessibleReader::new(&buses.accessibility, &deadline, &buses.bus_generation);
    let inventory = inventory_with_reader(&mut reader, maximum_items).await?;
    let mut matches = inventory
        .into_iter()
        .filter(|window| window.session_id == target);
    let Some(window) = matches.next() else {
        return Err(Failure::Stale);
    };
    if matches.next().is_some() {
        return Err(Failure::Ambiguous);
    }
    let root = reader.read(&window.native, true).await?;
    let snapshot_id = snapshot_id(target, &buses.bus_generation);
    let mut queue = std::collections::VecDeque::from([(root, None, 0_u8)]);
    let mut visited = std::collections::HashSet::new();
    let mut nodes = Vec::new();
    let mut reasons = BTreeSet::new();
    while let Some((record, parent_node_id, depth)) = queue.pop_front() {
        let native_key = (
            record.reference.owner.clone(),
            record.reference.path.clone(),
        );
        if !visited.insert(native_key) {
            reasons.insert(TruncationReason::CycleDetected);
            continue;
        }
        if nodes.len() >= maximum_items {
            reasons.insert(TruncationReason::ItemLimit);
            break;
        }
        let node_id = node_id(&snapshot_id, &record.reference);
        if record.name_truncated {
            reasons.insert(TruncationReason::NameLimit);
        }
        if !record.property_read_complete {
            reasons.insert(TruncationReason::PropertyError);
        }
        nodes.push(TreeNode {
            snapshot_id: snapshot_id.clone(),
            node_id: node_id.clone(),
            parent_node_id,
            depth,
            role: neutral_role(record.role),
            name: record.name,
            enabled: record.enabled,
            child_count: record.child_count,
            property_read_complete: record.property_read_complete,
        });
        if record.child_count == 0 {
            continue;
        }
        if depth == maximum_depth {
            reasons.insert(TruncationReason::DepthLimit);
            continue;
        }
        let remaining_slots = maximum_items.saturating_sub(nodes.len() + queue.len());
        let child_total = usize::try_from(record.child_count).unwrap_or(usize::MAX);
        let child_limit = child_total.min(remaining_slots);
        if child_total > remaining_slots {
            reasons.insert(TruncationReason::ItemLimit);
        }
        for index in 0..child_limit {
            let child = match reader.child_at(&record.reference, index as u32).await {
                Ok(value) => value,
                Err(Failure::Timeout) => return Err(Failure::Timeout),
                Err(_) => {
                    reasons.insert(TruncationReason::StaleDescendant);
                    continue;
                }
            };
            match reader.read(&child, false).await {
                Ok(child_record) => {
                    queue.push_back((child_record, Some(node_id.clone()), depth + 1))
                }
                Err(Failure::Timeout) => return Err(Failure::Timeout),
                Err(_) => {
                    reasons.insert(TruncationReason::StaleDescendant);
                }
            }
        }
    }
    Ok(TreeSnapshot {
        snapshot_id,
        nodes,
        reasons,
        audit: reader.audit,
    })
}

async fn inventory_with_reader(
    reader: &mut AccessibleReader<'_>,
    maximum_items: usize,
) -> Result<Vec<WindowRecord>, Failure> {
    let root = reader
        .reference(REGISTRY_NAME.to_owned(), REGISTRY_ROOT.to_owned())
        .await?;
    let child_count = reader.child_count(&root).await?;
    let mut windows = Vec::new();
    for index in 0..usize::try_from(child_count)
        .unwrap_or(usize::MAX)
        .min(maximum_items)
    {
        let child = reader.child_at(&root, index as u32).await?;
        let record = reader.read(&child, false).await?;
        let Some(role) = window_role(record.role) else {
            continue;
        };
        if record.visible
            && record.showing
            && !record.name.is_empty()
            && record.property_read_complete
        {
            windows.push(WindowRecord {
                session_id: window_session_id(&record.reference),
                role,
                title: record.name,
                name_truncated: record.name_truncated,
                native: record.reference,
            });
        }
    }
    Ok(windows)
}

async fn build_proxy(
    connection: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    deadline: &Deadline,
) -> Result<Proxy<'static>, Failure> {
    let builder = ProxyBuilder::<Proxy<'static>>::new(connection)
        .destination(destination.to_owned())
        .map_err(map_error)?
        .path(path.to_owned())
        .map_err(map_error)?
        .interface(interface.to_owned())
        .map_err(map_error)?
        .cache_properties(CacheProperties::No);
    within(deadline, builder.build()).await
}

async fn within<T, F>(deadline: &Deadline, operation: F) -> Result<T, Failure>
where
    F: Future<Output = zbus::Result<T>>,
{
    let remaining = deadline.remaining()?;
    future::race(async { operation.await.map_err(map_error) }, async {
        async_io::Timer::after(remaining).await;
        Err(Failure::Timeout)
    })
    .await
}

fn map_error(error: zbus::Error) -> Failure {
    match error {
        zbus::Error::MethodError(name, _, _) if name.as_str().contains("AccessDenied") => {
            Failure::PermissionDenied
        }
        zbus::Error::MethodError(name, _, _)
            if name.as_str().contains("UnknownObject")
                || name.as_str().contains("NameHasNoOwner") =>
        {
            Failure::Stale
        }
        zbus::Error::InputOutput(ref source) if source.kind() == std::io::ErrorKind::TimedOut => {
            Failure::Timeout
        }
        _ => Failure::AccessibilityUnavailable,
    }
}

fn state_contains(states: &[u32], state: usize) -> bool {
    states
        .get(state / 32)
        .is_some_and(|word| word & (1_u32 << (state % 32)) != 0)
}

fn truncate_name(value: &str) -> (String, bool) {
    if value.len() <= MAXIMUM_NAME_BYTES {
        return (value.to_owned(), false);
    }
    let mut boundary = MAXIMUM_NAME_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_owned(), true)
}

fn window_role(role: u32) -> Option<&'static str> {
    match role {
        16 => Some("dialog"),
        23 => Some("frame"),
        69 => Some("window"),
        _ => None,
    }
}

fn neutral_role(role: u32) -> &'static str {
    match role {
        16 => "dialog",
        23 => "frame",
        39 => "panel",
        43 => "button",
        61 => "text",
        69 => "window",
        75 => "application",
        _ => "other",
    }
}

fn window_session_id(reference: &NativeReference) -> String {
    OpaqueTargetId::new(
        OpaqueTargetKind::Window,
        &format!(
            "linux-atspi2-window\0{}\0{}\0{}",
            reference.bus_generation, reference.owner, reference.path
        ),
    )
    .to_string()
}

fn snapshot_id(target: &str, bus_generation: &str) -> String {
    let value = OpaqueTargetId::new(
        OpaqueTargetKind::Element,
        &format!(
            "linux-atspi2-snapshot\0{bus_generation}\0{target}\0{:?}",
            Instant::now()
        ),
    )
    .to_string();
    format!("as2:{}", &value[5..])
}

fn node_id(snapshot: &str, reference: &NativeReference) -> String {
    OpaqueTargetId::new(
        OpaqueTargetKind::Element,
        &format!(
            "linux-atspi2-node\0{snapshot}\0{}\0{}",
            reference.owner, reference.path
        ),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_bits_and_names_are_bounded() {
        let mut states = vec![0_u32; 1];
        states[0] |= 1 << 25;
        assert!(state_contains(&states, 25));
        assert!(!state_contains(&states, 30));
        let (name, truncated) = truncate_name(&"界".repeat(100));
        assert!(truncated);
        assert!(name.len() <= 256);
    }
}
