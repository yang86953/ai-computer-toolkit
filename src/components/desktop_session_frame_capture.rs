//! 桌面会话单帧输入、像素归一化与 PNG 编码 Component。

use std::{io::Cursor, path::Path, time::Duration};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use crate::{
    components::byte_digest,
    domain::{AppControlError, AppResult},
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const MINIMUM_TIMEOUT: Duration = Duration::from_secs(1);
const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(30);
const MAXIMUM_PATH_BYTES: usize = 32_767;
const MAXIMUM_DIMENSION: u32 = 16_384;
const MAXIMUM_PIXELS: u64 = 67_108_864;
const MAXIMUM_RAW_BYTES: usize = 256 * 1024 * 1024;
const MAXIMUM_PNG_BYTES: usize = 64 * 1024 * 1024;

/// 严格解析后的单帧输出请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopFrameCaptureInput {
    path: String,
    timeout: Duration,
    overwrite: bool,
    max_dimension: Option<u32>,
}

impl DesktopFrameCaptureInput {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn overwrite(&self) -> bool {
        self.overwrite
    }

    pub(crate) const fn max_dimension(&self) -> Option<u32> {
        self.max_dimension
    }
}

/// Adapter 可交给 Component 的受支持 packed 四字节像素格式。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopPackedPixelFormat {
    Rgba,
    Bgra,
    Rgbx,
    Bgrx,
    Xrgb,
    Xbgr,
}

/// 已完成上限校验的单帧 PNG 候选。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopCapturedFrame {
    png: Vec<u8>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    pixel_digest: String,
    source_width: u32,
    source_height: u32,
}

impl DesktopCapturedFrame {
    pub(crate) fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }

    pub(crate) fn png(&self) -> &[u8] {
        &self.png
    }

    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn pixel_digest(&self) -> &str {
        &self.pixel_digest
    }

    pub(crate) fn pixels(&self) -> u64 {
        u64::from(self.source_width) * u64::from(self.source_height)
    }

    pub(crate) const fn source_size(&self) -> (u32, u32) {
        (self.source_width, self.source_height)
    }
}

/// 只描述受限像素管线失败，不携带原生地址、FD 或节点身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopFrameDataError {
    InvalidFrame,
    ResourceExhausted,
    EncodingFailed,
}

/// 在任何路径解析或文件触碰前由 Module 完成确认后调用。
pub(crate) fn parse_input(value: &Value) -> AppResult<DesktopFrameCaptureInput> {
    let object = value.as_object().ok_or_else(|| {
        AppControlError::new(
            "INVALID_ARGUMENT",
            "Screen capture input must be an object.",
        )
    })?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "path" | "timeoutMs" | "overwrite" | "maxDimension"))
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Screen capture accepts path, timeoutMs, overwrite and maxDimension only.",
        ));
    }
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty() && path.len() <= MAXIMUM_PATH_BYTES)
        .ok_or_else(|| {
            AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "Screen capture requires a bounded UTF-8 .png path.",
            )
        })?;
    if Path::new(path).extension().and_then(|value| value.to_str()) != Some("png") {
        return Err(AppControlError::new(
            "INVALID_OUTPUT_PATH",
            "Screen capture output must use the .png extension.",
        ));
    }
    let timeout = match object.get("timeoutMs") {
        None => DEFAULT_TIMEOUT,
        Some(value) => value.as_u64().map(Duration::from_millis).ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "Screen capture timeoutMs must be an integer.",
            )
        })?,
    };
    if !(MINIMUM_TIMEOUT..=MAXIMUM_TIMEOUT).contains(&timeout) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Screen capture timeoutMs must be 1000..30000ms.",
        ));
    }
    let overwrite = match object.get("overwrite") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "Screen capture overwrite must be a boolean.",
            )
        })?,
    };
    let max_dimension = match object.get("maxDimension") {
        None => None,
        Some(value) => Some(value.as_u64()
            .filter(|value| (256..=u64::from(MAXIMUM_DIMENSION)).contains(value))
            .ok_or_else(|| AppControlError::new(
                "INVALID_ARGUMENT", "Screen capture maxDimension must be an integer in 256..16384.",
            ))? as u32),
    };
    Ok(DesktopFrameCaptureInput {
        path: path.to_owned(),
        timeout,
        overwrite,
        max_dimension,
    })
}

