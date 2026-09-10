//! 固定 named-pipe 单帧 JSON 与预编码文本写入 Component。

// 导入短轮询与单调 deadline。
use std::{
    // 在写缓冲区暂时不足时执行有界退避。
    thread,
    // 使用单调绝对 deadline 和毫秒轮询片。
    time::{Duration, Instant},
};

// 导入 JSON 序列化 trait。
use serde::Serialize;
// 导入 Windows 同步单帧写入。
use windows::Win32::{
    // 只对当前已连接 handle 写入一条 message。
    Storage::FileSystem::WriteFile,
};

// 导入统一错误与结果边界。
use crate::domain::{AppControlError, AppResult};

// 导入父 pipe Component 的连接与安全 helper。
use super::{
    // 借用已连接 pipe 与稳定错误 helper。
    ConnectedPipe,
    cancelled_error,
    endpoint_error,
    require_browser_authority_live,
    // 复用不超过单调 deadline 的短轮询片。
    wait_slice_ms,
};

// 表示 browser client 单次非阻塞全帧写入的封闭结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BoundedTextWriteAttempt {
    // 表示完整 message 已经由当前 WriteFile 原子写入。
    Written,
    // 表示零字节写入且可证明仅为暂时回压。
    BackpressureZero,
    // 表示平台或短写无法排除 broker 已观察到请求。
    DeliveryUncertain,
}

// 保存有界文本写入的投递分类与安全错误。
#[derive(Debug)]
pub(crate) struct BoundedTextWriteFailure {
    // 保存不泄漏 pipe 状态的统一错误。
    error: AppControlError,
    // 标记是否已经完整写入，或平台错误无法排除投递。
    delivery_may_have_occurred: bool,
}

// 为调用方提供不丢失 D/R 语义的窄投影。
impl BoundedTextWriteFailure {
    // 返回是否必须以同 nonce 恢复或投影未知结果。
    pub(crate) const fn delivery_may_have_occurred(&self) -> bool {
        // 复制封闭投递事实。
        self.delivery_may_have_occurred
    }

    // 消费失败并返回统一安全错误。
    pub(crate) fn into_error(self) -> AppControlError {
        // 转移错误所有权。
        self.error
    }

    // 测试与窄边界可读取稳定错误码。
    #[cfg(test)]
    pub(crate) fn error_code(&self) -> &'static str {
        // 只借用静态错误码。
        self.error.code
    }
}

// 构造尚未完整写入任何字节的失败。
fn not_dispatched(error: AppControlError) -> BoundedTextWriteFailure {
    // 返回允许 client 保留本地未派发事实的分类。
    BoundedTextWriteFailure {
        // 保存统一错误。
        error,
        // message 非阻塞空写不会部分投递。
        delivery_may_have_occurred: false,
    }
}

// 构造已完整写入或无法排除投递的失败。
fn delivery_uncertain(error: AppControlError) -> BoundedTextWriteFailure {
    // 返回禁止新 nonce 自动重试的分类。
    BoundedTextWriteFailure {
        // 保存统一错误。
        error,
        // 调用方必须以原 nonce 恢复或保守未知。
        delivery_may_have_occurred: true,
    }
}

// 为已连接 pipe 提供唯一单帧写入边界。
impl ConnectedPipe {
    // 写入一条完整且有界的 JSON message。
    pub(crate) fn write_json(&self, value: &impl Serialize) -> AppResult<()> {
        // 序列化固定协议对象。
        let text = serde_json::to_string(value).map_err(|_| {
            // 不回显协议对象。
            endpoint_error("The interactive endpoint response could not be serialized.")
        })?;
        // 复用预编码文本的同一帧边界。
        self.write_text(&text)
    }

    // 写入一条已经由严格 codec 生成的完整 UTF-8 JSON message。
    pub(crate) fn write_text(&self, text: &str) -> AppResult<()> {
        // owner 关闭后 secondary 不得再发送业务或 control JSON。
        require_browser_authority_live(self.browser_authority.as_ref())?;
        // 借用 Rust 已保证有效 UTF-8 的文本字节。
        let bytes = text.as_bytes();
        // 拒绝空值与超出硬上限的 frame。
        if bytes.is_empty() || bytes.len() > self.maximum_write_frame_bytes {
            // 返回结构化资源边界错误。
            return Err(endpoint_error(
                // 不回显输入长度或内容。
                "The interactive endpoint message exceeded its safety boundary.",
            ));
        }
        // 保存 Windows 写入长度。
        let mut written = 0_u32;
        // message-mode pipe 必须以单次 WriteFile 保留一帧边界。
        unsafe {
            // 调用同步 Windows 写入。
            WriteFile(
                // 使用已连接 pipe。
                self.handle.raw(),
                // 写入完整 JSON 字节。
                Some(bytes),
                // 接收实际写入长度。
                Some(&mut written),
                // 使用同步 I/O。
                None,
            )
        }
        // 写入失败保持 endpoint 不可用。
        .map_err(|_| endpoint_error("The interactive endpoint message could not be written."))?;
        // 短写会破坏 message framing，必须失败闭合。
        if usize::try_from(written).ok() != Some(bytes.len()) {
            // 返回稳定协议失败。
            return Err(endpoint_error(
                // 不公开平台短写长度。
                "The interactive endpoint message was not written completely.",
            ));
        }
        // 单帧不超过创建时的方向预算。
        Ok(())
    }

