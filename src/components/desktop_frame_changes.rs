//! 有界 RGBA 差分；只报告视觉事实，不推断应用操作的完成状态。
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::{AppControlError, AppResult};

/// 每会话最多保留 32 MiB RGBA；8 个桌面会话合计最多 256 MiB 基准缓存。
pub(crate) const MAX_TRACKED_PIXELS: u64 = 8_388_608;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeOptions {
    pub baseline_frame_id: Option<String>,
    pub region: Option<Region>,
    #[serde(default)]
    pub pixel_threshold: u8,
    #[serde(default = "one")]
    pub min_changed_pixels: u64,
}

fn one() -> u64 {
    1
}
fn invalid(message: &str) -> AppControlError {
    AppControlError::new("INVALID_ARGUMENT", message)
}

impl ChangeOptions {
    pub(crate) fn parse(value: &Value) -> AppResult<Self> {
        let options: Self = serde_json::from_value(value.clone())
            .map_err(|_| invalid("Invalid changeDetection options."))?;
        if options.baseline_frame_id.as_ref().is_some_and(|id| {
            id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err(invalid(
                "baselineFrameId must be a 32-character lowercase hexadecimal frame identity.",
            ));
        }
        if !(1..=MAX_TRACKED_PIXELS).contains(&options.min_changed_pixels) {
            return Err(invalid("minChangedPixels is outside the tracking budget."));
        }
        if options.region.is_some_and(|r| {
            r.width == 0
                || r.height == 0
                || r.x > 16383
                || r.y > 16383
                || r.width > 16384
                || r.height > 16384
                || r.x.checked_add(r.width).is_none()
                || r.y.checked_add(r.height).is_none()
        }) {
            return Err(invalid("region must be nonempty and bounded."));
        }
        Ok(options)
    }

    pub(crate) fn checked_region(&self, width: u32, height: u32) -> AppResult<Region> {
        let region = self.region.unwrap_or(Region {
            x: 0,
            y: 0,
            width,
            height,
        });
        if width == 0
            || height == 0
            || region.width == 0
            || region.height == 0
            || region
                .x
                .checked_add(region.width)
                .is_none_or(|end| end > width)
            || region
                .y
                .checked_add(region.height)
                .is_none_or(|end| end > height)
        {
            return Err(invalid("region must fit the current observation-px image."));
        }
        Ok(region)
    }

    pub(crate) fn compare(
        &self,
        previous: &[u8],
        current: &[u8],
        width: u32,
        height: u32,
    ) -> AppResult<Value> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_TRACKED_PIXELS
            || previous.len() as u64 != pixels * 4
            || current.len() != previous.len()
        {
            return Err(invalid(
                "Invalid or oversized RGBA change-detection buffers.",
            ));
        }
        let region = self.checked_region(width, height)?;
        let mut count = 0u64;
        let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
        for y in region.y..region.y + region.height {
            let start = (y as usize * width as usize + region.x as usize) * 4;
            let end = start + region.width as usize * 4;
            for (index, (old, new)) in previous[start..end]
                .chunks_exact(4)
                .zip(current[start..end].chunks_exact(4))
                .enumerate()
            {
                if old
                    .iter()
                    .zip(new)
                    .any(|(a, b)| a.abs_diff(*b) > self.pixel_threshold)
                {
                    let x = region.x + index as u32;
                    count += 1;
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
            }
        }
        let bounds = (count > 0).then(|| Region {
            x: left,
            y: top,
            width: right - left + 1,
            height: bottom - top + 1,
        });
        Ok(
            json!({"status":"compared", "changed":count >= self.min_changed_pixels,
            "changedPixels":count, "comparedPixels":u64::from(region.width)*u64::from(region.height),
            "changedBounds":bounds, "region":region, "pixelThreshold":self.pixel_threshold,
            "minChangedPixels":self.min_changed_pixels}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_comparison_and_thresholds() {
        let old = vec![0; 4 * 3 * 2];
        let mut new = old.clone();
        let options = ChangeOptions::parse(&json!({})).unwrap();
        let same = options.compare(&old, &new, 3, 2).unwrap();
        assert_eq!(same["changed"], false);
        assert!(same["changedBounds"].is_null());
        new[4] = 3;
        new[4 * 5 + 3] = 4;
        let result = options.compare(&old, &new, 3, 2).unwrap();
        assert_eq!(result["changedPixels"], 2);
        assert_eq!(
            result["changedBounds"],
            json!({"x":1,"y":0,"width":2,"height":2})
        );
        let options =
            ChangeOptions::parse(&json!({"pixelThreshold":3,"minChangedPixels":2})).unwrap();
        let result = options.compare(&old, &new, 3, 2).unwrap();
        assert_eq!(result["changedPixels"], 1);
        assert_eq!(result["changed"], false);
    }

    #[test]
    fn region_ignores_outside_noise_and_keeps_absolute_coordinates() {
        let old = vec![0; 24];
        let mut new = old.clone();
        new[0] = 255;
        new[20] = 255;
        let options =
            ChangeOptions::parse(&json!({"region":{"x":2,"y":1,"width":1,"height":1}})).unwrap();
        let result = options.compare(&old, &new, 3, 2).unwrap();
        assert_eq!(result["changedPixels"], 1);
        assert_eq!(result["comparedPixels"], 1);
        assert_eq!(
            result["changedBounds"],
            json!({"x":2,"y":1,"width":1,"height":1})
        );
    }

    #[test]
    fn rejects_invalid_options_and_buffers() {
        for value in [
            json!(null),
            json!({"unknown":1}),
            json!({"pixelThreshold":256}),
            json!({"minChangedPixels":0}),
            json!({"minChangedPixels":MAX_TRACKED_PIXELS+1}),
            json!({"baselineFrameId":"A".repeat(32)}),
            json!({"baselineFrameId":"a"}),
            json!({"region":{"x":0,"y":0,"width":0,"height":1}}),
            json!({"region":{"x":u32::MAX,"y":0,"width":1,"height":1}}),
        ] {
            assert!(ChangeOptions::parse(&value).is_err(), "{value}");
        }
        let options = ChangeOptions::parse(&json!({})).unwrap();
        assert!(options.compare(&[0; 4], &[0; 3], 1, 1).is_err());
        assert!(options.compare(&[], &[], u32::MAX, u32::MAX).is_err());
        assert!(options.compare(&[], &[], 0, 0).is_err());
        let options =
            ChangeOptions::parse(&json!({"region":{"x":1,"y":0,"width":1,"height":1}})).unwrap();
        assert!(options.compare(&[0; 4], &[0; 4], 1, 1).is_err());
    }
}
