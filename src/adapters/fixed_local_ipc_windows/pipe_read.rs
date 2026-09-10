//! 固定 named-pipe 单帧 UTF-8 文本读取 Component。

// 导入短轮询与单调 deadline。
use std::{
    // 在非阻塞空管道时执行有界退避。
    thread,
    // 使用短睡眠与绝对单调 deadline。
    time::{Duration, Instant},
};

// 导入 Windows message pipe 读取与错误分类。
use windows::Win32::{
    // 区分同一 message 未读完与非阻塞空管道。
    Foundation::{ERROR_MORE_DATA, ERROR_NO_DATA},
    // 读取当前 message 的一个固定块。
    Storage::FileSystem::ReadFile,
    // 只为旧同步 endpoint 保留非消费式可读门禁。
    System::Pipes::PeekNamedPipe,
};

// 导入统一结果边界。
use crate::domain::AppResult;

// 导入父 pipe Component 的连接与安全 helper。
use super::{
    // 借用已连接 pipe 的私有 I/O 事实与预算。
    ConnectedPipe,
    // 复用固定小块读取 message。
    READ_CHUNK_BYTES,
    // 构造稳定取消错误。
    cancelled_error,
    // 构造不泄漏 native 状态的 endpoint 错误。
    endpoint_error,
    // 校验 browser owner 生命周期。
    require_browser_authority_live,
    // 取得不越过绝对 deadline 的短轮询片。
    wait_slice_ms,
};

// 为已连接 pipe 提供唯一有界 message 读取边界。
impl ConnectedPipe {
    // 读取一条完整且有界的 UTF-8 JSON message。
    pub(crate) fn read_text_until(
        // 接收覆盖整条消息的单调 deadline。
        &self,
        // 接收固定绝对 deadline。
        deadline: Instant,
        // 接收调用方取消轮询函数。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<String> {
        // owner 关闭后 secondary 不得再读取或解释业务 JSON。
        require_browser_authority_live(self.browser_authority.as_ref())?;
        // 保存累计 message 字节。
        let mut output = Vec::new();
        // 使用固定小块处理 ERROR_MORE_DATA。
        let mut chunk = [0_u8; READ_CHUNK_BYTES];
        // 持续读取到一条 message 完成。
        loop {
            // browser 的 PIPE_NOWAIT handle 必须在 ReadFile 前直接核对取消与 deadline。
            let nonblocking_slice = if self.bounded_write_nonblocking {
                // 取消优先阻止下一次原生读取。
                if cancelled() {
                    // 返回统一取消语义。
                    return Err(cancelled_error());
                }
                // 每次 ReadFile 前取得不越过总 deadline 的退避片。
                Some(wait_slice_ms(deadline)?)
            } else {
                // Interactive 与 LongOperation 保持既有 Peek 后同步 ReadFile 行为。
                self.wait_for_blocking_message(deadline, &cancelled)?;
                // 阻塞 handle 不需要空管道退避片。
                None
            };
            // 初始化本次读取长度。
            let mut read = 0_u32;
            // 直接读取 browser 非阻塞 handle，避免 PeekNamedPipe 自身阻塞。
            let result = unsafe {
                // 调用同步 ReadFile；PIPE_NOWAIT 保证空管道立即返回。
                ReadFile(
                    // 使用已连接 pipe。
                    self.handle.raw(),
                    // 传入固定块。
                    Some(&mut chunk),
                    // 接收实际长度。
                    Some(&mut read),
                    // 使用同步 I/O。
                    None,
                )
            };
            // 转换本次长度。
            let read = usize::try_from(read).map_err(|_| {
                // 不公开异常平台长度。
                endpoint_error("The interactive endpoint read length overflowed.")
            })?;
            // 平台不得报告超出缓冲区的长度。
            if read > chunk.len() {
                // 返回稳定协议错误。
                return Err(endpoint_error(
                    // 不公开原生长度。
                    "The interactive endpoint returned an invalid read boundary.",
                ));
            }
            // 非阻塞空管道允许平台以成功零字节或 ERROR_NO_DATA 零字节表示暂时无数据。
            if self.bounded_write_nonblocking
                // 零字节保证该轮没有消费任何 message 数据。
                && read == 0
                // 同时兼容 Win32 在不同 pipe 端报告的两种无数据结果。
                && (result.is_ok()
                    // 只放行微软定义的空管道错误。
                    || matches!(&result, Err(error) if error.code() == ERROR_NO_DATA.to_hresult()))
            {
                // 创建期非阻塞事实保证该轮没有消费任何 message 字节。
                let slice = nonblocking_slice.ok_or_else(|| {
                    // 理论模式漂移返回安全 endpoint 错误。
                    endpoint_error("The interactive endpoint read mode was invalid.")
                })?;
                // 仅按已经由绝对 deadline 截断的短片退避。
                thread::sleep(Duration::from_millis(u64::from(slice)));
                // 下一轮重新观察取消与 deadline 后再直接 ReadFile。
                continue;
            }
            // 检查累计长度不会越过硬上限。
            if output.len().saturating_add(read) > self.maximum_read_frame_bytes {
                // 返回结构化资源边界错误。
                return Err(endpoint_error(
                    // 不回显累计长度。
                    "The interactive endpoint message exceeded its safety boundary.",
                ));
            }
            // 追加本次有效字节。
            output.extend_from_slice(&chunk[..read]);
            // 完整 message 结束循环。
            match result {
                // 正常完成一条消息。
                Ok(()) => break,
                // 缓冲区不足时继续读取同一条消息。
                Err(error) if error.code() == ERROR_MORE_DATA.to_hresult() => continue,
                // 其他 I/O 失败保持 endpoint 不可用。
                Err(_) => {
                    // 返回不泄漏 pipe 状态的错误。
                    return Err(endpoint_error(
                        // 不公开 Win32 错误。
                        "The interactive endpoint message could not be read.",
                    ));
                }
            }
        }
        // 空 message 不属于 JSON 协议。
        if output.is_empty() {
            // 返回稳定协议错误。
            return Err(endpoint_error(
                // 不把空 message 当作回压。
                "The interactive endpoint message was empty.",
            ));
        }
        // 严格解码 UTF-8，拒绝宽松替换字符。
        String::from_utf8(output).map_err(|_| {
            // 不回显无效字节。
            endpoint_error("The interactive endpoint message was not valid UTF-8.")
        })
    }