    // 尝试一次已在连接建立期固定为非阻塞的 browser message 全帧写入。
    pub(crate) fn try_write_text_once(
        // 借用当前已连接 pipe。
        &self,
        // 借用已经由严格 codec 生成的 UTF-8 文本。
        text: &str,
        // 接收不会阻塞的取消观察。
        cancelled: impl Fn() -> bool,
    ) -> Result<BoundedTextWriteAttempt, BoundedTextWriteFailure> {
        // owner 关闭后 secondary 不得开始新写入。
        require_browser_authority_live(self.browser_authority.as_ref()).map_err(not_dispatched)?;
        // 借用 Rust 已保证有效 UTF-8 的文本字节。
        let bytes = text.as_bytes();
        // 拒绝空帧与超出当前方向硬上限的帧。
        if bytes.is_empty() || bytes.len() > self.maximum_write_frame_bytes {
            // 返回不回显长度或内容的稳定错误。
            return Err(not_dispatched(endpoint_error(
                // 保持与同步写入相同的资源边界。
                "The interactive endpoint message exceeded its safety boundary.",
            )));
        }
        // 写入模式必须在连接建立期固定，运行中禁止切换。
        if !self.bounded_write_nonblocking {
            // 非 browser 或未声明的连接不能假设 WriteFile 可立即返回。
            return Err(not_dispatched(endpoint_error(
                // 不公开连接类型或 native wait 状态。
                "The interactive endpoint does not support bounded text writes.",
            )));
        }
        // 每次平台写入前重新核对 owner 关闭事实。
        require_browser_authority_live(self.browser_authority.as_ref())
            // 在任何全帧成功前 owner 关闭仍可证明未派发。
            .map_err(not_dispatched)?;
        // 取消后不得再进入 WriteFile。
        if cancelled() {
            // 返回统一取消语义。
            return Err(not_dispatched(cancelled_error()));
        }
        // 保存 Windows 写入长度。
        let mut written = 0_u32;
        // 在 message 非阻塞模式下仅尝试一次全帧写入。
        let write_result = unsafe {
            // 调用同步 API，但 PIPE_NOWAIT 保证缓冲不足时立即返回。
            WriteFile(
                // 使用已连接 pipe handle。
                self.handle.raw(),
                // 尝试写入完整 message 字节。
                Some(bytes),
                // 接收平台返回的写入长度。
                Some(&mut written),
                // 不使用 overlapped 结构。
                None,
            )
        };
        // 平台错误无法从 API 回值证明零投递。
        if write_result.is_err() {
            // 把不确定投递事实交给 adapter 以同 nonce 恢复。
            return Ok(BoundedTextWriteAttempt::DeliveryUncertain);
        }
        // 完整长度证明一条 message 已原子写入。
        if usize::try_from(written).ok() == Some(bytes.len()) {
            // 返回可信完整写入事实。
            return Ok(BoundedTextWriteAttempt::Written);
        }
        // message 非阻塞模式的零字节写入仅表示缓冲暂时不足。
        if written == 0 {
            // 返回可在新剩余预算下重建 frame 的回压事实。
            return Ok(BoundedTextWriteAttempt::BackpressureZero);
        }
        // 任何部分 message 都会破坏 framing，必须失败闭合。
        Ok(BoundedTextWriteAttempt::DeliveryUncertain)
    }

    // 在单调 deadline 内以旧有轮询语义写入一条完整文本帧。
    pub(crate) fn write_text_until(
        // 借用当前已连接 pipe。
        &self,
        // 借用已经由严格 codec 生成的 UTF-8 文本。
        text: &str,
        // 接收覆盖整个写入的单调绝对 deadline。
        deadline: Instant,
        // 接收不会阻塞的取消观察。
        cancelled: impl Fn() -> bool,
    ) -> Result<(), BoundedTextWriteFailure> {
        // 在当前兼容入口中保持既有的完整写入或 deadline 结果。
        loop {
            // 取消优先阻止当前轮进入任何平台写入。
            if cancelled() {
                // 返回可证明未派发的统一取消错误。
                return Err(not_dispatched(cancelled_error()));
            }
            // 每次 WriteFile 前先核对绝对 deadline 并取得回压等待片。
            let slice_ms = wait_slice_ms(deadline).map_err(not_dispatched)?;
            // 单次尝试不在此处休眠，避免改变 browser adapter 的重建时机。
            match self.try_write_text_once(text, &cancelled)? {
                // 已完成全帧写入时结束。
                BoundedTextWriteAttempt::Written => return Ok(()),
                // 平台无法排除投递时保持既有保守失败投影。
                BoundedTextWriteAttempt::DeliveryUncertain => {
                    // 不公开 native 写入状态。
                    return Err(delivery_uncertain(endpoint_error(
                        // 保持同步写入一致的安全错误说明。
                        "The interactive endpoint message could not be written completely.",
                    )));
                }
                // 零字节回压仍由兼容入口按 caller deadline 有界轮询。
                BoundedTextWriteAttempt::BackpressureZero => {
                    // 只等待已被 deadline 截断的短片。
                    thread::sleep(Duration::from_millis(u64::from(slice_ms)));
                }
            }
        }
    }
}
