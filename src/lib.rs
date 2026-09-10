//! 后台专用应用控制的库入口。

#[cfg_attr(not(target_os = "windows"), path = "adapters_linux.rs")]
pub mod adapters;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub mod catalog;
#[cfg_attr(not(target_os = "windows"), path = "cli_linux.rs")]
pub mod cli;
// 导出不含原生类型的捕获探针 worker stdio 入口。
#[cfg(target_os = "windows")]
pub mod capture_worker;
// Linux 发布 System 同时服务 archive 构建工具与安装包内固定 installer 入口。
#[cfg(target_os = "linux")]
pub mod linux_release;
// Linux Portal lease 必须由单线程长期进程持有；公开入口只暴露 JSONL stdio 宿主。
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[doc(hidden)]
pub mod desktop_session_broker;
// Linux AT-SPI 候选 worker 只在显式非默认 feature 下编译，默认发布不包含。
#[cfg(all(target_os = "linux", feature = "linux-atspi-candidate"))]
#[doc(hidden)]
pub mod atspi_observation_worker;
// 私有 fixture launcher 只验证 kill+wait、取消映射与终态发布，不进入 AppControlService。
#[cfg(all(target_os = "linux", feature = "linux-atspi-candidate"))]
#[doc(hidden)]
pub mod atspi_candidate_client;
// 候选 worker 不允许在 Windows 或其他平台形成 stub。
#[cfg(all(feature = "linux-atspi-candidate", not(target_os = "linux")))]
compile_error!("linux-atspi-candidate feature is only supported on Linux targets");
// MPRIS 候选不得在非 Linux target 生成 stub。
#[cfg(all(feature = "linux-mpris-candidate", not(target_os = "linux")))]
compile_error!("linux-mpris-candidate feature is only supported on Linux targets");
// 私有地址候选协议仅供合成 D-Bus fixture 调用，生产组合根不路由。
#[cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#[doc(hidden)]
pub mod mpris_candidate;
// 生产 MPRIS 只读 self-worker；公开 Rust 边界仅为固定 stdio 入口。
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub mod linux_media_observation_worker;
// 私有 MPRIS worker launcher 只在显式 candidate feature 下导出。
#[cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#[doc(hidden)]
pub mod mpris_candidate_client;
#[cfg(all(target_os = "linux", not(feature = "linux-mpris-candidate")))]
mod mpris_candidate_client;
// 生产 MPRIS 写 self-worker 与只读 worker 保持进程和协议分离。
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub mod linux_media_control_worker;
// 两阶段 MPRIS control launcher 只在显式 candidate feature 下导出。
#[cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#[doc(hidden)]
pub mod mpris_control_candidate_client;
#[cfg(all(target_os = "linux", not(feature = "linux-mpris-candidate")))]
mod mpris_control_candidate_client;
// 生产只使用 current-user 方法；显式私有地址辅助入口仍仅向 candidate fixture 导出。
#[cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#[doc(hidden)]
pub mod mpris_runtime_client;
#[cfg(all(target_os = "linux", not(feature = "linux-mpris-candidate")))]
mod mpris_runtime_client;
// Windows production binaries 不允许在非 Windows 上生成 unavailable stub。
#[cfg(all(feature = "windows-workers", not(target_os = "windows")))]
compile_error!("windows-workers feature is only supported on Windows targets");
// 导出不含公共原生类型的浏览器截图 worker stdio 入口。
#[cfg(target_os = "windows")]
pub mod browser_worker;
// 公开固定 sibling 二进制所需的私有浏览器会话 worker 入口。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_worker;
// 导出固定同会话 browser-session broker 组合根入口。
#[cfg(target_os = "windows")]
pub mod browser_session_broker;
// 公开固定 transport 集成测试 executable 所需的隐藏入口。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_broker_transport_fixture;
// 公开固定 command 集成测试 executable 所需的隐藏入口。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_broker_command_fixture;
// 公开固定 raw protocol 集成测试 executable 所需的隐藏入口。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_broker_raw_protocol_fixture;
// 公开固定 OutcomeUnknown 集成测试 executable 所需的隐藏入口。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_broker_outcome_unknown_fixture;
// 公开仓库固定 Job 客户端 fixture 所需的测试入口，不形成公开 capability。
#[doc(hidden)]
#[cfg(target_os = "windows")]
pub mod browser_session_client_fixture;
// 导出仅由固定集成测试驱动调用的 Browser Session Module fixture。
#[cfg(target_os = "windows")]
pub(crate) mod browser_session_module_fixture;
pub mod domain;
// 导出固定独立交互会话 command worker 的 stdio 入口。
#[cfg(target_os = "windows")]
pub mod interactive_command_worker;
// 导出固定独立交互会话 session broker 入口。
#[cfg(target_os = "windows")]
pub mod interactive_session_broker;
// 导出固定同会话长操作 broker 入口。
#[cfg(target_os = "windows")]
pub mod long_operation_broker;
pub mod methods;
// 导出不含原生类型的 companion worker stdio 入口。
#[cfg(target_os = "windows")]
pub mod observation_worker;
// 导出不含公共原生类型的语义动作 worker stdio 入口。
#[cfg(target_os = "windows")]
pub mod semantic_action_worker;
// 导出固定 sequence step worker 的 JSON Lines stdio 入口；Linux 由主映像隐藏模式承载。
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub mod sequence_step_worker;
// 导出不含公共原生类型的录制 worker stdio 入口。
#[cfg_attr(not(target_os = "windows"), path = "policy_linux.rs")]
pub mod policy;
#[cfg(target_os = "windows")]
pub mod recording_worker;
#[cfg_attr(not(target_os = "windows"), path = "service_linux.rs")]
pub mod service;
// 新任务宿主只发布 provider-neutral 的版本化 stdio 入口。
pub mod task_control;
// 公开控制面：标准 MCP stdio 服务。
pub mod mcp;

