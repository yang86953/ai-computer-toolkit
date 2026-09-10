//! 稳定 capability ID 目录；不拥有运行时路由与策略元数据。

// 定义应用打开 capability 的稳定 ID。
pub(crate) const APPLICATION_OPEN: &str = "application.open@1";
// 定义 Linux 认证 Desktop Entry 工具自有夹具启动的独立版本二 ID。
pub(crate) const APPLICATION_OPEN_V2: &str = "application.open@2";
// 定义画布创建 capability 的稳定 ID。
pub(crate) const IMAGE_CANVAS_CREATE: &str = "image.canvas.create@1";
// 定义文本文档创建 capability 的稳定 ID。
pub(crate) const TEXT_DOCUMENT_CREATE: &str = "text.document.create@1";
// 定义图层变更 capability 的稳定 ID。
pub(crate) const IMAGE_LAYERS_APPLY: &str = "image.layers.apply@1";
// 定义文本输入 capability 的稳定 ID。
pub(crate) const UI_TEXT_INPUT: &str = "ui.text.input@1";
// 定义键盘输入 capability 的稳定 ID。
pub(crate) const UI_INPUT_KEY: &str = "ui.input.key@1";
// 定义 UIX 协作式应用内按键 capability 的稳定版本二 ID。
pub(crate) const UI_INPUT_KEY_V2: &str = "ui.input.key@2";
// 定义 UIX 应用内请求级成对按键序列 capability。
pub(crate) const UI_INPUT_KEY_SEQUENCE: &str = "ui.input.key.sequence@1";
/// 定义 UIX 完整按键序列与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_KEY_SEQUENCE_TRANSITION: &str = "ui.input.key.sequence.transition@1";
/// UIX 单次完整按键与提交后语义条件的同连接能力。
pub(crate) const UI_INPUT_KEY_TRANSITION: &str = "ui.input.key.transition@1";
// 定义 UIX 应用内请求级键盘与指针混合序列 capability。
pub(crate) const UI_INPUT_SEQUENCE: &str = "ui.input.sequence@1";
/// 定义 UIX 跨模态输入序列与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_SEQUENCE_TRANSITION: &str = "ui.input.sequence.transition@1";
/// Portal 授权桌面会话内的主机前台键盘输入。
pub(crate) const UI_INPUT_KEY_V3: &str = "ui.input.key@3";
// 定义指针输入 capability 的稳定 ID。
pub(crate) const UI_INPUT_POINTER: &str = "ui.input.pointer@1";
// 定义 UIX 协作式应用内指针 capability 的稳定版本二 ID。
pub(crate) const UI_INPUT_POINTER_V2: &str = "ui.input.pointer@2";
// 定义 UIX 应用内普通左键点击序列 capability。
pub(crate) const UI_INPUT_POINTER_CLICK_SEQUENCE: &str = "ui.input.pointer.click.sequence@1";
/// 定义 UIX 完整普通左键点击序列与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION: &str =
    "ui.input.pointer.click.sequence.transition@1";
// 定义 UIX 普通左键 click 与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_CLICK_TRANSITION: &str = "ui.input.pointer.click.transition@1";
/// 定义 UIX 单次 pointer_move 与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_MOVE_TRANSITION: &str = "ui.input.pointer.move.transition@1";
// 定义 UIX 应用内纯移动点序列 capability。
pub(crate) const UI_INPUT_POINTER_MOVE_SEQUENCE: &str = "ui.input.pointer.move.sequence@1";
/// 定义 UIX 完整移动点序列与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION: &str =
    "ui.input.pointer.move.sequence.transition@1";
// 定义 UIX 应用内悬停—普通左键点击混合序列 capability。
pub(crate) const UI_INPUT_POINTER_SEQUENCE: &str = "ui.input.pointer.sequence@1";
/// 定义 UIX 悬停—点击序列与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_SEQUENCE_TRANSITION: &str =
    "ui.input.pointer.sequence.transition@1";
