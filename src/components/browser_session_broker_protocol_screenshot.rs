//! 验证 Browser Session Broker 的 request-bound PNG 成功投影。

// 导入标准 Base64 解码引擎。
use base64::Engine;
// 导入 JSON 值。
use serde_json::Value;

// 导入稳定字节摘要 Component。
use crate::components::byte_digest;
// 导入公开页面 identity 的同源验证器。
use crate::components::browser_session_broker_protocol::canonical_page_id;

// 验证 schema 与原 request 限定的 screenshot 成功数据。
pub(super) fn screenshot_data(value: &Value, expected_page_id: &str) -> bool {
    // screenshot data 必须是对象。
    let Some(object) = value.as_object() else {
        // 拒绝其他 JSON 类型。
        return false;
    };
    // 字段集合必须与 schema 完全一致。
    if object.len() != 8
        // 每个字段必须属于冻结白名单。
        || !object.keys().all(|key| {
            // 穷举 request-bound PNG 字段。
            [
                "pageId",
                "navigationGeneration",
                "mimeType",
                "pngBase64",
                "pngBytes",
                "width",
                "height",
                "digest",
            ]
            // 检查当前键。
            .contains(&key.as_str())
        })
        // MIME 类型固定为 PNG。
        || object.get("mimeType").and_then(Value::as_str) != Some("image/png")
    {
        // 拒绝字段漂移或伪造 MIME。
        return false;
    }
    // 页面必须逐字绑定原 request。
    if object.get("pageId").and_then(Value::as_str) != Some(expected_page_id)
        // 页面还必须保持 canonical 公开形状。
        || !canonical_page_id(expected_page_id)
    {
        // 拒绝跨页面成功投影。
        return false;
    }
    // 导航代际必须为正 u32。
    if !object
        // 读取代际。
        .get("navigationGeneration")
        // 转换为无符号整数。
        .and_then(Value::as_u64)
        // 核对公开范围。
        .is_some_and(|generation| (1..=u64::from(u32::MAX)).contains(&generation))
    {
        // 拒绝缺失或越界代际。
        return false;
    }
    // 读取有界 Base64 文本。
    let Some(encoded) = object.get("pngBase64").and_then(Value::as_str) else {
        // Base64 必须存在。
        return false;
    };
    // Base64 wire 文本必须非空且遵守 schema 上限。
    if encoded.is_empty() || encoded.len() > 16 * 1024 * 1024 {
        // 拒绝资源超限。
        return false;
    }
    // 解码标准 Base64。
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        // 拒绝非标准 Base64。
        return false;
    };
    // 解码后 PNG 必须非空且不超过 Module 的 12 MiB 边界。
    if bytes.is_empty() || bytes.len() > 12 * 1024 * 1024 {
        // 拒绝资源超限。
        return false;
    }
    // 公开字节数必须匹配真实解码长度。
    if object.get("pngBytes").and_then(Value::as_u64) != u64::try_from(bytes.len()).ok() {
        // 拒绝伪造字节数。
        return false;
    }
    // 读取并验证 PNG signature、IHDR 与尺寸。
    let Some((width, height)) = png_dimensions(&bytes) else {
        // 拒绝非 PNG 数据。
        return false;
    };
    // response 尺寸必须逐字段匹配 IHDR。
    if object.get("width").and_then(Value::as_u64) != Some(u64::from(width))
        // 高度也必须匹配 IHDR。
        || object.get("height").and_then(Value::as_u64) != Some(u64::from(height))
    {
        // 拒绝伪造尺寸。
        return false;
    }
    // 计算解码后 PNG 的稳定字节摘要。
    let digest = byte_digest::digest(&bytes);
    // digest 必须逐字匹配解码后 PNG 的稳定摘要。
    object.get("digest").and_then(Value::as_str) == Some(digest.as_str())
}

// 读取并验证 PNG signature、IHDR 与公开尺寸边界。
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // 固定 PNG signature。
    const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    // PNG 至少必须容纳 signature、IHDR 长度、类型和宽高。
    if bytes.len() < 24
        // 校验 PNG signature。
        || bytes.get(0..8) != Some(PNG_SIGNATURE.as_slice())
        // 首块长度必须是 IHDR 的 13 字节。
        || bytes.get(8..12) != Some(13_u32.to_be_bytes().as_slice())
        // 首块类型必须是 IHDR。
        || bytes.get(12..16) != Some(b"IHDR".as_slice())
    {
        // 拒绝非 PNG 或缺失 IHDR 的字节。
        return None;
    }
    // 读取大端宽度。
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    // 读取大端高度。
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    // 两个公开尺寸都必须落在契约边界内。
    ((1..=10_000).contains(&width) && (1..=10_000).contains(&height))
        // 返回经过边界验证的尺寸。
        .then_some((width, height))
}
