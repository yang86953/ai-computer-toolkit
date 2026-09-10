//! 负责私有 CDP WebSocket 帧的严格编解码。

// 导入 TCP 帧读写能力。
use std::{
    // 导入同步读写。
    io::{Read, Write},
    // 导入固定 TCP 流。
    net::TcpStream,
};

// 导入安全随机 nonce、统一结果与父传输窄辅助函数。
use super::{
    // 导入控制帧上限。
    MAXIMUM_CONTROL_BYTES,
    // 导入消息上限。
    MAXIMUM_MESSAGE_BYTES,
    // 导入统一 CDP 错误构造器。
    cdp_error,
    // 导入随机 nonce 解码器。
    decode_nonce,
    // 导入断开错误构造器。
    disconnected,
    // 导入帧错误构造器。
    protocol_frame_error,
};
// 导入安全随机源。
use crate::components::secure_nonce_windows::random_nonce;
// 导入统一结果。
use crate::domain::AppResult;

// 写入一个 masked client frame。
pub(super) fn write_frame(
    // 借用唯一 TCP 流。
    stream: &mut TcpStream,
    // 接收封闭 opcode。
    opcode: u8,
    // 借用有界 payload。
    payload: &[u8],
) -> AppResult<()> {
    // 只允许 text 或 close 控制帧。
    if !matches!(opcode, 0x1 | 0x8)
        // 控制帧 payload 必须有界。
        || (opcode == 0x8 && payload.len() > MAXIMUM_CONTROL_BYTES)
        // 数据帧必须有界。
        || payload.len() > MAXIMUM_MESSAGE_BYTES
    {
        // 拒绝非法内部调用。
        return Err(cdp_error(
            // 使用协议类别。
            "BROWSER_PROTOCOL_FAILED",
            // 输出固定诊断。
            "The browser protocol client frame was invalid.",
        ));
    }
    // 生成随机四字节 mask 来源。
    let nonce = random_nonce()?;
    // 解码 16 随机字节。
    let random = decode_nonce(&nonce)?;
    // 取前四字节作为 mask。
    let mask = [random[0], random[1], random[2], random[3]];
    // 构造 frame header。
    let mut frame = Vec::with_capacity(payload.len().saturating_add(14));
    // FIN=1 且使用固定 opcode。
    frame.push(0x80 | opcode);
    // 按长度选择 canonical 编码，并设置 MASK 位。
    match payload.len() {
        // 小 payload 使用七位长度。
        length @ 0..=125 => frame.push(0x80 | u8::try_from(length).unwrap_or(125)),
        // 中等 payload 使用 16 位长度。
        length @ 126..=65_535 => {
            // 写入 16 位标记。
            frame.push(0x80 | 126);
            // 写入大端长度。
            frame.extend_from_slice(&u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes());
        }
        // 大 payload 使用 64 位长度。
        length => {
            // 写入 64 位标记。
            frame.push(0x80 | 127);
            // 写入大端长度。
            frame.extend_from_slice(&u64::try_from(length).unwrap_or(u64::MAX).to_be_bytes());
        }
    }
    // 写入 mask key。
    frame.extend_from_slice(&mask);
    // 写入逐字节 mask 后 payload。
    frame.extend(
        payload
            // 遍历 payload。
            .iter()
            // 结合索引应用循环 mask。
            .enumerate()
            // 生成 masked 字节。
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    // 写入完整 frame。
    stream.write_all(&frame).map_err(|_| {
        // 映射断开。
        cdp_error(
            // 使用断开类别。
            "BROWSER_PROTOCOL_DISCONNECTED",
            // 输出安全诊断。
            "The browser protocol connection closed while sending a command.",
        )
    })
}

// 读取一条完整 server text message，并处理 ping/close。
pub(super) fn read_message(
    // 借用唯一 TCP 流。
    stream: &mut TcpStream,
) -> AppResult<Vec<u8>> {
    // 保存可能分片的消息。
    let mut message = Vec::new();
    // 保存是否已经开始 text 消息。
    let mut started = false;
    // 持续读取 frame 直至 FIN。
    loop {
        // 读取基本 header。
        let mut header = [0_u8; 2];
        // 读取两个固定字节。
        stream.read_exact(&mut header).map_err(|_| disconnected())?;
        // 提取 FIN。
        let fin = header[0] & 0x80 != 0;
        // RSV 位必须全零。
        if header[0] & 0x70 != 0 {
            // 拒绝未协商扩展。
            return Err(protocol_frame_error());
        }
        // 提取 opcode。
        let opcode = header[0] & 0x0f;
        // server frame 不得 masked。
        if header[1] & 0x80 != 0 {
            // 拒绝 masked server frame。
            return Err(protocol_frame_error());
        }
        // 读取 canonical payload 长度。
        let length = read_payload_length(stream, header[1] & 0x7f)?;
        // 控制帧必须 FIN 且不超过 125。
        if opcode >= 0x8 && (!fin || length > MAXIMUM_CONTROL_BYTES as u64) {
            // 拒绝非法控制帧。
            return Err(protocol_frame_error());
        }
        // 所有 payload 都必须位于硬上限。
        let length = usize::try_from(length)
            // 映射平台长度溢出。
            .ok()
            // 限制最大消息。
            .filter(|length| message.len().saturating_add(*length) <= MAXIMUM_MESSAGE_BYTES)
            // 超限失败。
            .ok_or_else(|| {
                // 返回资源错误。
                cdp_error(
                    // 使用输出上限码。
                    "WORKER_OUTPUT_TOO_LARGE",
                    // 输出固定诊断。
                    "The browser protocol message exceeded its boundary.",
                )
            })?;
        // 读取 payload。
        let mut payload = vec![0_u8; length];
        // 读取完整 payload。
        stream
            .read_exact(&mut payload)
            .map_err(|_| disconnected())?;
        // 按 opcode 处理。
        match opcode {
            // text 开始帧。
            0x1 if !started => {
                // 标记消息开始。
                started = true;
                // 追加 payload。
                message.extend_from_slice(&payload);
            }
            // continuation 只能跟随消息开始。
            0x0 if started => {
                // 追加分片。
                message.extend_from_slice(&payload);
            }
            // ping 立即回复 masked pong。
            0x9 => {
                // 写入 pong 控制帧。
                write_control_frame(stream, 0xA, &payload)?;
                // 继续读取当前消息。
                continue;
            }
            // pong 可安全忽略。
            0xA => continue,
            // close 表示结构化断开。
            0x8 => return Err(disconnected()),
            // binary、重复 text 或无起点 continuation 全部拒绝。
            _ => return Err(protocol_frame_error()),
        }
        // 完整 text 消息返回。
        if fin && started {
            // 返回累计字节。
            return Ok(message);
        }
    }
}

// 写入 masked client 控制帧。
fn write_control_frame(
    // 借用唯一 TCP 流。
    stream: &mut TcpStream,
    // 接收固定 pong opcode。
    opcode: u8,
    // 借用有界 payload。
    payload: &[u8],
) -> AppResult<()> {
    // pong 使用与 client frame 相同的 mask 逻辑。
    if opcode != 0xA || payload.len() > MAXIMUM_CONTROL_BYTES {
        // 拒绝非法内部控制帧。
        return Err(protocol_frame_error());
    }
    // 生成随机 mask。
    let random = decode_nonce(&random_nonce()?)?;
    // 取前四字节。
    let mask = [random[0], random[1], random[2], random[3]];
    // 构造固定小 frame。
    let mut frame = Vec::with_capacity(payload.len().saturating_add(6));
    // FIN 和 pong opcode。
    frame.push(0x80 | opcode);
    // MASK 和七位长度。
    frame.push(0x80 | u8::try_from(payload.len()).unwrap_or(125));
    // 写入 mask。
    frame.extend_from_slice(&mask);
    // 写入 masked payload。
    frame.extend(
        payload
            // 遍历 payload。
            .iter()
            // 结合索引。
            .enumerate()
            // 应用循环 mask。
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    // 写入完整控制帧。
    stream.write_all(&frame).map_err(|_| disconnected())
}

// 读取 WebSocket payload 长度并强制 canonical 编码。
fn read_payload_length(
    // 借用唯一 TCP 流。
    stream: &mut TcpStream,
    // 接收七位长度标记。
    short: u8,
) -> AppResult<u64> {
    // 按长度标记解析。
    match short {
        // 七位长度直接返回。
        0..=125 => Ok(u64::from(short)),
        // 读取 16 位长度。
        126 => {
            // 保存两字节。
            let mut bytes = [0_u8; 2];
            // 读取完整长度。
            stream.read_exact(&mut bytes).map_err(|_| disconnected())?;
            // 转换大端值。
            let length = u64::from(u16::from_be_bytes(bytes));
            // 非 canonical 小值拒绝。
            if length < 126 {
                // 返回 frame 错误。
                return Err(protocol_frame_error());
            }
            // 返回长度。
            Ok(length)
        }
        // 读取 64 位长度。
        127 => {
            // 保存八字节。
            let mut bytes = [0_u8; 8];
            // 读取完整长度。
            stream.read_exact(&mut bytes).map_err(|_| disconnected())?;
            // 最高位必须为零。
            if bytes[0] & 0x80 != 0 {
                // 拒绝负语义长度。
                return Err(protocol_frame_error());
            }
            // 转换大端值。
            let length = u64::from_be_bytes(bytes);
            // 非 canonical 中等值拒绝。
            if length <= u64::from(u16::MAX) {
                // 返回 frame 错误。
                return Err(protocol_frame_error());
            }
            // 返回长度。
            Ok(length)
        }
        // 七位值不存在其他分支。
        _ => Err(protocol_frame_error()),
    }
}
