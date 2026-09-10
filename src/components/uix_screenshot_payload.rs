//! UIX Agent 截图 base64 与 PNG 信封校验 Component。

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::components::byte_digest;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAXIMUM_DIMENSION: u32 = 16_384;
const MAXIMUM_PIXELS: u64 = 67_108_864;

/// 经上限、编码和 IHDR 交叉校验的 PNG。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UixScreenshotPng {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    digest: String,
}

impl UixScreenshotPng {
    pub(crate) fn decode(
        data_base64: &str,
        reported_width: u32,
        reported_height: u32,
        maximum_png_bytes: usize,
    ) -> Result<Self, UixScreenshotPayloadError> {
        let maximum_encoded = maximum_png_bytes
            .checked_add(2)
            .and_then(|value| value.checked_div(3))
            .and_then(|value| value.checked_mul(4))
            .ok_or(UixScreenshotPayloadError::ResourceExhausted)?;
        if data_base64.is_empty() || data_base64.len() > maximum_encoded {
            return Err(UixScreenshotPayloadError::ResourceExhausted);
        }
        let bytes = STANDARD
            .decode(data_base64)
            .map_err(|_| UixScreenshotPayloadError::InvalidPayload)?;
        if bytes.len() < 24 || bytes.len() > maximum_png_bytes {
            return Err(UixScreenshotPayloadError::ResourceExhausted);
        }
        if bytes.get(..8) != Some(PNG_SIGNATURE.as_slice())
            || bytes.get(8..12) != Some([0, 0, 0, 13].as_slice())
            || bytes.get(12..16) != Some(b"IHDR".as_slice())
        {
            return Err(UixScreenshotPayloadError::InvalidPayload);
        }
        let width = u32::from_be_bytes(
            bytes[16..20]
                .try_into()
                .map_err(|_| UixScreenshotPayloadError::InvalidPayload)?,
        );
        let height = u32::from_be_bytes(
            bytes[20..24]
                .try_into()
                .map_err(|_| UixScreenshotPayloadError::InvalidPayload)?,
        );
        let pixels = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > MAXIMUM_DIMENSION
            || height > MAXIMUM_DIMENSION
            || pixels > MAXIMUM_PIXELS
            || width != reported_width
            || height != reported_height
        {
            return Err(UixScreenshotPayloadError::InvalidPayload);
        }
        if !has_complete_png_chunk_layout(&bytes) {
            return Err(UixScreenshotPayloadError::InvalidPayload);
        }
        let digest = byte_digest::digest(&bytes);
        Ok(Self {
            bytes,
            width,
            height,
            digest,
        })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

fn has_complete_png_chunk_layout(bytes: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    let mut first = true;
    let mut saw_idat = false;
    loop {
        let Some(header_end) = offset.checked_add(8) else {
            return false;
        };
        if header_end > bytes.len() {
            return false;
        }
        let Ok(length_bytes) = bytes[offset..offset + 4].try_into() else {
            return false;
        };
        let length = u32::from_be_bytes(length_bytes) as usize;
        let chunk_type = &bytes[offset + 4..header_end];
        let Some(chunk_end) = header_end
            .checked_add(length)
            .and_then(|value| value.checked_add(4))
        else {
            return false;
        };
        if chunk_end > bytes.len() {
            return false;
        }
        if first && (chunk_type != b"IHDR" || length != 13) {
            return false;
        }
        if !first && chunk_type == b"IHDR" {
            return false;
        }
        first = false;
        if chunk_type == b"IDAT" {
            saw_idat = true;
        }
        if chunk_type == b"IEND" {
            return length == 0
                && saw_idat
                && chunk_end == bytes.len()
                && bytes.get(chunk_end - 4..chunk_end)
                    == Some([0xae, 0x42, 0x60, 0x82].as_slice());
        }
        offset = chunk_end;
    }
}

/// 不携带传输内容的封闭校验失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UixScreenshotPayloadError {
    InvalidPayload,
    ResourceExhausted,
}

#[cfg(test)]
mod tests {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    use super::*;

    fn fixture_png(width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::new();
        let Ok(pixel_bytes) = usize::try_from(u64::from(width) * u64::from(height) * 4) else {
            panic!("测试 PNG 像素数量必须可表示");
        };
        let pixels = vec![0_u8; pixel_bytes];
        assert!(
            PngEncoder::new(&mut png)
                .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
                .is_ok(),
            "测试 PNG 必须成功编码"
        );
        png
    }

    #[test]
    fn png_header_and_reported_dimensions_must_agree() {
        let png = fixture_png(2, 3);
        let encoded = STANDARD.encode(&png);
        let Ok(payload) = UixScreenshotPng::decode(&encoded, 2, 3, 1024) else {
            panic!("受限测试 PNG 必须通过校验");
        };
        assert_eq!(payload.width(), 2);
        assert_eq!(payload.height(), 3);
        assert!(UixScreenshotPng::decode(&encoded, 3, 2, 1024).is_err());
    }

    #[test]
    fn truncated_missing_idat_and_corrupt_iend_are_rejected() {
        let png = fixture_png(1, 1);

        let truncated = STANDARD.encode(&png[..png.len() - 12]);
        assert!(UixScreenshotPng::decode(&truncated, 1, 1, 1024).is_err());

        let mut missing_idat = png.clone();
        let Some(idat_offset) = missing_idat.windows(4).position(|value| value == b"IDAT") else {
            panic!("测试 PNG 必须包含 IDAT");
        };
        missing_idat[idat_offset..idat_offset + 4].copy_from_slice(b"tEXt");
        assert!(UixScreenshotPng::decode(&STANDARD.encode(missing_idat), 1, 1, 1024).is_err());

        let mut corrupt_iend = png;
        let Some(last) = corrupt_iend.last_mut() else {
            panic!("测试 PNG 不能为空");
        };
        *last ^= 1;
        assert!(UixScreenshotPng::decode(&STANDARD.encode(corrupt_iend), 1, 1, 1024).is_err());
    }
}
