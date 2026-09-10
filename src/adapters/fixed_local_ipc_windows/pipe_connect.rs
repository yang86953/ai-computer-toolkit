//! 连接固定 named-pipe 并在建立期一次性安装写入等待模式。

// 导入单调 deadline。
use std::time::Instant;

// 导入固定 client 建连所需的 Windows API。
use windows::{
    // 导入状态、权限与最后错误读取。
    Win32::{
        // 导入连接失败分类与读写权限。
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE,
            GetLastError,
        },
        // 导入同步 client handle 创建属性。
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING},
        // 导入 message wait-mode 与 listener 等待。
        System::Pipes::{
            NAMED_PIPE_MODE, PIPE_NOWAIT, PIPE_READMODE_MESSAGE, PIPE_WAIT,
            SetNamedPipeHandleState, WaitNamedPipeW,
        },
    },
    // 导入 NUL 结尾 pipe 名称指针。
    core::PCWSTR,
};

// 导入统一结果边界。
use crate::domain::AppResult;

// 导入父 Component 私有连接依赖。
use super::{
    // 导入已连接 pipe 与其 private 字段类型。
    ConnectedPipe,
    // 导入固定 endpoint 类别与测试预算。
    FixedLocalEndpointKind,
    OwnedPipeHandle,
    PipeRole,
    // 导入取消和安全错误投影。
    cancelled_error,
    endpoint_error,
    // 导入固定名称和有界等待 helper。
    pipe_name,
    wait_slice_ms,
};

// 测试命名入口复用交互 endpoint 的默认帧预算。
#[cfg(test)]
use super::MAXIMUM_FRAME_BYTES;

// 为已连接 pipe 提供受控 client 创建。
impl ConnectedPipe {
    // 连接精确 session 的固定 broker pipe。
    pub(crate) fn connect_until(
        // 接收私有目标 Windows session。
        session_id: u32,
        // 接收覆盖连接阶段的单调 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 保持既有独立交互会话入口语义。
        Self::connect_for_until(
            // 固定选择独立交互会话 endpoint。
            FixedLocalEndpointKind::InteractiveSession,
            // 传递目标 native session。
            session_id,
            // 传递单调 deadline。
            deadline,
            // 传递取消观察函数。
            cancelled,
        )
    }

    // 在 deadline 内连接指定固定类别的同 session broker。
    pub(crate) fn connect_for_until(
        // 接收封闭 endpoint 类别而非调用方名称。
        kind: FixedLocalEndpointKind,
        // 接收私有目标 Windows session。
        session_id: u32,
        // 接收覆盖连接阶段的单调 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 构造私有固定名称。
        let name = pipe_name(kind, session_id)?;
        // 取得双向冻结预算。
        let (request_frame_bytes, response_frame_bytes) = kind.frame_budgets();
        // 只有 browser client 在建立期固定安装 PIPE_NOWAIT。
        let bounded_write_nonblocking = matches!(kind, FixedLocalEndpointKind::BrowserSession);
        // 按固定预算和连接期写入模式创建 client。
        Self::connect_named_with_budgets_and_write_mode_until(
            // 传递固定名称。
            &name,
            // client 读取 server response。
            response_frame_bytes,
            // client 写入 server request。
            request_frame_bytes,
            // 传递 browser 专属写入模式事实。
            bounded_write_nonblocking,
            // 传递单调 deadline。
            deadline,
            // 传递取消观察函数。
            cancelled,
        )
    }

    // 在 deadline 内连接已经校验来源的 NUL 结尾 pipe 名称。
    #[cfg(test)]
    pub(crate) fn connect_named_until(
        // 借用仅供当前 Component 使用的 endpoint 名称。
        name: &[u16],
        // 接收覆盖连接阶段的单调 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 测试命名入口保持独立交互会话预算。
        Self::connect_named_with_budgets_until(
            // 传递唯一测试名称。
            name,
            // 使用测试默认读取预算。
            MAXIMUM_FRAME_BYTES,
            // 使用测试默认写入预算。
            MAXIMUM_FRAME_BYTES,
            // 传递测试 deadline。
            deadline,
            // 传递取消观察。
            cancelled,
        )
    }

    // 在 deadline 内按固定双向预算连接已经校验来源的 pipe 名称。
    #[cfg(test)]
    pub(super) fn connect_named_with_budgets_until(
        // 借用仅供当前 Component 使用的 endpoint 名称。
        name: &[u16],
        // 接收当前方向的读取预算。
        maximum_read_frame_bytes: usize,
        // 接收当前方向的写入预算。
        maximum_write_frame_bytes: usize,
        // 接收覆盖连接阶段的单调 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 普通与旧测试连接保持 PIPE_WAIT。
        Self::connect_named_with_budgets_and_write_mode_until(
            // 传递固定名称。
            name,
            // 传递读取预算。
            maximum_read_frame_bytes,
            // 传递写入预算。
            maximum_write_frame_bytes,
            // 不在连接期安装 PIPE_NOWAIT。
            false,
            // 传递总 deadline。
            deadline,
            // 传递取消观察。
            cancelled,
        )
    }

