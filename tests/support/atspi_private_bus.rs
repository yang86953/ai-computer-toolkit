//! AT-SPI 候选验收专用的双私有 D-Bus 与工具自有 exporter。

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use zbus::{Connection, connection::Builder, zvariant::OwnedObjectPath};

#[derive(Default)]
pub struct Metrics {
    pub name: AtomicU64,
    pub role: AtomicU64,
    pub state: AtomicU64,
    pub child_count: AtomicU64,
    pub child_at: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct ChildRef {
    pub destination: String,
    pub path: String,
}

#[derive(Clone)]
pub struct NodeSpec {
    pub path: String,
    pub role: u32,
    pub name: Arc<Mutex<String>>,
    pub visible: bool,
    pub showing: bool,
    pub enabled: bool,
    pub children: Arc<Mutex<Vec<ChildRef>>>,
    pub child_count_override: Option<u32>,
    pub fail_name: bool,
    pub fail_child_at: bool,
    pub delay_ms: Arc<AtomicU64>,
}

impl NodeSpec {
    pub fn new(path: &str, role: u32, name: &str) -> Self {
        Self {
            path: path.to_owned(),
            role,
            name: Arc::new(Mutex::new(name.to_owned())),
            visible: true,
            showing: true,
            enabled: true,
            children: Arc::new(Mutex::new(Vec::new())),
            child_count_override: None,
            fail_name: false,
            fail_child_at: false,
            delay_ms: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[derive(Clone)]
struct AddressBroker {
    address: String,
    delay_ms: u64,
    deny: bool,
}

#[zbus::interface(name = "org.a11y.Bus")]
impl AddressBroker {
    async fn get_address(&self) -> zbus::fdo::Result<String> {
        if self.delay_ms > 0 {
            async_io::Timer::after(Duration::from_millis(self.delay_ms)).await;
        }
        if self.deny {
            return Err(zbus::fdo::Error::AccessDenied("fixture-denied".to_owned()));
        }
        Ok(self.address.clone())
    }
}

#[derive(Clone)]
struct AccessibleObject {
    spec: NodeSpec,
    metrics: Arc<Metrics>,
}

#[zbus::interface(name = "org.a11y.atspi.Accessible")]
impl AccessibleObject {
    #[zbus(property)]
    async fn name(&self) -> zbus::fdo::Result<String> {
        self.metrics.name.fetch_add(1, Ordering::Relaxed);
        self.delay().await;
        if self.spec.fail_name {
            return Err(zbus::fdo::Error::Failed("fixture-name-fault".to_owned()));
        }
        Ok(self.spec.name.lock().expect("name lock").clone())
    }

    #[zbus(property)]
    async fn child_count(&self) -> u32 {
        self.metrics.child_count.fetch_add(1, Ordering::Relaxed);
        self.delay().await;
        self.spec
            .child_count_override
            .unwrap_or_else(|| self.spec.children.lock().expect("children lock").len() as u32)
    }

    async fn get_role(&self) -> u32 {
        self.metrics.role.fetch_add(1, Ordering::Relaxed);
        self.delay().await;
        self.spec.role
    }

    async fn get_state(&self) -> Vec<u32> {
        self.metrics.state.fetch_add(1, Ordering::Relaxed);
        self.delay().await;
        let mut words = vec![0_u32];
        if self.spec.enabled {
            words[0] |= 1 << 8;
        }
        if self.spec.showing {
            words[0] |= 1 << 25;
        }
        if self.spec.visible {
            words[0] |= 1 << 30;
        }
        words
    }

    async fn get_child_at_index(&self, index: u32) -> zbus::fdo::Result<(String, OwnedObjectPath)> {
        self.metrics.child_at.fetch_add(1, Ordering::Relaxed);
        self.delay().await;
        if self.spec.fail_child_at {
            return Err(zbus::fdo::Error::UnknownObject(
                "fixture-child-missing".to_owned(),
            ));
        }
        let children = self.spec.children.lock().expect("children lock");
        let child = children.get(index as usize).ok_or_else(|| {
            zbus::fdo::Error::UnknownObject("fixture-child-index-missing".to_owned())
        })?;
        let path = OwnedObjectPath::try_from(child.path.clone())
            .map_err(|_| zbus::fdo::Error::Failed("fixture-path-invalid".to_owned()))?;
        Ok((child.destination.clone(), path))
    }
}

impl AccessibleObject {
    async fn delay(&self) {
        let delay_ms = self.spec.delay_ms.load(Ordering::Relaxed);
        if delay_ms > 0 {
            async_io::Timer::after(Duration::from_millis(delay_ms)).await;
        }
    }
}

pub struct PrivateBus {
    child: Child,
    pub address: String,
}

impl PrivateBus {
    pub fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().ok_or("dbus-daemon stdout missing")?;
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader.read_line(&mut address)?;
        let address = address.trim().to_owned();
        if !address.starts_with("unix:path=") {
            return Err("private bus returned a non-Unix address".into());
        }
        Ok(Self { child, address })
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct AtspiFixture {
    pub session_bus: PrivateBus,
    pub accessibility_bus: PrivateBus,
    _session_connection: Connection,
    registry_connection: Option<Connection>,
    exporter_connection: Option<Connection>,
    registry_children: Arc<Mutex<Vec<ChildRef>>>,
    registry_spec: NodeSpec,
    nodes: BTreeMap<String, NodeSpec>,
    pub metrics: Arc<Metrics>,
    pub exporter_name: String,
}

impl AtspiFixture {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_options(0, false, None, 0).await
    }

    pub async fn with_broker(
        delay_ms: u64,
        deny: bool,
        returned_address: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_options(delay_ms, deny, returned_address, 0).await
    }

    pub async fn with_registry_delay(delay_ms: u64) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_options(0, false, None, delay_ms).await
    }

    async fn with_options(
        delay_ms: u64,
        deny: bool,
        returned_address: Option<String>,
        registry_delay_ms: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let session_bus = PrivateBus::start()?;
        let accessibility_bus = PrivateBus::start()?;
        let broker = AddressBroker {
            address: returned_address.unwrap_or_else(|| accessibility_bus.address.clone()),
            delay_ms,
            deny,
        };
        let session_connection = Builder::address(session_bus.address.as_str())?
            .name("org.a11y.Bus")?
            .serve_at("/org/a11y/bus", broker)?
            .build()
            .await?;
        let registry_children = Arc::new(Mutex::new(Vec::new()));
        let registry_spec = NodeSpec {
            path: "/org/a11y/atspi/accessible/root".to_owned(),
            role: 14,
            name: Arc::new(Mutex::new("private-registry".to_owned())),
            visible: false,
            showing: false,
            enabled: true,
            children: registry_children.clone(),
            child_count_override: None,
            fail_name: false,
            fail_child_at: false,
            delay_ms: Arc::new(AtomicU64::new(registry_delay_ms)),
        };
        let metrics = Arc::new(Metrics::default());
        let registry_path = registry_spec.path.clone();
        let registry_connection = Builder::address(accessibility_bus.address.as_str())?
            .name("org.a11y.atspi.Registry")?
            .serve_at(
                registry_path.as_str(),
                AccessibleObject {
                    spec: registry_spec.clone(),
                    metrics: metrics.clone(),
                },
            )?
            .build()
            .await?;
        let exporter_name = "org.ai_computer_toolkit.PrivateExporter".to_owned();
        let exporter_connection = Builder::address(accessibility_bus.address.as_str())?
            .name(exporter_name.as_str())?
            .build()
            .await?;
        Ok(Self {
            session_bus,
            accessibility_bus,
            _session_connection: session_connection,
            registry_connection: Some(registry_connection),
            exporter_connection: Some(exporter_connection),
            registry_children,
            registry_spec,
            nodes: BTreeMap::new(),
            metrics,
            exporter_name,
        })
    }

    pub async fn add_node(
        &mut self,
        spec: NodeSpec,
        top_level: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let connection = self
            .exporter_connection
            .as_ref()
            .ok_or("exporter stopped")?;
        connection
            .object_server()
            .at(
                spec.path.as_str(),
                AccessibleObject {
                    spec: spec.clone(),
                    metrics: self.metrics.clone(),
                },
            )
            .await?;
        if top_level {
            self.registry_children
                .lock()
                .expect("registry children lock")
                .push(ChildRef {
                    destination: self.exporter_name.clone(),
                    path: spec.path.clone(),
                });
        }
        self.nodes.insert(spec.path.clone(), spec);
        Ok(())
    }

    pub fn add_registry_reference(&self, destination: &str, path: &str) {
        self.registry_children
            .lock()
            .expect("registry children lock")
            .push(ChildRef {
                destination: destination.to_owned(),
                path: path.to_owned(),
            });
    }

    pub fn duplicate_top_reference(&self, path: &str) {
        self.add_registry_reference(self.exporter_name.as_str(), path);
    }

    pub fn registry_children_for_test_clear_duplicate(&self) {
        let mut children = self
            .registry_children
            .lock()
            .expect("registry children lock");
        children.dedup_by(|left, right| {
            left.destination == right.destination && left.path == right.path
        });
    }

    pub async fn stop_exporter(&mut self) {
        self.exporter_connection.take();
        async_io::Timer::after(Duration::from_millis(40)).await;
    }

    pub async fn stop_registry(&mut self) {
        self.registry_connection.take();
        async_io::Timer::after(Duration::from_millis(40)).await;
    }

    pub async fn restart_registry(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.stop_registry().await;
        let connection = Builder::address(self.accessibility_bus.address.as_str())?
            .name("org.a11y.atspi.Registry")?
            .serve_at(
                self.registry_spec.path.as_str(),
                AccessibleObject {
                    spec: self.registry_spec.clone(),
                    metrics: self.metrics.clone(),
                },
            )?
            .build()
            .await?;
        self.registry_connection = Some(connection);
        Ok(())
    }

    pub async fn restart_exporter(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.stop_exporter().await;
        let connection = Builder::address(self.accessibility_bus.address.as_str())?
            .name(self.exporter_name.as_str())?
            .build()
            .await?;
        for spec in self.nodes.values() {
            connection
                .object_server()
                .at(
                    spec.path.as_str(),
                    AccessibleObject {
                        spec: spec.clone(),
                        metrics: self.metrics.clone(),
                    },
                )
                .await?;
        }
        self.exporter_connection = Some(connection);
        Ok(())
    }
}

pub fn child(destination: &str, path: &str) -> ChildRef {
    ChildRef {
        destination: destination.to_owned(),
        path: path.to_owned(),
    }
}

pub fn count(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}
