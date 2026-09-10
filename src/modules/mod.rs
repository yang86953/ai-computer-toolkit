//! 组合窄 Component 并实现领域行为的 Module 集合。

// 导出隔离可访问性观察 Module 供 service 与兼容 adapter 协调。
pub(crate) mod accessibility;
// 导出 provider-neutral UIA 元素等待 Module。
pub(crate) mod accessibility_wait;
// 导出 provider-neutral UIA 元素定位 Module。
pub(crate) mod element_location;
// 导出 provider-neutral 语义元素动作 Module。
pub(crate) mod semantic_action;
// 导出 sequence execution 持久记录与恢复状态机 Module。
// #2391 分批接入原子 journal 和生产 broker 前保留纯领域实现。
#[allow(dead_code)]
pub(crate) mod sequence_execution;
// 导出只读应用关系图 Module 供 service 协调。
pub(crate) mod application_discovery;
// 注册动态已安装应用认证与精确启动 Module。
pub(crate) mod application_launch;
// 导出构建元数据 Module 供 service 协调。
pub(crate) mod build_info;
// 导出隔离浏览器截图 Module 供 legacy browser facade 协调。
pub(crate) mod browser_screenshot;
// 导出浏览器会话公开路线的无状态固定 broker 客户端 Module。
pub(crate) mod browser_session_client;
// 导出浏览器会话 Module 供后续公开 capability 协调。
pub(crate) mod browser_session;
// 导出 capability 迁移元数据 Module 供 service 协调。
pub(crate) mod capability_metadata;
// 导出无副作用 capability assessment Module 供 service 协调。
pub(crate) mod capability_assessment;
// 导出通用请求内有界键盘输入 Module。
pub(crate) mod keyboard_input;
// 导出长操作任务状态机与有界生命周期 Module。
// #2014 接入持久 registry 前保留已冻结但尚无生产调用方的领域实现。
#[allow(dead_code)]
pub(crate) mod long_operation;
// 导出长操作任务有界 registry Module。
// #2017 接入固定 broker 前保留当前仅由回归测试使用的实现。
#[allow(dead_code)]
pub(crate) mod long_operation_registry;
// 导出 broker 所有的异步窗口录制任务生命周期 Module。
pub(crate) mod long_operation_recording;
// 导出公开 status/cancel 的长操作客户端 Module。
pub(crate) mod long_operation_client;
// 导出认证独立交互会话发现与单次 Command Module。
pub(crate) mod interactive_isolation;
// 导出通用请求内有界指针输入 Module。
pub(crate) mod pointer_input;
// 导出受保护上下文与活动交互会话的 Permission Assessment Module。
pub(crate) mod permission_boundary;
// 导出精确进程优雅与强制终止 Module。
pub(crate) mod process_termination;
// 导出 Standard Edit 领域 Module。
pub(crate) mod standard_edit;
// 导出受控文本文档创建 Module。
pub(crate) mod text_document;
// 注册精确窗口关闭领域 Module。
pub(crate) mod window_close;
// 注册通用窗口状态与几何生命周期 Module。
pub(crate) mod window_lifecycle;
// 导出持久窗口代际 owner 的纯 registry Module。
// #2336 只冻结状态机，后续子任务再接 Windows 事件和生产路由。
#[allow(dead_code)]
pub(crate) mod window_generation_registry;
// 导出精确窗口关闭等待 Module。
pub(crate) mod window_closed_wait;
// 导出精确窗口首帧元数据探针 Module 供 service 协调。
pub(crate) mod window_capture_frame_probe;
// 导出精确窗口零帧捕获预检 Module 供 service 协调。
pub(crate) mod window_capture_preflight;
// 导出精确窗口原子截图 Module 供 app facade 协调。
pub(crate) mod window_screenshot;
// 导出精确窗口录制 Module 供 app 与 legacy facade 协调。
pub(crate) mod window_record;

// CLI 与 MCP 共用的桌面会话生命周期，平台通过 lease 接口接入。
pub(crate) mod desktop_session;
