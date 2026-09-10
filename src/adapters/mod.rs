//! 可替换的后台适配器边界。

mod app;
mod browser;
mod desktop;
// 注册固定本机 broker 共用的 Windows IPC 与 peer 认证 Adapter。
pub(crate) mod fixed_local_ipc_windows;
// 注册固定 browser-session broker 的启动、连接与认证 Adapter。
pub(crate) mod browser_session_broker_windows;
// 注册仅供固定协议集成测试使用的认证 raw broker 连接 Adapter。
pub(crate) mod browser_session_broker_windows_fixture;
// 向键鼠输入 Module 公开共享的精确前景窗口 Component。
pub(crate) mod foreground_input_windows;
// 注册传统卸载 Registry 与 Shell AppsFolder 只读组件。
pub(crate) mod installed_applications;
// 注册独立交互会话 endpoint 的私有 Windows Adapter。
pub(crate) mod interactive_session_windows;
// 注册固定长操作 broker 的启动、连接与认证 Adapter。
pub(crate) mod long_operation_broker_windows;
// 向 Keyboard Input Module 公开私有 Windows 键位与注入 Adapter。
pub(crate) mod keyboard_input_windows;
mod notepad;
// 向 Text Document Module 公开固定 Notepad 启动适配器。
mod photoshop;
mod process;
// 向 Process Lifecycle Module 公开精确代际进程终止 Adapter。
pub(crate) mod process_termination_windows;
// 向 Permission Boundary Module 提供只读 Windows 会话与桌面姿态探针。
pub(crate) mod security_context_windows;
// 向 Pointer Input Module 公开窄 Windows 坐标与前台指针 Adapter。
pub(crate) mod pointer_input_windows;
// 注册只接受私有 Shell identity 的精确应用启动 Component。
pub(crate) mod shell_application_launch;
// 注册固定 Windows Start Menu Known Folder 的只读认证来源 Component。
pub(crate) mod start_menu_applications;
// 向 Standard Edit Module 公开固定消息与有界回读 Adapter。
pub(crate) mod standard_edit_windows;
pub(crate) mod text_document_windows;
mod uia;
// 向隔离录制 worker 公开固定 WGC 与项目自有编码 adapter。
pub(crate) mod video_recording;
mod win32_control;
// 向可访问性 Module 公开窗口重新发现与安全投影边界。
pub(crate) mod window;
// 向捕获 Module 公开不跨语言中立边界的 Windows capture Component。
pub(crate) mod window_capture;
// 向捕获预检 Module 公开零帧、零文件的 Windows 元数据探测 Component。
pub(crate) mod window_capture_preflight_windows;
// 向 Window Close Module 公开固定 WM_CLOSE Adapter。
pub(crate) mod window_close_windows;
// 向窗口 mutation Adapter 公开共享的精确代际身份 Component。
pub(crate) mod window_identity_windows;
// 向 Window Lifecycle Module 公开固定状态与外框几何 Adapter。
pub(crate) mod window_lifecycle_windows;
// 向其他 Adapter 公开共享 Windows backend。
pub(crate) mod windows;
// 注册共享 Windows backend 私有封闭错误码。
mod windows_error;

use std::{collections::HashMap, sync::Arc};

use serde_json::Value;

use crate::domain::{AppControlError, AppResult, CommandRequest};

pub use browser::BrowserAdapter;
pub use desktop::DesktopAdapter;
pub use notepad::NotepadAdapter;
pub use process::ProcessAdapter;
pub use uia::UiaAdapter;
pub use win32_control::Win32ControlAdapter;
pub use window::WindowAdapter;

pub trait AppAdapter: Send + Sync {
    fn app_id(&self) -> &'static str;
    fn status(&self) -> AppResult<Value>;
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value>;
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value>;
    fn run(&self, request: &CommandRequest) -> AppResult<Value>;
}

pub struct AdapterRegistry {
    adapters: HashMap<&'static str, Arc<dyn AppAdapter>>,
}

impl AdapterRegistry {
    pub fn adaptive() -> Self {
        let adapters: Vec<Arc<dyn AppAdapter>> = vec![
            Arc::new(AppFacadeAdapter::new()),
            // 注册 legacy uia 只读别名。
            Arc::new(UiaAdapter::new("uia")),
            // 注册语义化 accessibility 只读别名。
            Arc::new(UiaAdapter::new("accessibility")),
            Arc::new(WindowAdapter),
            Arc::new(ProcessAdapter),
            Arc::new(BrowserAdapter),
            Arc::new(NotepadAdapter),
            Arc::new(Win32ControlAdapter),
            Arc::new(DesktopAdapter),
            // 阶段六隔离 worker 认证前不装配旧主进程媒体适配器。
        ];
        Self {
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.app_id(), adapter))
                .collect(),
        }
    }

    pub fn get(&self, app_id: &str) -> AppResult<&Arc<dyn AppAdapter>> {
        self.adapters.get(app_id).ok_or_else(|| {
            AppControlError::new(
                "BACKGROUND_OPERATION_UNAVAILABLE",
                format!("应用 '{}' 没有已装载的控制适配器。", app_id),
            )
        })
    }
}
pub use app::AppFacadeAdapter;

pub(crate) mod desktop_session_windows;