/// 在映射边界内直接归一化原帧或最近邻预览；预览只分配输出尺寸，默认保留全部像素。
pub(crate) fn encode_mapped_frame(
    allocation: &[u8],
    offset: u32,
    chunk_size: u32,
    stride: i32,
    size: (u32, u32),
    format: DesktopPackedPixelFormat,
    max_dimension: Option<u32>,
) -> Result<DesktopCapturedFrame, DesktopFrameDataError> {
    let (source_width, source_height) = size;
    let pixels = u64::from(source_width) * u64::from(source_height);
    if source_width == 0 || source_height == 0
        || source_width > MAXIMUM_DIMENSION || source_height > MAXIMUM_DIMENSION
        || pixels > MAXIMUM_PIXELS || allocation.len() > MAXIMUM_RAW_BYTES
    {
        return Err(DesktopFrameDataError::ResourceExhausted);
    }
    if max_dimension.is_some_and(|value| !(256..=MAXIMUM_DIMENSION).contains(&value)) {
        return Err(DesktopFrameDataError::InvalidFrame);
    }
    let row_bytes = source_width as usize * 4;
    let absolute_stride = stride.unsigned_abs() as usize;
    if stride == 0 || absolute_stride < row_bytes {
        return Err(DesktopFrameDataError::InvalidFrame);
    }
    let row_span = absolute_stride.checked_mul(source_height as usize - 1)
        .ok_or(DesktopFrameDataError::ResourceExhausted)?;
    let required_span = row_span.checked_add(row_bytes)
        .ok_or(DesktopFrameDataError::ResourceExhausted)?;
    if required_span > MAXIMUM_RAW_BYTES || (chunk_size as usize) < required_span {
        return Err(DesktopFrameDataError::InvalidFrame);
    }
    let offset = offset as usize;
    // 即使预览没有采样某一行，也必须验证整个源帧范围，不能掩盖损坏或截断的 mapping。
    let last_row = if stride > 0 { offset.checked_add(row_span) } else { offset.checked_sub(row_span) }
        .ok_or(DesktopFrameDataError::InvalidFrame)?;
    let end = offset.max(last_row).checked_add(row_bytes)
        .ok_or(DesktopFrameDataError::InvalidFrame)?;
    if end > allocation.len() {
        return Err(DesktopFrameDataError::InvalidFrame);
    }
    let largest = source_width.max(source_height);
    let limit = max_dimension.unwrap_or(largest).min(largest);
    let width = ((u64::from(source_width) * u64::from(limit) / u64::from(largest)) as u32).max(1);
    let height = ((u64::from(source_height) * u64::from(limit) / u64::from(largest)) as u32).max(1);
    let output_row_bytes = width as usize * 4;
    let mut rgba = vec![0_u8; output_row_bytes * height as usize];
    for (row, destination) in rgba.chunks_exact_mut(output_row_bytes).enumerate() {
        let source_y = ((2 * row + 1) as u64 * u64::from(source_height) / (2 * u64::from(height))) as usize;
        let row_delta = source_y * absolute_stride;
        let source_start = if stride > 0 { offset + row_delta } else { offset - row_delta };
        let source = &allocation[source_start..source_start + row_bytes];
        if width == source_width {
            normalize_row(source, destination, format)?;
        } else {
            for (column, pixel) in destination.chunks_exact_mut(4).enumerate() {
                let source_x = ((2 * column + 1) as u64 * u64::from(source_width) / (2 * u64::from(width))) as usize;
                normalize_row(&source[source_x * 4..source_x * 4 + 4], pixel, format)?;
            }
        }
    }
    let pixel_digest = byte_digest::digest(&rgba);
    let mut png = Cursor::new(Vec::new());
    PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|_| DesktopFrameDataError::EncodingFailed)?;
    let png = png.into_inner();
    if png.is_empty() || png.len() > MAXIMUM_PNG_BYTES {
        return Err(DesktopFrameDataError::ResourceExhausted);
    }
    Ok(DesktopCapturedFrame { png, rgba, width, height, pixel_digest, source_width, source_height })
}