    // 在 deadline 内连接名称，并在创建期固定选择连接写入模式。
    fn connect_named_with_budgets_and_write_mode_until(
        // 借用仅供当前 Component 使用的 endpoint 名称。
        name: &[u16],
        // 接收当前方向的读取预算。
        maximum_read_frame_bytes: usize,
        // 接收当前方向的写入预算。
        maximum_write_frame_bytes: usize,
        // 指定连接期是否固定安装 PIPE_NOWAIT。
        bounded_write_nonblocking: bool,
        // 接收覆盖连接阶段的单调 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 拒绝空名称或缺少 Windows 终止符的测试输入。
        if name.len() < 2 || name.last().copied() != Some(0) {
            // 保持名称细节不进入公共错误。
            return Err(endpoint_error(
                "The fixed interactive endpoint name was invalid.",
            ));
        }
        // 在 pipe 忙碌时按短片等待，pipe 不存在时立即失败。
        loop {
            // 连接前取消不会触碰 endpoint。
            if cancelled() {
                // 返回确定性取消。
                return Err(cancelled_error());
            }
            // 取得不越过调用方 deadline 的等待片。
            let slice = wait_slice_ms(deadline)?;
            // 等待一个固定 pipe instance 可连接。
            if unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), slice) }.as_bool() {
                // endpoint 当前可连接。
                break;
            }
            // 读取当前线程的稳定 Win32 失败码。
            let error = unsafe { GetLastError() };
            // 未部署 endpoint 应立即返回。
            if error == ERROR_FILE_NOT_FOUND {
                // 返回结构化不可用。
                return Err(endpoint_error(
                    "No certified interactive endpoint is installed in the target session.",
                ));
            }
            // 只有 pipe 忙或本轮等待到期可以继续。
            if error != ERROR_PIPE_BUSY && error != ERROR_SEM_TIMEOUT {
                // 其他状态不进行协议猜测。
                return Err(endpoint_error(
                    "No certified interactive endpoint accepted the connection deadline.",
                ));
            }
        }
        // 只打开已经由固定 endpoint 发布的同步双向 pipe。
        let handle = unsafe {
            // 调用 Windows fixed-pipe client open。
            CreateFileW(
                // 使用私有固定名称。
                PCWSTR(name.as_ptr()),
                // 只申请双向协议所需读写权限。
                GENERIC_READ.0 | GENERIC_WRITE.0,
                // pipe 不需要文件共享模式。
                FILE_SHARE_MODE::default(),
                // 不提供可继承安全属性。
                None,
                // 只打开已存在 broker endpoint。
                OPEN_EXISTING,
                // 使用普通同步 handle。
                FILE_ATTRIBUTE_NORMAL,
                // 不提供模板 handle。
                None,
            )
        }
        // 打开失败保持结构化不可用。
        .map_err(|_| endpoint_error("The interactive endpoint connection could not be opened."))?;
        // 接管 client pipe handle。
        let handle = OwnedPipeHandle::new(handle)?;
        // browser 固定使用 PIPE_NOWAIT，其余连接保持 PIPE_WAIT。
        let wait_mode = if bounded_write_nonblocking {
            // 让 browser client 的有界写入无需运行中切换模式。
            PIPE_NOWAIT
        } else {
            // 保持既有连接的阻塞 write 语义。
            PIPE_WAIT
        };
        // 在连接建立期一次性安装消息读取与写入等待模式。
        let mode = NAMED_PIPE_MODE(PIPE_READMODE_MESSAGE.0 | wait_mode.0);
        // 安装读取模式。
        unsafe { SetNamedPipeHandleState(handle.raw(), Some(&raw const mode), None, None) }
            // 模式不可认证时关闭连接。
            .map_err(|_| endpoint_error("The interactive endpoint message mode is unavailable."))?;
        // 返回已连接 client。
        Ok(Self {
            // 保存拥有的 handle。
            handle,
            // client 查询 server PID。
            role: PipeRole::Client,
            // 保存响应读取预算。
            maximum_read_frame_bytes,
            // 保存请求写入预算。
            maximum_write_frame_bytes,
            // 保存创建期已经安装的有界写入模式事实。
            bounded_write_nonblocking,
            // client 不拥有 server 首实例 authority。
            browser_authority: None,
        })
    }

    // 为回压测试创建已经固定为 PIPE_NOWAIT 的受控 client 连接。
    #[cfg(test)]
    pub(crate) fn connect_named_with_budgets_and_nonblocking_write_until(
        // 借用唯一测试名称。
        name: &[u16],
        // 接收测试读取预算。
        maximum_read_frame_bytes: usize,
        // 接收测试写入预算。
        maximum_write_frame_bytes: usize,
        // 接收覆盖连接的单调 deadline。
        deadline: Instant,
        // 接收测试取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<Self> {
        // 测试显式选择同 browser 的固定非阻塞写入事实。
        Self::connect_named_with_budgets_and_write_mode_until(
            // 传递唯一测试名称。
            name,
            // 传递测试读取预算。
            maximum_read_frame_bytes,
            // 传递测试写入预算。
            maximum_write_frame_bytes,
            // 固定安装 PIPE_NOWAIT。
            true,
            // 传递连接 deadline。
            deadline,
            // 传递取消观察。
            cancelled,
        )
    }
}
