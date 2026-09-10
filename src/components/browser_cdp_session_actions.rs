//! 执行固定浏览器元素动作与截图。

// 导入单调期限。
use std::time::Instant;

// 导入 Base64 解码器。
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入父会话与窄组件。
use super::{BrowserCdpSession, session_error};
// 导入固定 CDP 方法、页面边界与字节摘要。
use crate::{
    // 导入私有组件。
    components::{
        // 导入封闭方法。
        browser_cdp_transport::CdpMethod,
        // 导入冻结输出边界。
        browser_page_protocol::{MAXIMUM_PNG_BYTES, MAXIMUM_TYPE_TEXT_BYTES},
        // 导入稳定摘要。
        byte_digest,
    },
    // 导入统一结果。
    domain::AppResult,
};

// 固定 PNG signature。
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

// 为私有 CDP 会话提供元素写动作与截图。
impl BrowserCdpSession {
    // 在 dispatch 前验证元素引用并返回私有 backend node ID。
    pub(crate) fn resolve_element(&self, element_ref: &str) -> AppResult<u64> {
        // 查找当前导航代际 registry。
        self.elements.get(element_ref).copied().ok_or_else(|| {
            // 未找到表示 stale。
            session_error(
                // 使用稳定元素 stale 码。
                "STALE_ELEMENT",
                // 不回显私有引用。
                "The browser element reference is stale.",
            )
        })
    }

