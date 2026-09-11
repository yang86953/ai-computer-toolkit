//! Linux 平台 provider 集合。

mod app;
#[cfg(feature = "linux-atspi-candidate")]
pub(crate) mod atspi;
mod desktop;
pub(crate) mod desktop_entries;
pub(crate) mod desktop_entry_launch;
pub(crate) mod desktop_frame_pipewire;
pub(crate) mod desktop_frame_subscription;
pub(crate) mod desktop_input_eis;
pub(crate) mod desktop_portal_authorization_store;
pub(crate) mod desktop_session_host_activity_logind;
pub(crate) mod desktop_session_portal;
#[cfg(feature = "linux-mpris-candidate")]
pub(crate) mod media_session_candidate;
pub(crate) mod mpris;
pub(crate) mod mpris_runtime;
mod process;
pub(crate) mod process_termination_pidfd;
pub(crate) mod procfs;
pub(crate) mod uix_agent;
pub(crate) mod wayland_portal;

pub use app::AppFacadeAdapter;
pub use desktop::DesktopAdapter;
pub use process::ProcessAdapter;
pub use window::{AccessibilityAdapter, WindowAdapter};

mod window;