// Linux 首批纵切不装配 Windows worker；保留同名二进制入口并稳定失败闭合。
#[cfg(not(target_os = "windows"))]
#[doc(hidden)]
pub mod unsupported_worker_linux {
    fn emit_unavailable() {
        println!(
            "{}",
            r#"{"ok":false,"error":{"code":"CAPABILITY_UNAVAILABLE","message":"This worker requires a certified Windows provider.","details":{"platform":"linux","executionRealm":"none","fallback":"none"}}}"#
        );
    }

    /// Windows-only worker 在 Linux 上不得链接、启动或伪报可用。
    pub fn run_stdio() -> i32 {
        emit_unavailable();
        2
    }

    /// Windows-only broker 在 Linux 上不得链接、启动或伪报可用。
    pub fn run() -> i32 {
        emit_unavailable();
        2
    }
}

#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_broker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_broker_command_fixture;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_broker_outcome_unknown_fixture;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_broker_raw_protocol_fixture;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_broker_transport_fixture;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_client_fixture;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_session_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as browser_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as capture_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as interactive_command_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as interactive_session_broker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as long_operation_broker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as observation_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as recording_worker;
#[cfg(not(target_os = "windows"))]
pub use unsupported_worker_linux as semantic_action_worker;
#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub use unsupported_worker_linux as sequence_step_worker;

// 注册版本化 capability 的 Rust 运行时单一来源。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod capabilities;

// 注册供各领域 Module 复用的窄 Component 集合。
#[cfg_attr(not(target_os = "windows"), path = "components_linux.rs")]
mod components;

// 注册负责组合领域行为的 Module 集合。
#[cfg_attr(not(target_os = "windows"), path = "modules_linux.rs")]
mod modules;

#[cfg(target_os = "windows")]
mod recording;

pub use domain::{AppControlError, AppResult};
pub use service::AppControlService;

// 为 CLI 二进制暴露不含原生类型的取消处理器入口。
pub fn install_cancellation_handler() {
    // 委托窄 Component 安装控制台处理器。
    components::cancellation::install_console_handler();
}
