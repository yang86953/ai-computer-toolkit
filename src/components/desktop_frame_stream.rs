//! 有界最新帧邮箱：原生 damage 更新、无元数据降级、合并背压与可重建的补丁。
use crate::components::{
    desktop_frame_changes::{MAX_TRACKED_PIXELS, Region},
    desktop_session_frame_capture::{
        DesktopFrameDataError, DesktopPackedPixelFormat, normalize_row,
    },
};
use std::{
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

pub(crate) struct MappedFrame<'a> {
    pub allocation: &'a [u8],
    pub offset: u32,
    pub chunk_size: u32,
    pub stride: i32,
    pub width: u32,
    pub height: u32,
    pub format: DesktopPackedPixelFormat,
}
impl MappedFrame<'_> {
    fn validate(&self) -> Result<(), DesktopFrameDataError> {
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels == 0
            || pixels > MAX_TRACKED_PIXELS
            || self.width > 16384
            || self.height > 16384
            || self.allocation.len() > 256 * 1024 * 1024
        {
            return Err(DesktopFrameDataError::ResourceExhausted);
        }
        let row = self.width as usize * 4;
        let stride = self.stride.unsigned_abs() as usize;
        let span = stride
            .checked_mul(self.height as usize - 1)
            .ok_or(DesktopFrameDataError::InvalidFrame)?;
        let required = span
            .checked_add(row)
            .ok_or(DesktopFrameDataError::InvalidFrame)?;
        let offset = self.offset as usize;
        let last = if self.stride > 0 {
            offset.checked_add(span)
        } else {
            offset.checked_sub(span)
        }
        .ok_or(DesktopFrameDataError::InvalidFrame)?;
        if self.stride == 0
            || stride < row
            || required > self.chunk_size as usize
            || offset
                .max(last)
                .checked_add(row)
                .is_none_or(|end| end > self.allocation.len())
        {
            return Err(DesktopFrameDataError::InvalidFrame);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FrameUpdate {
    pub sequence: u64,
    pub base_sequence: u64,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub region: Region,
    pub rgba: Vec<u8>,
    pub keyframe: bool,
    pub method: &'static str,
    pub frames_received: u64,
    pub pixels_read: u64,
    pub coalesced_frames: u64,
    pub age_ms: u64,
}

impl FrameUpdate {
    pub(crate) fn png(&self) -> Result<Vec<u8>, DesktopFrameDataError> {
        use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
        let mut output = Vec::new();
        PngEncoder::new(&mut output)
            .write_image(
                &self.rgba,
                self.region.width,
                self.region.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|_| DesktopFrameDataError::EncodingFailed)?;
        Ok(output)
    }
}

#[derive(Default)]
struct State {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    format: Option<DesktopPackedPixelFormat>,
    sequence: u64,
    source_sequence: Option<u64>,
    generation: u64,
    delivered: u64,
    delivered_generation: u64,
    dirty: Option<Region>,
    frames_received: u64,
    pixels_read: u64,
    source_pixels: u64,
    method: &'static str,
    terminal: Option<&'static str>,
    last_received: Option<Instant>,
}

/// 只有一个 producer 和串行 broker consumer；没有逐帧无界队列。
#[derive(Default)]
pub(crate) struct FrameMailbox {
    state: Mutex<State>,
    wake: Condvar,
}

fn union(a: Option<Region>, b: Region) -> Region {
    match a {
        None => b,
        Some(a) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Region {
                x,
                y,
                width: (a.x + a.width).max(b.x + b.width) - x,
                height: (a.y + a.height).max(b.y + b.height) - y,
            }
        }
    }
}

impl FrameMailbox {
    pub(crate) fn stats(&self) -> (u64, u64) {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        (s.frames_received, s.source_pixels)
    }

    pub(crate) fn finish(&self, code: &'static str) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.terminal.is_none() {
            s.terminal = Some(code);
            s.rgba.clear();
            s.rgba.shrink_to_fit();
        }
        self.wake.notify_all();
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminal
            .is_some()
    }
    pub(crate) fn ingest(
        &self,
        frame: MappedFrame<'_>,
        source_sequence: Option<u64>,
        discontinuity: bool,
        damage: Option<&[Region]>,
    ) -> Result<(), DesktopFrameDataError> {
        frame.validate()?;
        let full = Region {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        };
        if damage.is_some_and(|regions| {
            regions.len() > 256
                || regions.iter().any(|r| {
                    r.width == 0
                        || r.height == 0
                        || r.x.checked_add(r.width).is_none_or(|end| end > frame.width)
                        || r.y
                            .checked_add(r.height)
                            .is_none_or(|end| end > frame.height)
                })
        }) {
            return Err(DesktopFrameDataError::InvalidFrame);
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.terminal.is_some() {
            return Ok(());
        }
        let resized =
            s.width != frame.width || s.height != frame.height || s.format != Some(frame.format);
        let gap = discontinuity
            || s.source_sequence
                .zip(source_sequence)
                .is_some_and(|(previous, current)| previous.checked_add(1) != Some(current));
        let reset = resized || gap || s.rgba.is_empty();
        let native =
            !reset && s.source_sequence.is_some() && source_sequence.is_some() && damage.is_some();
        let area = if native {
            damage.and_then(|rs| rs.iter().copied().fold(None, |a, r| Some(union(a, r))))
        } else {
            Some(full)
        };
        if reset {
            let size = frame.width as usize * frame.height as usize * 4;
            if s.rgba.len() != size {
                // Avoid Vec's geometric growth retaining more than the advertised cache budget.
                s.rgba = vec![0; size];
            }
            s.width = frame.width;
            s.height = frame.height;
            s.format = Some(frame.format);
            s.generation += 1;
            s.dirty = Some(full);
        }
        s.source_sequence = source_sequence;
        s.frames_received += 1;
        s.source_pixels += u64::from(frame.width) * u64::from(frame.height);
        s.sequence += 1;
        s.last_received = Some(Instant::now());
        s.method = if native {
            "pipewire-damage"
        } else {
            "full-frame-diff"
        };
        if let Some(area) = area {
            let mut row = vec![0; area.width as usize * 4];
            let stride = frame.stride.unsigned_abs() as usize;
            let mut changed = None;
            for y in area.y..area.y + area.height {
                let delta = y as usize * stride;
                let start = if frame.stride > 0 {
                    frame.offset as usize + delta
                } else {
                    frame.offset as usize - delta
                } + area.x as usize * 4;
                normalize_row(
                    &frame.allocation[start..start + row.len()],
                    &mut row,
                    frame.format,
                )?;
                let destination = (y as usize * frame.width as usize + area.x as usize) * 4;
                for (column, pixel) in row.chunks_exact(4).enumerate() {
                    let index = destination + column * 4;
                    if s.rgba[index..index + 4] != *pixel {
                        s.rgba[index..index + 4].copy_from_slice(pixel);
                        changed = Some(union(
                            changed,
                            Region {
                                x: area.x + column as u32,
                                y,
                                width: 1,
                                height: 1,
                            },
                        ));
                    }
                }
            }
            s.pixels_read += u64::from(area.width) * u64::from(area.height);
            if let Some(changed) = changed {
                s.dirty = Some(union(s.dirty, changed));
            }
        }
        self.wake.notify_all();
        Ok(())
    }

    /// after=0 或交付链不匹配时返回完整关键帧。非关键帧的 region 必须贴到 base_sequence 上。
    pub(crate) fn next(
        &self,
        after: u64,
        timeout: Duration,
    ) -> Result<Option<FrameUpdate>, &'static str> {
        let deadline = Instant::now() + timeout;
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(code) = s.terminal {
                return Err(code);
            }
            let keyframe =
                after == 0 || after != s.delivered || s.delivered_generation != s.generation;
            if !s.rgba.is_empty() && (keyframe || s.dirty.is_some()) {
                let region = if keyframe {
                    Region {
                        x: 0,
                        y: 0,
                        width: s.width,
                        height: s.height,
                    }
                } else {
                    s.dirty.ok_or("INTERNAL_PROTOCOL_ERROR")?
                };
                let mut rgba =
                    Vec::with_capacity(region.width as usize * region.height as usize * 4);
                for y in region.y..region.y + region.height {
                    let start = (y as usize * s.width as usize + region.x as usize) * 4;
                    rgba.extend_from_slice(&s.rgba[start..start + region.width as usize * 4]);
                }
                let update = FrameUpdate {
                    sequence: s.sequence,
                    base_sequence: if keyframe { 0 } else { s.delivered },
                    generation: s.generation,
                    width: s.width,
                    height: s.height,
                    region,
                    rgba,
                    keyframe,
                    method: s.method,
                    frames_received: s.frames_received,
                    pixels_read: s.pixels_read,
                    coalesced_frames: s.sequence.saturating_sub(s.delivered).saturating_sub(1),
                    age_ms: s
                        .last_received
                        .map(|time| time.elapsed().as_millis() as u64)
                        .unwrap_or(0),
                };
                s.delivered = s.sequence;
                s.delivered_generation = s.generation;
                s.dirty = None;
                return Ok(Some(update));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            s = self
                .wake
                .wait_timeout(s, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(m: &FrameMailbox, pixels: &[u8], seq: Option<u64>, damage: Option<&[Region]>) {
        m.ingest(
            MappedFrame {
                allocation: pixels,
                offset: 0,
                chunk_size: pixels.len() as u32,
                stride: pixels.len() as i32,
                width: (pixels.len() / 4) as u32,
                height: 1,
                format: DesktopPackedPixelFormat::Rgba,
            },
            seq,
            false,
            damage,
        )
        .unwrap();
    }
    #[test]
    fn native_damage_reads_only_affected_pixels_and_coalesces_without_losing_regions() {
        let m = FrameMailbox::default();
        let mut pixels = vec![0; 400];
        feed(&m, &pixels, Some(1), None);
        let first = m.next(0, Duration::ZERO).unwrap().unwrap();
        assert!(first.keyframe);
        assert_eq!(first.pixels_read, 100);
        pixels[8] = 1;
        feed(
            &m,
            &pixels,
            Some(2),
            Some(&[Region {
                x: 2,
                y: 0,
                width: 1,
                height: 1,
            }]),
        );
        pixels[20] = 2;
        feed(
            &m,
            &pixels,
            Some(3),
            Some(&[Region {
                x: 5,
                y: 0,
                width: 1,
                height: 1,
            }]),
        );
        let delta = m.next(first.sequence, Duration::ZERO).unwrap().unwrap();
        assert!(!delta.keyframe);
        assert_eq!(delta.base_sequence, 1);
        assert_eq!(delta.sequence, 3);
        assert_eq!(
            delta.region,
            Region {
                x: 2,
                y: 0,
                width: 4,
                height: 1
            }
        );
        assert_eq!(delta.pixels_read, 102);
        assert_eq!(delta.method, "pipewire-damage");
        assert_eq!(delta.coalesced_frames, 1);
        assert!(m.next(delta.sequence, Duration::ZERO).unwrap().is_none());
    }
    #[test]
    fn missing_metadata_and_sequence_gaps_force_safe_full_reads_and_resynchronization() {
        let m = FrameMailbox::default();
        feed(&m, &[0; 8], Some(1), None);
        let first = m.next(0, Duration::ZERO).unwrap().unwrap();
        feed(&m, &[1; 8], Some(3), Some(&[]));
        let gap = m.next(first.sequence, Duration::ZERO).unwrap().unwrap();
        assert!(gap.keyframe);
        assert_eq!(gap.rgba, [1; 8]);
        assert_eq!(gap.method, "full-frame-diff");
        feed(&m, &[2; 8], None, Some(&[]));
        let fallback = m.next(gap.sequence, Duration::ZERO).unwrap().unwrap();
        assert_eq!(fallback.rgba, [2; 8]);
        assert_eq!(fallback.method, "full-frame-diff");
        assert!(m.next(999, Duration::ZERO).unwrap().unwrap().keyframe);
        feed(&m, &[0; 4], None, None);
        assert!(
            m.next(fallback.sequence, Duration::ZERO)
                .unwrap()
                .unwrap()
                .keyframe
        );
    }
    #[test]
    fn static_frames_do_not_emit_patches_and_finish_wakes_waiters() {
        let m = std::sync::Arc::new(FrameMailbox::default());
        feed(&m, &[0; 4], Some(1), None);
        let first = m.next(0, Duration::ZERO).unwrap().unwrap();
        feed(&m, &[0; 4], Some(2), Some(&[]));
        assert!(m.next(first.sequence, Duration::ZERO).unwrap().is_none());
        let reader = m.clone();
        let thread =
            std::thread::spawn(move || reader.next(first.sequence, Duration::from_secs(5)));
        m.finish("SUBSCRIPTION_CLOSED");
        assert_eq!(thread.join().unwrap().unwrap_err(), "SUBSCRIPTION_CLOSED");
    }
    #[test]
    fn negative_stride_bgra_updates_normalize_and_encode_the_exact_patch() {
        let m = FrameMailbox::default();
        let mut raw = [7, 6, 5, 255, 3, 2, 1, 255];
        m.ingest(
            MappedFrame {
                allocation: &raw,
                offset: 4,
                chunk_size: 8,
                stride: -4,
                width: 1,
                height: 2,
                format: DesktopPackedPixelFormat::Bgra,
            },
            Some(1),
            false,
            None,
        )
        .unwrap();
        let first = m.next(0, Duration::ZERO).unwrap().unwrap();
        assert_eq!(first.rgba, [1, 2, 3, 255, 5, 6, 7, 255]);
        raw[0] = 8;
        m.ingest(
            MappedFrame {
                allocation: &raw,
                offset: 4,
                chunk_size: 8,
                stride: -4,
                width: 1,
                height: 2,
                format: DesktopPackedPixelFormat::Bgra,
            },
            Some(2),
            false,
            Some(&[Region {
                x: 0,
                y: 1,
                width: 1,
                height: 1,
            }]),
        )
        .unwrap();
        let patch = m.next(first.sequence, Duration::ZERO).unwrap().unwrap();
        let decoded = image::load_from_memory(&patch.png().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.as_raw(), &[5, 6, 8, 255]);
    }

    #[test]
    fn invalid_mapping_and_damage_are_rejected_before_cache_mutation() {
        let m = FrameMailbox::default();
        assert!(
            m.ingest(
                MappedFrame {
                    allocation: &[0; 8],
                    offset: 0,
                    chunk_size: 8,
                    stride: 4,
                    width: 2,
                    height: 1,
                    format: DesktopPackedPixelFormat::Rgba
                },
                Some(1),
                false,
                None
            )
            .is_err()
        );
        assert!(m.next(0, Duration::ZERO).unwrap().is_none());
        let f = MappedFrame {
            allocation: &[0; 8],
            offset: 0,
            chunk_size: 8,
            stride: 8,
            width: 2,
            height: 1,
            format: DesktopPackedPixelFormat::Rgba,
        };
        assert!(
            m.ingest(
                f,
                Some(1),
                false,
                Some(&[Region {
                    x: u32::MAX,
                    y: 0,
                    width: 1,
                    height: 1
                }])
            )
            .is_err()
        );
    }
}
