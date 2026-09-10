//! 固定本机 named-pipe endpoint 身份与帧预算 Component。

// 固定独立交互会话 pipe 名称前缀，native session 后缀永不进入公共响应。
const INTERACTIVE_SESSION_PIPE_NAME_PREFIX: &str =
    r"\\.\pipe\ai-computer-toolkit-interactive-session-v1-";
// 固定长操作 broker pipe 名称前缀，避免与独立交互会话争抢实例。
const LONG_OPERATION_PIPE_NAME_PREFIX: &str = r"\\.\pipe\ai-computer-toolkit-long-operation-v1-";
// 固定浏览器会话 broker pipe 名称前缀，native session 只作为私有后缀。
const BROWSER_SESSION_PIPE_NAME_PREFIX: &str = r"\\.\pipe\ai-computer-toolkit-browser-session-v1-";
// 限制独立交互会话单条 frame 为 256 KiB。
pub(crate) const MAXIMUM_FRAME_BYTES: usize = 256 * 1024;
// 为长操作 1 MiB 结果与固定 envelope 预留 128 KiB 边界。
pub(crate) const LONG_OPERATION_MAXIMUM_FRAME_BYTES: usize = 1_152 * 1024;
// 冻结浏览器会话 client 到 server 的 64 KiB 输入边界。
pub(crate) const BROWSER_SESSION_MAXIMUM_REQUEST_FRAME_BYTES: usize = 64 * 1024;
// 冻结浏览器会话 server 到 client 的 16 MiB 加 128 KiB 响应边界。
pub(crate) const BROWSER_SESSION_MAXIMUM_RESPONSE_FRAME_BYTES: usize =
    16 * 1024 * 1024 + 128 * 1024;

// 表示固定本机 message pipe 可服务的封闭 endpoint 类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixedLocalEndpointKind {
    // 表示显式授权的独立交互会话 broker。
    InteractiveSession,
    // 表示当前登录会话的长操作 broker。
    // #2020 接入 broker 前保留已经冻结的固定 endpoint 类别。
    #[allow(dead_code)]
    LongOperation,
    // 表示当前登录会话的浏览器会话 broker。
    BrowserSession,
}

// 为 endpoint 类别提供不可由请求覆盖的固定前缀。
impl FixedLocalEndpointKind {
    // 返回当前类别编译期固定的本机 pipe 前缀。
    pub(super) const fn pipe_name_prefix(self) -> &'static str {
        // 封闭映射所有 endpoint 类别。
        match self {
            // 独立交互会话保留既有名称。
            Self::InteractiveSession => INTERACTIVE_SESSION_PIPE_NAME_PREFIX,
            // 长操作使用独立固定名称。
            Self::LongOperation => LONG_OPERATION_PIPE_NAME_PREFIX,
            // 浏览器会话使用独立固定名称。
            Self::BrowserSession => BROWSER_SESSION_PIPE_NAME_PREFIX,
        }
    }

    // 返回 client-to-server 与 server-to-client 的冻结帧预算。
    pub(super) const fn frame_budgets(self) -> (usize, usize) {
        // 按封闭 endpoint 类别选择预算。
        match self {
            // 交互命令维持既有 256 KiB 契约。
            Self::InteractiveSession => (MAXIMUM_FRAME_BYTES, MAXIMUM_FRAME_BYTES),
            // 长操作必须能投递完整 1 MiB 结果与状态 envelope。
            Self::LongOperation => (
                LONG_OPERATION_MAXIMUM_FRAME_BYTES,
                LONG_OPERATION_MAXIMUM_FRAME_BYTES,
            ),
            // 浏览器会话严格区分小请求与大截图响应。
            Self::BrowserSession => (
                BROWSER_SESSION_MAXIMUM_REQUEST_FRAME_BYTES,
                BROWSER_SESSION_MAXIMUM_RESPONSE_FRAME_BYTES,
            ),
        }
    }
}