// 定义 UIX 应用内请求级配平拖拽 capability。
pub(crate) const UI_INPUT_POINTER_DRAG: &str = "ui.input.pointer.drag@1";
/// 定义 UIX 配平拖拽与提交后语义条件同步 capability。
pub(crate) const UI_INPUT_POINTER_DRAG_TRANSITION: &str = "ui.input.pointer.drag.transition@1";
/// Portal 授权桌面会话内的主机前台相对指针输入。
pub(crate) const UI_INPUT_POINTER_V3: &str = "ui.input.pointer@3";
// 定义制品保存 capability 的稳定 ID。
pub(crate) const ARTIFACT_SAVE: &str = "artifact.save@1";
// 定义图像导出 capability 的稳定 ID。
pub(crate) const IMAGE_EXPORT: &str = "image.export@1";
// 定义文档关闭 capability 的稳定 ID。
pub(crate) const DOCUMENT_CLOSE: &str = "document.close@1";
// 定义窗口关闭 capability 的稳定 ID。
pub(crate) const WINDOW_CLOSE: &str = "window.close@1";
// 定义协作式 UIX Agent 精确窗口关闭请求 capability。
pub(crate) const WINDOW_CLOSE_V2: &str = "window.close@2";
// 定义 UIX 关闭请求与精确 generation 终态同连接同步 capability。
pub(crate) const WINDOW_CLOSE_TRANSITION: &str = "window.close.transition@1";
// 定义窗口状态与几何生命周期 capability 的稳定 ID。
pub(crate) const WINDOW_LIFECYCLE: &str = "window.lifecycle@1";
// 定义 UIX 协作式窗口生命周期 capability 的稳定版本二 ID。
pub(crate) const WINDOW_LIFECYCLE_V2: &str = "window.lifecycle@2";
// 定义 UIX 同连接窗口生命周期动作序列 capability。
pub(crate) const WINDOW_LIFECYCLE_SEQUENCE: &str = "window.lifecycle.sequence@1";
/// 定义同连接完整生命周期序列与框架状态条件同步 capability。
pub(crate) const WINDOW_LIFECYCLE_SEQUENCE_TRANSITION: &str =
    "window.lifecycle.sequence.transition@1";
