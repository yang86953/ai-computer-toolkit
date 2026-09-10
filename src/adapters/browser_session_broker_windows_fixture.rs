//! 为固定协议集成测试封装认证 browser-session broker 原始帧连接。

// 导入单调 deadline 类型。
use std::time::Instant;

// 导入固定认证连接、server-first ready 与取消观察。
use crate::{
    // 复用生产 broker 的启动、连接与认证边界。
    adapters::browser_session_broker_windows::{
        // 建立固定 endpoint 的已认证连接。
        connect_certified,
        // 构造受冻结范围限制的握手 deadline。
        handshake_deadline,
        // 严格读取并解析 server-first ready。
        read_certified_ready,
    },
    // 保存而不泄漏原生 pipe 的私有连接类型。
    adapters::fixed_local_ipc_windows::pipe::ConnectedPipe,
    // 复用进程级取消而不开放测试专用绕过。
    components::cancellation,
    // 复用统一安全错误结果边界。
    domain::AppResult,
};

// 保存仅供同一 crate 固定协议夹具使用的认证原始帧连接。
pub(crate) struct CertifiedBrokerFixtureConnection {
    // 私有持有已认证的固定本机 pipe。
    pipe: ConnectedPipe,
    // 保存已经严格解析的当前 broker epoch。
    epoch: String,
}

// 为认证夹具连接提供不泄漏原生传输事实的窄操作。
impl CertifiedBrokerFixtureConnection {
    // 借用已认证且严格解析的当前 broker epoch。
    pub(crate) fn epoch(&self) -> &str {
        // 返回不含 pipe、PID、endpoint 或镜像路径的协议 epoch。
        &self.epoch
    }

    // 在调用方给定的单调 deadline 前写入一条严格 codec 已构造的原始帧。
    pub(crate) fn write_raw_until(&self, text: &str, deadline: Instant) -> AppResult<()> {
        // 委托真实固定 pipe 的有界写入并保留其安全错误投影。
        self.pipe
            // 写入唯一完整文本 frame。
            .write_text_until(
                // 只接受调用方已经通过严格 codec 构造的文本。
                text,
                // 不允许写入越过调用方的绝对 deadline。
                deadline,
                // 使用生产相同的进程级取消观察。
                cancellation::is_cancelled,
            )
            // 不向夹具调用方泄漏底层投递分类或原生错误。
            .map_err(|failure| failure.into_error())
    }

    // 在调用方给定的单调 deadline 前读取一条未经业务解码的原始帧。
    pub(crate) fn read_raw_until(&self, deadline: Instant) -> AppResult<String> {
        // 委托真实固定 pipe 的有界读取并保留其安全错误投影。
        self.pipe.read_text_until(
            // 不允许读取越过调用方的绝对 deadline。
            deadline,
            // 使用生产相同的进程级取消观察。
            cancellation::is_cancelled,
        )
    }
}

// 在调用方持有的绝对 deadline 前建立真实固定 pipe、完成双向认证并读取唯一 server-first ready。
pub(crate) fn connect_ready_until(
    deadline: Instant,
) -> AppResult<CertifiedBrokerFixtureConnection> {
    // 建立生产路径使用的固定 sibling 认证连接。
    let pipe = connect_certified(deadline)?;
    // 只在认证后读取并严格验证 server-first ready。
    let epoch = read_certified_ready(&pipe, deadline)?;
    // 返回不公开 ConnectedPipe 的窄夹具连接投影。
    Ok(CertifiedBrokerFixtureConnection { pipe, epoch })
}

// 建立真实固定 pipe、完成双向认证并读取唯一 server-first ready。
pub(crate) fn connect_ready(timeout_ms: u32) -> AppResult<CertifiedBrokerFixtureConnection> {
    // 构造覆盖连接、认证与 ready 的同一冻结 deadline。
    let deadline = handshake_deadline(timeout_ms)?;
    // 复用绝对 deadline 连接路径以保持既有 wrapper 契约。
    connect_ready_until(deadline)
}