    // 为 Interactive 与 LongOperation 等待当前消息出现一个可读字节。
    fn wait_for_blocking_message(
        // 借用当前已连接 pipe。
        &self,
        // 接收固定绝对 deadline。
        deadline: Instant,
        // 借用取消轮询函数。
        cancelled: &impl Fn() -> bool,
    ) -> AppResult<()> {
        // 轮询只读可用字节，不消费协议数据。
        loop {
            // 取消优先关闭当前连接并停止等待。
            if cancelled() {
                // 返回统一取消错误。
                return Err(cancelled_error());
            }
            // 先核对 deadline 并取得下一轮休眠片。
            let slice = wait_slice_ms(deadline)?;
            // 接收 pipe 中当前全部可读字节数。
            let mut available = 0_u32;
            // 旧同步 endpoint 继续通过 Peek 避免 ReadFile 无限等待。
            unsafe {
                // 调用不会消费数据的可读查询。
                PeekNamedPipe(
                    // 使用当前已连接 handle。
                    self.handle.raw(),
                    // 不复制消息内容。
                    None,
                    // 无输出缓冲区。
                    0,
                    // 不需要本次复制长度。
                    None,
                    // 只读取总可用字节。
                    Some(&mut available),
                    // 不公开消息剩余长度。
                    None,
                )
            }
            // 对端断开或 pipe 异常必须立即结束等待。
            .map_err(|_| endpoint_error("The interactive endpoint message could not be read."))?;
            // 一旦存在数据即可执行旧同步 ReadFile。
            if available > 0 {
                // 返回可读状态。
                return Ok(());
            }
            // 空 pipe 使用短片休眠并在下一轮重新核对取消与 deadline。
            thread::sleep(Duration::from_millis(u64::from(slice)));
        }
    }
}