pub(crate) fn normalize_row(
    source: &[u8],
    destination: &mut [u8],
    format: DesktopPackedPixelFormat,
) -> Result<(), DesktopFrameDataError> {
    if source.len() != destination.len() || !source.len().is_multiple_of(4) {
        return Err(DesktopFrameDataError::InvalidFrame);
    }
    for (source, destination) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
        let rgba = match format {
            DesktopPackedPixelFormat::Rgba => [source[0], source[1], source[2], source[3]],
            DesktopPackedPixelFormat::Bgra => [source[2], source[1], source[0], source[3]],
            DesktopPackedPixelFormat::Rgbx => [source[0], source[1], source[2], 255],
            DesktopPackedPixelFormat::Bgrx => [source[2], source[1], source[0], 255],
            DesktopPackedPixelFormat::Xrgb => [source[1], source[2], source[3], 255],
            DesktopPackedPixelFormat::Xbgr => [source[3], source[2], source[1], 255],
        };
        destination.copy_from_slice(&rgba);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use image::GenericImageView;
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_bounded_png_request() {
        let input = parse_input(&json!({
            "path": "/tmp/frame.png",
            "timeoutMs": 1234,
            "overwrite": true,
        }))
        .unwrap_or_else(|error| panic!("解析请求失败：{error:?}"));
        assert_eq!(input.path(), "/tmp/frame.png");
        assert_eq!(input.timeout(), Duration::from_millis(1234));
        assert!(input.overwrite());
    }

    #[test]
    fn rejects_unregistered_capture_input_fields() {
        let Err(error) = parse_input(&json!({
            "path": "/tmp/frame.png",
            "nativeNodeId": 42,
        })) else {
            panic!("原生 PipeWire 字段必须在像素访问前被拒绝");
        };
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    #[test]
    fn normalizes_bgra_stride_and_encodes_png() {
        let bytes = [3, 2, 1, 4, 7, 6, 5, 8, 0, 0, 0, 0];
        let frame = encode_mapped_frame(&bytes, 0, 12, 12, (2, 1), DesktopPackedPixelFormat::Bgra, None)
            .unwrap_or_else(|error| panic!("编码单帧失败：{error:?}"));
        assert_eq!((frame.width(), frame.height()), (2, 1));
        let decoded = image::load_from_memory(frame.png())
            .unwrap_or_else(|error| panic!("读取 PNG 失败：{error}"));
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.to_rgba8().as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            frame.pixel_digest(),
            byte_digest::digest(&[1, 2, 3, 4, 5, 6, 7, 8])
        );
    }

    #[test]
    fn follows_negative_stride_without_reading_outside_the_mapping() {
        let bytes = [30, 20, 10, 255, 60, 50, 40, 255];
        let frame = encode_mapped_frame(&bytes, 4, 8, -4, (1, 2), DesktopPackedPixelFormat::Bgra, None)
            .unwrap_or_else(|error| panic!("编码负 stride 单帧失败：{error:?}"));
        let decoded = image::load_from_memory(frame.png())
            .unwrap_or_else(|error| panic!("读取负 stride PNG 失败：{error}"));
        assert_eq!(
            decoded.to_rgba8().as_raw(),
            &[40, 50, 60, 255, 10, 20, 30, 255]
        );
    }
}