    // 点击一个已在当前代际解析的元素。
    pub(crate) fn click(&mut self, backend_node_id: u64, deadline: Instant) -> AppResult<Value> {
        // 读取元素盒模型。
        let result = self.call(
            // 使用封闭盒模型方法。
            CdpMethod::DomGetBoxModel,
            // 只传入 registry 中的私有 node ID。
            json!({ "backendNodeId": backend_node_id }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 提取内容四边形中心点。
        let (x, y) = content_center(&result)?;
        // 派发固定左键按下。
        self.call(
            // 使用封闭鼠标方法。
            CdpMethod::InputDispatchMouseEvent,
            // 固定 left/clickCount 和坐标。
            json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 派发固定左键释放。
        self.call(
            // 使用同一封闭方法。
            CdpMethod::InputDispatchMouseEvent,
            // 固定释放事实。
            json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 返回 provider-neutral 完成数据。
        Ok(json!({ "kind": "click", "clicked": true }))
    }

    // 向当前代际元素输入文本。
    pub(crate) fn type_text(
        &mut self,
        // 接收私有 backend node ID。
        backend_node_id: u64,
        // 借用已验证 UTF-8 文本。
        text: &str,
        // 接收替换语义。
        replace: bool,
        // 接收绝对期限。
        deadline: Instant,
    ) -> AppResult<Value> {
        // 双重执行资源门禁。
        if text.is_empty() || text.len() > MAXIMUM_TYPE_TEXT_BYTES {
            // 返回确定参数失败。
            return Err(session_error(
                // 使用稳定参数码。
                "INVALID_ARGUMENT",
                // 不回显文本。
                "The browser text input exceeded its resource boundary.",
            ));
        }
        // 聚焦 registry 中的元素。
        self.call(
            // 使用封闭聚焦方法。
            CdpMethod::DomFocus,
            // 只传入私有 node ID。
            json!({ "backendNodeId": backend_node_id }),
            // 复用唯一总期限。
            deadline,
        )?;
        // replace 使用固定全选与删除按键序列。
        if replace {
            // 派发 Control 按下。
            self.dispatch_key("keyDown", "Control", "ControlLeft", 17, 2, deadline)?;
            // 派发 A 按下并携带 Control modifier。
            self.dispatch_key("keyDown", "a", "KeyA", 65, 2, deadline)?;
            // 派发 A 释放。
            self.dispatch_key("keyUp", "a", "KeyA", 65, 2, deadline)?;
            // 派发 Control 释放。
            self.dispatch_key("keyUp", "Control", "ControlLeft", 17, 0, deadline)?;
            // 派发 Backspace 按下。
            self.dispatch_key("keyDown", "Backspace", "Backspace", 8, 0, deadline)?;
            // 派发 Backspace 释放。
            self.dispatch_key("keyUp", "Backspace", "Backspace", 8, 0, deadline)?;
        }
        // 通过固定输入方法插入 UTF-8 文本。
        self.call(
            // 使用封闭文本方法。
            CdpMethod::InputInsertText,
            // 只传入协议已验证文本。
            json!({ "text": text }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 返回 provider-neutral 输入事实。
        Ok(json!({
            // 标记 type 数据。
            "kind": "type",
            // 声明输入完成。
            "typed": true,
            // 返回 UTF-8 字节计数。
            "utf8Bytes": text.len(),
        }))
    }

    // 捕获当前页面的有界 PNG。
    pub(crate) fn screenshot(&mut self, deadline: Instant) -> AppResult<Value> {
        // 请求固定 PNG 格式且不接受调用方质量或路径。
        let result = self.call(
            // 使用封闭截图方法。
            CdpMethod::PageCaptureScreenshot,
            // 固定 fromSurface 和 PNG。
            json!({ "format": "png", "fromSurface": true }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 读取标准 Base64 数据。
        let encoded = result.get("data").and_then(Value::as_str).ok_or_else(|| {
            // 缺失数据是协议失败。
            session_error(
                // 使用稳定截图码。
                "SCREENSHOT_FAILED",
                // 输出安全诊断。
                "The browser did not return a PNG screenshot.",
            )
        })?;
        // 编码文本必须有界。
        if encoded.is_empty()
            // Base64 上界按原始字节计算。
            || encoded.len()
                > (usize::try_from(MAXIMUM_PNG_BYTES).unwrap_or(usize::MAX).saturating_add(2) / 3)
                    .saturating_mul(4)
        {
            // 拒绝资源超限。
            return Err(screenshot_error());
        }
        // 严格解码标准 Base64。
        let bytes = BASE64_STANDARD
            .decode(encoded)
            .map_err(|_| screenshot_error())?;
        // 原始 PNG 字节必须非空有界。
        if bytes.is_empty() || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_PNG_BYTES {
            // 拒绝资源超限。
            return Err(screenshot_error());
        }
        // 读取并验证 PNG IHDR。
        let (width, height) = png_dimensions(&bytes)?;
        // 返回 provider-neutral 截图数据。
        Ok(json!({
            // 标记截图种类。
            "kind": "screenshot",
            // MIME 固定 PNG。
            "mimeType": "image/png",
            // 保留经过严格解码验证的 Base64。
            "pngBase64": encoded,
            // 返回原始字节数。
            "pngBytes": bytes.len(),
            // 返回 IHDR 宽度。
            "width": width,
            // 返回 IHDR 高度。
            "height": height,
            // 返回稳定完整容器摘要。
            "digest": byte_digest::digest(&bytes),
        }))
    }

    // 派发一条固定键盘事件。
    fn dispatch_key(
        &mut self,
        // 借用固定事件类型。
        event_type: &str,
        // 借用固定 key。
        key: &str,
        // 借用固定 code。
        code: &str,
        // 接收 Windows virtual key code。
        windows_virtual_key_code: u16,
        // 接收 CDP modifier 位图。
        modifiers: u8,
        // 接收绝对期限。
        deadline: Instant,
    ) -> AppResult<()> {
        // 派发封闭键盘方法。
        self.call(
            // 使用固定键盘事件方法。
            CdpMethod::InputDispatchKeyEvent,
            // 参数全部由 worker 固定序列构造。
            json!({
                // 写入事件类型。
                "type": event_type,
                // 写入 key。
                "key": key,
                // 写入 code。
                "code": code,
                // 写入 Windows key code。
                "windowsVirtualKeyCode": windows_virtual_key_code,
                // 写入 modifier 位图。
                "modifiers": modifiers,
            }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 返回成功。
        Ok(())
    }
}

// 从 DOM.getBoxModel 读取内容四边形中心。
fn content_center(result: &Value) -> AppResult<(f64, f64)> {
    // 读取八项 content quad。
    let coordinates = result
        // 定位固定结果字段。
        .pointer("/model/content")
        // 转换为数组。
        .and_then(Value::as_array)
        // 必须恰有四个点。
        .filter(|values| values.len() == 8)
        // 否则返回确定失败。
        .ok_or_else(|| {
            session_error(
                // 使用元素不可交互码。
                "ELEMENT_NOT_INTERACTABLE",
                // 输出安全诊断。
                "The browser element does not have an interactable box.",
            )
        })?;
    // 求四个 x 坐标平均值。
    let x = coordinates
        // 取偶数位置。
        .iter()
        // 每两个取一个 x。
        .step_by(2)
        // 转换为有限浮点。
        .map(finite_number)
        // 求和或失败。
        .collect::<AppResult<Vec<_>>>()?
        // 消费坐标。
        .into_iter()
        // 求总和。
        .sum::<f64>()
        // 求平均。
        / 4.0;
    // 求四个 y 坐标平均值。
    let y = coordinates
        // 跳过首个 x。
        .iter()
        // 从第一个 y 开始。
        .skip(1)
        // 每两个取一个 y。
        .step_by(2)
        // 转换为有限浮点。
        .map(finite_number)
        // 求和或失败。
        .collect::<AppResult<Vec<_>>>()?
        // 消费坐标。
        .into_iter()
        // 求总和。
        .sum::<f64>()
        // 求平均。
        / 4.0;
    // 中心必须有限。
    if !x.is_finite() || !y.is_finite() {
        // 返回不可交互。
        return Err(session_error(
            // 使用稳定元素码。
            "ELEMENT_NOT_INTERACTABLE",
            // 输出安全诊断。
            "The browser element does not have an interactable box.",
        ));
    }
    // 返回确定中心。
    Ok((x, y))
}

// 读取一个有限 JSON 数字。
fn finite_number(value: &Value) -> AppResult<f64> {
    // 转换并核对有限性。
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| {
            // 返回元素不可交互。
            session_error(
                // 使用稳定元素码。
                "ELEMENT_NOT_INTERACTABLE",
                // 输出安全诊断。
                "The browser element does not have an interactable box.",
            )
        })
}

// 读取并验证 PNG signature、IHDR 与尺寸。
fn png_dimensions(bytes: &[u8]) -> AppResult<(u32, u32)> {
    // PNG 必须至少包含完整 IHDR 头。
    if bytes.len() < 24
        // 核对固定 signature。
        || bytes.get(0..8) != Some(PNG_SIGNATURE.as_slice())
        // 首块长度必须为十三。
        || bytes.get(8..12) != Some(13_u32.to_be_bytes().as_slice())
        // 首块类型必须为 IHDR。
        || bytes.get(12..16) != Some(b"IHDR".as_slice())
    {
        // 拒绝伪造或截断 PNG。
        return Err(screenshot_error());
    }
    // 读取大端宽度。
    let width = u32::from_be_bytes(bytes[16..20].try_into().map_err(|_| screenshot_error())?);
    // 读取大端高度。
    let height = u32::from_be_bytes(bytes[20..24].try_into().map_err(|_| screenshot_error())?);
    // 尺寸必须非零且有界。
    if !(1..=10_000).contains(&width) || !(1..=10_000).contains(&height) {
        // 拒绝异常尺寸。
        return Err(screenshot_error());
    }
    // 返回尺寸。
    Ok((width, height))
}

// 构造稳定截图失败。
fn screenshot_error() -> crate::domain::AppControlError {
    // 不回显 Base64 或 CDP 响应。
    session_error(
        // 使用稳定截图码。
        "SCREENSHOT_FAILED",
        // 输出安全诊断。
        "The browser screenshot was not a valid bounded PNG.",
    )
}
