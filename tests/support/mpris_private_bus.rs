//! MPRIS 候选验收专用私有总线与最小 fake Player。
use std::{
    io::{self, BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use zbus::{Connection, connection::Builder};

const OWNER_CHURN_NAME: &str = "org.mpris.MediaPlayer2.ownerChurn";
const OWNER_CHURN_READY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Metrics {
    pub properties: AtomicU64,
    pub methods: AtomicU64,
}

#[derive(Clone)]
struct Player {
    metrics: Arc<Metrics>,
    status: String,
    can_control: bool,
    can_play: bool,
    can_pause: bool,
    can_next: bool,
    can_previous: bool,
    property_delay_ms: u64,
    method_delay_ms: u64,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    async fn playback_status(&self) -> String {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.status.clone()
    }
    #[zbus(property)]
    async fn can_control(&self) -> bool {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.can_control
    }
    #[zbus(property)]
    async fn can_play(&self) -> bool {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.can_play
    }
    #[zbus(property)]
    async fn can_pause(&self) -> bool {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.can_pause
    }
    #[zbus(property)]
    async fn can_go_next(&self) -> bool {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.can_next
    }
    #[zbus(property)]
    async fn can_go_previous(&self) -> bool {
        self.property_delay().await;
        self.metrics.properties.fetch_add(1, Ordering::Relaxed);
        self.can_previous
    }
    async fn play(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
    async fn pause(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
    async fn play_pause(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
    async fn stop(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
    async fn next(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
    async fn previous(&self) {
        self.method_delay().await;
        self.metrics.methods.fetch_add(1, Ordering::Relaxed);
    }
}

impl Player {
    async fn property_delay(&self) {
        if self.property_delay_ms > 0 {
            async_io::Timer::after(std::time::Duration::from_millis(self.property_delay_ms)).await;
        }
    }
    async fn method_delay(&self) {
        if self.method_delay_ms > 0 {
            async_io::Timer::after(std::time::Duration::from_millis(self.method_delay_ms)).await;
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
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("missing bus output")?).read_line(&mut line)?;
        let address = line.trim().to_owned();
        if !address.starts_with("unix:path=") {
            return Err("non-private bus".into());
        }
        Ok(Self { child, address })
    }
}
impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 在独立连接上持续占有/释放 MPRIS 名称，覆盖 observation 的订阅—枚举窗口。
pub struct OwnerChurn {
    stop: Arc<std::sync::atomic::AtomicBool>,
    cycles: Arc<AtomicU64>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

impl OwnerChurn {
    /// 连接与首轮 request/release 均就绪后才返回，避免单点 sleep 伪造覆盖证据。
    pub fn start(address: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cycles = Arc::new(AtomicU64::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_cycles = Arc::clone(&cycles);
        let bus_address = address.to_owned();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let error_tx = ready_tx.clone();
            let result = async_io::block_on(async move {
                let connection = Builder::address(bus_address.as_str())
                    .map_err(|error| error.to_string())?
                    .build()
                    .await
                    .map_err(|error| error.to_string())?;
                churn_once(&connection).await?;
                thread_cycles.fetch_add(1, Ordering::Release);
                ready_tx
                    .send(Ok(()))
                    .map_err(|_| "owner churn readiness receiver dropped".to_owned())?;
                while !thread_stop.load(Ordering::Acquire) {
                    churn_once(&connection).await?;
                    thread_cycles.fetch_add(1, Ordering::Release);
                    async_io::Timer::after(Duration::from_millis(1)).await;
                }
                Ok::<(), String>(())
            });
            if let Err(error) = &result {
                let _ = error_tx.send(Err(error.clone()));
            }
            result
        });
        match ready_rx.recv_timeout(OWNER_CHURN_READY_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stop,
                cycles,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(io::Error::other(error).into())
            }
            Err(RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(io::Error::other("owner churn readiness timed out").into())
            }
            Err(RecvTimeoutError::Disconnected) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(io::Error::other("owner churn readiness channel closed").into())
            }
        }
    }

    pub fn cycles(&self) -> u64 {
        self.cycles.load(Ordering::Acquire)
    }

    /// 停止并 join churn 线程，确保独立连接和名称均已清理。
    pub fn stop_and_join(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.stop.store(true, Ordering::Release);
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        match thread.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(io::Error::other(error).into()),
            Err(_) => Err(io::Error::other("owner churn thread panicked").into()),
        }
    }
}

impl Drop for OwnerChurn {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

async fn churn_once(connection: &Connection) -> Result<(), String> {
    connection
        .request_name(OWNER_CHURN_NAME)
        .await
        .map_err(|error| error.to_string())?;
    async_io::Timer::after(Duration::from_millis(2)).await;
    connection
        .release_name(OWNER_CHURN_NAME)
        .await
        .map_err(|error| error.to_string())?;
    async_io::Timer::after(Duration::from_millis(2)).await;
    Ok(())
}

pub struct Fixture {
    pub bus: PrivateBus,
    pub metrics: Arc<Metrics>,
    connections: Vec<Connection>,
}
impl Fixture {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let bus = PrivateBus::start()?;
        let metrics = Arc::new(Metrics::default());
        let player = Player {
            metrics: metrics.clone(),
            status: "Playing".to_owned(),
            can_control: true,
            can_play: true,
            can_pause: true,
            can_next: true,
            can_previous: false,
            property_delay_ms: 0,
            method_delay_ms: 0,
        };
        let connection = Builder::address(bus.address.as_str())?
            .name("org.mpris.MediaPlayer2.fixture")?
            .serve_at("/org/mpris/MediaPlayer2", player)?
            .build()
            .await?;
        Ok(Self {
            bus,
            metrics,
            connections: vec![connection],
        })
    }
    pub async fn stop(&mut self) {
        self.connections.clear();
        async_io::Timer::after(std::time::Duration::from_millis(30)).await;
    }
    pub async fn add_alias(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.connections
            .first()
            .ok_or("fixture stopped")?
            .request_name("org.mpris.MediaPlayer2.fixtureAlias")
            .await?;
        Ok(())
    }

    /// 为 owner 变化回归扩大真实的 MPRIS 名称枚举窗口，不触发 Player I/O。
    pub async fn add_mpris_aliases(&self, count: u32) -> Result<(), Box<dyn std::error::Error>> {
        let connection = self.connections.first().ok_or("fixture stopped")?;
        for index in 0..count {
            let name = format!("org.mpris.MediaPlayer2.enumerationAlias{index}");
            connection.request_name(name).await?;
        }
        Ok(())
    }

    pub async fn add_player(
        &mut self,
        name: &str,
        status: &str,
        can_control: bool,
        delay_ms: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let player = Player {
            metrics: self.metrics.clone(),
            status: status.to_owned(),
            can_control,
            can_play: can_control,
            can_pause: can_control,
            can_next: can_control,
            can_previous: can_control,
            property_delay_ms: 0,
            method_delay_ms: delay_ms,
        };
        let connection = Builder::address(self.bus.address.as_str())?
            .name(name)?
            .serve_at("/org/mpris/MediaPlayer2", player)?
            .build()
            .await?;
        self.connections.push(connection);
        Ok(())
    }
}