// 定义同连接生命周期动作与框架状态等待 capability。
pub(crate) const WINDOW_LIFECYCLE_TRANSITION: &str = "window.lifecycle.transition@1";
// 定义显式请求前台聚焦的精确窗口 capability。
pub(crate) const WINDOW_ACTIVATE: &str = "window.activate@1";
/// UIX 精确窗口激活请求与提交后焦点观察的同连接能力。
pub(crate) const WINDOW_ACTIVATE_TRANSITION: &str = "window.activate.transition@1";
// 定义窗口截图 capability 的稳定 ID。
pub(crate) const WINDOW_SCREENSHOT: &str = "window.screenshot@1";
// 定义 UIX 协作式应用表面截图 capability 的稳定版本二 ID。
pub(crate) const WINDOW_SCREENSHOT_V2: &str = "window.screenshot@2";
// 定义 Wayland Portal 交互式主机截图 capability 的稳定 ID。
pub(crate) const DESKTOP_SCREENSHOT_INTERACTIVE: &str = "desktop.screenshot.interactive@1";
// 定义 Wayland Portal 桌面授权会话打开 capability。
pub(crate) const DESKTOP_SESSION_OPEN: &str = "desktop.session.open@1";
// 定义 Wayland Portal 桌面授权会话关闭 capability。
pub(crate) const DESKTOP_SESSION_CLOSE: &str = "desktop.session.close@1";
// 定义既有 Portal 桌面会话内的单帧屏幕捕获 capability。
pub(crate) const SCREEN_CAPTURE: &str = "screen.capture@1";
pub(crate) const DESKTOP_OBSERVE: &str = "desktop.observe@1";
pub(crate) const DESKTOP_INTERACTION: &str = "desktop.interaction@1";
// 定义隔离浏览器截图 capability 的稳定 ID。
pub(crate) const BROWSER_SCREENSHOT: &str = "browser.screenshot@1";
// 定义浏览器会话打开 capability 的稳定 ID。
pub(crate) const BROWSER_SESSION_OPEN: &str = "browser.session.open@1";
// 定义浏览器会话关闭 capability 的稳定 ID。
pub(crate) const BROWSER_SESSION_CLOSE: &str = "browser.session.close@1";
// 定义浏览器页面导航 capability 的稳定 ID。
pub(crate) const BROWSER_PAGE_NAVIGATE: &str = "browser.page.navigate@1";
// 定义浏览器页面等待 capability 的稳定 ID。
pub(crate) const BROWSER_PAGE_WAIT: &str = "browser.page.wait@1";
// 定义浏览器页面查询 capability 的稳定 ID。
pub(crate) const BROWSER_PAGE_QUERY: &str = "browser.page.query@1";
// 定义浏览器元素点击 capability 的稳定 ID。
pub(crate) const BROWSER_ELEMENT_CLICK: &str = "browser.element.click@1";
// 定义浏览器元素输入 capability 的稳定 ID。
pub(crate) const BROWSER_ELEMENT_TYPE: &str = "browser.element.type@1";
// 定义浏览器页面截图 capability 的稳定 ID。
pub(crate) const BROWSER_PAGE_SCREENSHOT: &str = "browser.page.screenshot@1";
// 定义窗口录制 capability 的稳定 ID。
pub(crate) const WINDOW_RECORD: &str = "window.record@1";
// 定义完整应用关系图发现 capability 的稳定 ID。
pub(crate) const APPLICATION_DISCOVER: &str = "application.discover@1";
// 定义 Linux XDG Desktop Entry 与独立 procfs 清单的版本二 capability。
pub(crate) const APPLICATION_DISCOVER_V2: &str = "application.discover@2";
// 定义与 Linux application.open@2 原子发布启动状态的版本三清单。
pub(crate) const APPLICATION_DISCOVER_V3: &str = "application.discover@3";
// 定义统一应用 session 聚合发现 capability 的稳定 ID。
pub(crate) const APPLICATION_SESSION_DISCOVER: &str = "application.session.discover@1";
// 定义 Linux XDG/procfs 独立聚合的版本二应用 session 发现 capability。
pub(crate) const APPLICATION_SESSION_DISCOVER_V2: &str = "application.session.discover@2";
// 定义聚合 XDG、procfs 与协作式 UIX 窗口关系的版本三 capability。
pub(crate) const APPLICATION_SESSION_DISCOVER_V3: &str = "application.session.discover@3";
// 定义聚合 XDG 启动状态、procfs 与协作式 UIX 窗口的版本四 capability。
pub(crate) const APPLICATION_SESSION_DISCOVER_V4: &str = "application.session.discover@4";
// 定义认证独立交互会话发现 capability 的稳定 ID。
pub(crate) const INTERACTIVE_SESSION_DISCOVER: &str = "interactive.session.discover@1";
// 定义有界只读可访问性树 capability 的稳定 ID。
pub(crate) const ACCESSIBILITY_TREE_READ: &str = "accessibility.tree.read@1";
// 定义 Linux AT-SPI 中立只读树候选 capability；不得进入 UIA v1 路由。
pub(crate) const ACCESSIBILITY_TREE_READ_V2: &str = "accessibility.tree.read@2";
// 定义协作式 UIX Agent 中立只读树 capability；不扩写 AT-SPI v2 语义。
pub(crate) const ACCESSIBILITY_TREE_READ_V3: &str = "accessibility.tree.read@3";
// 定义 provider-neutral 语义元素定位 capability。
pub(crate) const UI_ELEMENT_LOCATE: &str = "ui.element.locate@1";
// 定义协作式 UIX 快照上的版本二语义元素定位 capability。
pub(crate) const UI_ELEMENT_LOCATE_V2: &str = "ui.element.locate@2";
// 定义 provider-neutral 语义元素动作 capability。
pub(crate) const UI_ELEMENT_ACTION: &str = "ui.element.action@1";
// 定义绑定协作式语义快照与精确元素身份的版本二动作 capability。
pub(crate) const UI_ELEMENT_ACTION_V2: &str = "ui.element.action@2";
// 定义有界可取消的语义元素等待 capability。
pub(crate) const UI_ELEMENT_WAIT: &str = "ui.element.wait@1";
// 定义 UIX 同连接语义修订等待 capability，不扩写隔离 UIA v1。
pub(crate) const UI_ELEMENT_WAIT_V2: &str = "ui.element.wait@2";
// 定义 UIX 精确语义动作与提交后条件同步 capability。
pub(crate) const UI_ELEMENT_TRANSITION: &str = "ui.element.transition@1";
// 定义有界可取消的精确窗口关闭等待 capability。
pub(crate) const WINDOW_CLOSED_WAIT: &str = "window.closed.wait@1";
// 定义协作式 UIX Agent 精确 generation 关闭等待 capability。
pub(crate) const WINDOW_CLOSED_WAIT_V2: &str = "window.closed.wait@2";
// 定义协作式精确窗口修订与呈现等待 capability。
pub(crate) const WINDOW_REVISION_WAIT: &str = "window.revision.wait@1";
// 定义进程发现 capability 的稳定 ID。
pub(crate) const PROCESS_DISCOVER: &str = "process.discover@1";
// 定义进程元数据读取 capability 的稳定 ID。
pub(crate) const PROCESS_METADATA_READ: &str = "process.metadata.read@1";
// 定义通用进程优雅终止 capability 的稳定 ID。
pub(crate) const PROCESS_TERMINATE_GRACEFUL: &str = "process.terminate.graceful@1";
// 定义 Linux 精确进程代际的 pidfd 温和终止 capability，禁止扩写 Windows v1 语义。
pub(crate) const PROCESS_TERMINATE_GRACEFUL_V2: &str = "process.terminate.graceful@2";
// 定义通用进程强制终止 capability 的稳定 ID。
pub(crate) const PROCESS_TERMINATE_FORCE: &str = "process.terminate.force@1";
// 定义 Linux procfs owner epoch + pidfd 绑定的显式强制终止 capability。
pub(crate) const PROCESS_TERMINATE_FORCE_V2: &str = "process.terminate.force@2";
// 定义窗口发现 capability 的稳定 ID。
pub(crate) const WINDOW_DISCOVER: &str = "window.discover@1";
// 定义 Linux partial accessibility exporter 窗口发现候选 capability。
pub(crate) const WINDOW_DISCOVER_V2: &str = "window.discover@2";
// 定义显式启用 Agent 的 UIX 应用窗口发现 capability。
pub(crate) const WINDOW_DISCOVER_V3: &str = "window.discover@3";
// 定义窗口元数据读取 capability 的稳定 ID。
pub(crate) const WINDOW_METADATA_READ: &str = "window.metadata.read@1";
// 定义 Linux AT-SPI 窗口元数据只读候选 capability。
pub(crate) const WINDOW_METADATA_READ_V2: &str = "window.metadata.read@2";
// 定义 UIX Agent 精确窗口元数据只读 capability。
pub(crate) const WINDOW_METADATA_READ_V3: &str = "window.metadata.read@3";
// 定义 UIX Agent 协商式窗口框架当前状态读取 capability。
pub(crate) const WINDOW_STATE_READ: &str = "window.state.read@1";
// 定义 UIX Agent 同连接有界窗口框架状态等待 capability。
pub(crate) const WINDOW_STATE_WAIT: &str = "window.state.wait@1";
// 定义精确窗口零帧捕获预检 capability 的稳定 ID。
pub(crate) const WINDOW_CAPTURE_PREFLIGHT: &str = "window.capture.preflight@1";
// 定义精确窗口首帧元数据探针 capability 的稳定 ID。
pub(crate) const WINDOW_CAPTURE_FRAME_PROBE: &str = "window.capture.frame.probe@1";
// 定义冻结 Windows v1 之外的 Linux MPRIS 候选能力组。
pub(crate) const MEDIA_SESSION_DISCOVER_V2: &str = "media.session.discover@2";
pub(crate) const MEDIA_PLAYBACK_STATE_READ_V2: &str = "media.playback.state.read@2";
pub(crate) const MEDIA_PLAYBACK_CONTROL_V2: &str = "media.playback.control@2";
// 定义生产 App facade 的窗口无关发现、状态与带观察验证的媒体控制。
pub(crate) const MEDIA_SESSION_DISCOVER_V3: &str = "media.session.discover@3";
pub(crate) const MEDIA_PLAYBACK_STATE_READ_V3: &str = "media.playback.state.read@3";
pub(crate) const MEDIA_PLAYBACK_CONTROL_V3: &str = "media.playback.control@3";
