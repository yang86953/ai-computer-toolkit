//! 在录制 worker 内组合固定捕获、编码、分析与原子提交。

// 把错误码实现保留为当前 Adapter 的普通私有类型。
#[path = "video_recording_error.rs"]
mod error_code;

use std::{
    fs,
    path::Path,
    thread,
    // 导入编码调度使用的单调时钟。
    time::Instant,
};

use image::{ImageFormat, Rgba, RgbaImage, imageops::FilterType};
use serde_json::json;

// 导入当前 Adapter 私有封闭错误码。
use error_code::VideoRecordingErrorCode;

use crate::{
    // 导入可复用的同目录原子文件与自有视频编码 Component。
    components::{
        // 导入同目录原子文件提交 Component。
        atomic_file::{AtomicFileError, StagedFile},
        // 导入项目自有 Media Foundation 编码器。
        media_foundation_encoder::{
            // 导入单会话编码器。
            MediaFoundationEncoder,
            // 导入 provider-neutral 编码配置。
            MediaFoundationEncoderConfig,
            // 导入稳定编码器错误。
            MediaFoundationEncoderError,
        },
    },
    domain::{AppControlError, AppResult},
    recording::RecordingConfig,
};

use super::window_capture::{CapturedFrame, WindowCapture, with_window_capture};

const STORYBOARD_TILE_WIDTH: u32 = 320;
const STORYBOARD_COLUMNS: u32 = 3;

pub(crate) struct VideoRecordingResult {
    pub bytes: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub encoded_frames: u64,
    pub captured_frames: u64,
    pub device_driver: &'static str,
    pub keyframe_count: usize,
}

pub(crate) fn record_window_mp4(
    hwnd: isize,
    config: RecordingConfig,
) -> AppResult<VideoRecordingResult> {
    // 以 CREATE_NEW 在目标同目录取得唯一 staging 生命周期。
    let staged_video = StagedFile::reserve(&config.output_path).map_err(video_commit_error)?;
    // 只把私有 staging 路径交给项目自有编码 Component。
    let temporary_path = staged_video.path().to_path_buf();
    let worker_config = config.clone();
    let capture_result = with_window_capture(hwnd, "window-recording-mta", move |capture| {
        // 只使用 worker 内嵌的项目自有编码器。
        encode_capture(capture, &worker_config, &temporary_path)
    })?;
    // 在任何公开提交前验证 encoder 产生了非空候选。
    let staged_bytes = fs::metadata(staged_video.path())
        // staging 丢失时返回稳定视频错误。
        .map_err(|_| VideoRecordingErrorCode::VideoOutputMissing.error("视频候选产物不存在。"))?
        // 读取候选长度。
        .len();
    // 空候选不得替换既有目标。
    if staged_bytes == 0 {
        // RAII 会在返回时清理 staging。
        return Err(VideoRecordingErrorCode::VideoOutputMissing.error(
            // 不公开 staging 路径。
            "视频编码器返回成功，但候选输出文件为空。",
        ));
    }
    // durable flush 后以 write-through rename 原子建立主视频产物。
    staged_video
        // 只接收上层已经解析的覆盖许可。
        .commit(config.overwrite)
        // 把 Component 错误映射为领域错误。
        .map_err(video_commit_error)?;

    let bytes = fs::metadata(&config.output_path)
        // 保持既有文件系统错误消息。
        .map_err(|error| VideoRecordingErrorCode::VideoOutputMissing.error(error.to_string()))?
        .len();
    if bytes == 0 {
        // 返回稳定空产物错误。
        return Err(VideoRecordingErrorCode::VideoOutputMissing.error(
            // 保持既有公开消息。
            "视频编码器返回成功，但输出文件为空。",
        ));
    }

    let analysis = write_analysis_artifacts(
        &config,
        &capture_result.keyframes,
        capture_result.width,
        capture_result.height,
        bytes,
        &capture_result,
    )?;
    let duration_ms = config.duration_ms();
    Ok(VideoRecordingResult {
        bytes,
        source_width: capture_result.source_width,
        source_height: capture_result.source_height,
        width: capture_result.width,
        height: capture_result.height,
        fps: config.fps,
        duration_ms,
        encoded_frames: capture_result.encoded_frames,
        captured_frames: capture_result.captured_frames,
        device_driver: capture_result.device_driver,
        keyframe_count: analysis.keyframe_count,
    })
}

struct CaptureEncodingResult {
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    encoded_frames: u64,
    captured_frames: u64,
    device_driver: &'static str,
    keyframes: Vec<Keyframe>,
}

fn encode_capture(
    capture: &mut WindowCapture,
    config: &RecordingConfig,
    temporary_path: &Path,
) -> AppResult<CaptureEncodingResult> {
    let source_width = capture.width();
    let source_height = capture.height();
    let (width, height) = output_dimensions(source_width, source_height, config.max_width)?;
    // 用 provider-neutral 配置启动项目自有编码器。
    let mut encoder = MediaFoundationEncoder::start(
        // 传入私有候选路径。
        temporary_path,
        // 传入无 provider 类型的冻结配置。
        MediaFoundationEncoderConfig {
            // 传入有界编码宽度。
            width,
            // 传入有界编码高度。
            height,
            // 传入有界帧率。
            frames_per_second: config.fps,
            // 传入 provider-neutral 质量等级。
            quality: config.quality,
        },
    )
    // 映射为 Recording Module 稳定错误。
    .map_err(video_encoder_error)?;
    let first = capture.next_frame(config.timeout)?;
    let mut current = scale_frame(first, width, height)?;
    let total_frames = total_frames(config.duration_ms(), config.fps);
    let interval = std::time::Duration::from_secs_f64(1.0 / f64::from(config.fps));
    let started = Instant::now();
    let mut captured_frames = 1_u64;
    let mut collector = KeyframeCollector::new(config.max_keyframes, config.change_threshold);

    for index in 0..total_frames {
        if index > 0 {
            let scheduled = started + interval.mul_f64(index as f64);
            if let Some(remaining) = scheduled.checked_duration_since(Instant::now()) {
                thread::sleep(remaining);
            }
            if let Some(frame) = capture.try_latest_frame()? {
                current = scale_frame(frame, width, height)?;
                captured_frames += 1;
            }
        }
        // 把当前顶向下 RGBA 帧写入内嵌编码器。
        encoder
            // 调用固定帧契约。
            .write_rgba_frame(&current)
            // 映射为 Recording Module 稳定错误。
            .map_err(video_encoder_error)?;
        collector.observe(
            index,
            timestamp_ms(index, config.fps),
            &current,
            width,
            height,
        )?;
    }
    // 完成 H.264 与 MP4 容器收尾。
    encoder.finish().map_err(video_encoder_error)?;

    Ok(CaptureEncodingResult {
        source_width,
        source_height,
        width,
        height,
        encoded_frames: total_frames,
        captured_frames,
        device_driver: capture.device_driver(),
        keyframes: collector.finish(),
    })
}

fn total_frames(duration_ms: u64, fps: u32) -> u64 {
    duration_ms
        .saturating_mul(u64::from(fps))
        .div_ceil(1_000)
        .max(1)
}

fn timestamp_ms(index: u64, fps: u32) -> u64 {
    index.saturating_mul(1_000) / u64::from(fps)
}

fn output_dimensions(
    source_width: u32,
    source_height: u32,
    max_width: u32,
) -> AppResult<(u32, u32)> {
    if source_width < 2 || source_height < 2 {
        // 返回稳定目标尺寸错误。
        return Err(VideoRecordingErrorCode::CaptureTargetFailed.error(
            // 保持既有公开消息。
            "视频目标尺寸必须至少为 2x2。",
        ));
    }
    let width = source_width.min(max_width) & !1;
    let scaled_height = u64::from(source_height)
        .saturating_mul(u64::from(width))
        .div_ceil(u64::from(source_width));
    let height = u32::try_from(scaled_height).unwrap_or(u32::MAX).max(2) & !1;
    Ok((width.max(2), height.max(2)))
}

fn scale_frame(frame: CapturedFrame, width: u32, height: u32) -> AppResult<Vec<u8>> {
    if frame.width == width && frame.height == height {
        return Ok(frame.rgba);
    }
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.rgba).ok_or_else(|| {
        // 返回稳定帧回读错误。
        VideoRecordingErrorCode::CaptureReadbackFailed.error("捕获帧长度与 RGBA 尺寸不一致。")
    })?;
    Ok(image::imageops::resize(&image, width, height, FilterType::Triangle).into_raw())
}

#[derive(Clone)]
struct Keyframe {
    frame_index: u64,
    timestamp_ms: u64,
    change_score: f64,
    rgba: Vec<u8>,
}

struct KeyframeCollector {
    max_keyframes: usize,
    change_threshold: f64,
    previous_thumbnail: Option<Vec<u8>>,
    first: Option<Keyframe>,
    changes: Vec<Keyframe>,
    last_frame_index: u64,
    last_timestamp_ms: u64,
    last_change_score: f64,
    last_rgba: Vec<u8>,
}

impl KeyframeCollector {
    fn new(max_keyframes: usize, change_threshold: f64) -> Self {
        Self {
            max_keyframes,
            change_threshold,
            previous_thumbnail: None,
            first: None,
            changes: Vec::new(),
            last_frame_index: 0,
            last_timestamp_ms: 0,
            last_change_score: 0.0,
            last_rgba: Vec::new(),
        }
    }

    fn observe(
        &mut self,
        frame_index: u64,
        timestamp_ms: u64,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> AppResult<()> {
        let thumbnail = luma_thumbnail(rgba, width, height)?;
        let change_score = self
            .previous_thumbnail
            .as_deref()
            .map_or(0.0, |previous| difference_score(previous, &thumbnail));
        self.previous_thumbnail = Some(thumbnail);
        self.last_frame_index = frame_index;
        self.last_timestamp_ms = timestamp_ms;
        self.last_change_score = change_score;
        self.last_rgba.clear();
        self.last_rgba.extend_from_slice(rgba);

        if self.first.is_none() {
            self.first = Some(Keyframe {
                frame_index,
                timestamp_ms,
                change_score,
                rgba: rgba.to_vec(),
            });
        } else if change_score >= self.change_threshold {
            self.consider_change(Keyframe {
                frame_index,
                timestamp_ms,
                change_score,
                rgba: rgba.to_vec(),
            });
        }
        Ok(())
    }

    fn consider_change(&mut self, candidate: Keyframe) {
        let capacity = self.max_keyframes.saturating_sub(2);
        if capacity == 0 {
            return;
        }
        if self.changes.len() < capacity {
            self.changes.push(candidate);
            return;
        }
        if let Some((index, weakest)) = self
            .changes
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| left.change_score.total_cmp(&right.change_score))
            && candidate.change_score > weakest.change_score
        {
            self.changes[index] = candidate;
        }
    }

    fn finish(mut self) -> Vec<Keyframe> {
        let mut selected = self.first.take().into_iter().collect::<Vec<_>>();
        selected.append(&mut self.changes);
        let first_index = selected.first().map(|frame| frame.frame_index);
        let visually_changed =
            self.last_change_score >= self.change_threshold || selected.len() > 1;
        if first_index != Some(self.last_frame_index) && visually_changed {
            selected.retain(|frame| frame.frame_index != self.last_frame_index);
            selected.push(Keyframe {
                frame_index: self.last_frame_index,
                timestamp_ms: self.last_timestamp_ms,
                change_score: self.last_change_score,
                rgba: self.last_rgba,
            });
        }
        selected.sort_by_key(|frame| frame.frame_index);
        selected.truncate(self.max_keyframes);
        selected
    }
}

fn luma_thumbnail(rgba: &[u8], width: u32, height: u32) -> AppResult<Vec<u8>> {
    const THUMB_WIDTH: u32 = 64;
    const THUMB_HEIGHT: u32 = 36;
    let expected = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(usize::try_from(height).ok()?))
        .and_then(|value| value.checked_mul(4))
        // 将尺寸溢出映射为分析失败。
        .ok_or_else(|| VideoRecordingErrorCode::VideoAnalysisFailed.error("帧长度溢出。"))?;
    if rgba.len() != expected {
        // 返回稳定帧长度错误。
        return Err(VideoRecordingErrorCode::VideoAnalysisFailed.error(
            // 保持既有公开消息。
            "关键帧分析收到长度不正确的 RGBA 帧。",
        ));
    }
    let mut thumbnail = Vec::with_capacity((THUMB_WIDTH * THUMB_HEIGHT) as usize);
    for y in 0..THUMB_HEIGHT {
        let source_y = y.saturating_mul(height) / THUMB_HEIGHT;
        for x in 0..THUMB_WIDTH {
            let source_x = x.saturating_mul(width) / THUMB_WIDTH;
            let pixel = (u64::from(source_y) * u64::from(width) + u64::from(source_x)) * 4;
            let index = usize::try_from(pixel).map_err(|_| {
                // 返回稳定采样索引错误。
                VideoRecordingErrorCode::VideoAnalysisFailed.error("缩略图采样索引溢出。")
            })?;
            let luma = (77_u16 * u16::from(rgba[index])
                + 150_u16 * u16::from(rgba[index + 1])
                + 29_u16 * u16::from(rgba[index + 2]))
                >> 8;
            thumbnail.push(u8::try_from(luma).unwrap_or(u8::MAX));
        }
    }
    Ok(thumbnail)
}

fn difference_score(left: &[u8], right: &[u8]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 1.0;
    }
    let total = left
        .iter()
        .zip(right)
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum::<u64>();
    total as f64 / (left.len() as f64 * 255.0)
}

struct AnalysisResult {
    // 保存已落盘的关键帧数量。
    keyframe_count: usize,
}

fn write_analysis_artifacts(
    config: &RecordingConfig,
    keyframes: &[Keyframe],
    width: u32,
    height: u32,
    video_bytes: u64,
    capture: &CaptureEncodingResult,
) -> AppResult<AnalysisResult> {
    fs::create_dir_all(&config.analysis_dir)
        // 保持既有文件系统错误消息。
        .map_err(|error| {
            // 由 Adapter 私有类型选择分析写入失败码。
            VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
        })?;
    if config.overwrite {
        remove_owned_analysis_files(&config.analysis_dir)?;
    }

    let mut metadata = Vec::with_capacity(keyframes.len());
    for (index, keyframe) in keyframes.iter().enumerate() {
        let path = config.analysis_dir.join(format!(
            "frame-{:02}-{:06}ms.png",
            index + 1,
            keyframe.timestamp_ms
        ));
        save_rgba_png(&path, &keyframe.rgba, width, height)?;
        // manifest 只能引用最终公开目录，不能泄漏 worker staging。
        let public_path = config
            // 从固定分析目录开始组合。
            .public_analysis_dir
            // 复用由本函数生成的受控文件名。
            .join(path.file_name().unwrap_or_default());
        // 记录 provider-neutral 关键帧事实。
        metadata.push(json!({
            "index": index + 1,
            "frameIndex": keyframe.frame_index,
            "timestampMs": keyframe.timestamp_ms,
            "changeScore": keyframe.change_score,
            "path": public_path,
        }));
    }

    let storyboard_path = config.analysis_dir.join("storyboard.png");
    write_storyboard(&storyboard_path, keyframes, width, height)?;
    let manifest_path = config.analysis_dir.join("manifest.json");
    let manifest = json!({
        "schemaVersion": 1,
        "video": {
            "path": config.public_output_path,
            "bytes": video_bytes,
            "codec": "H.264",
            "container": "MP4",
            "fps": config.fps,
            "durationMs": config.duration_ms(),
            "sourceWidth": capture.source_width,
            "sourceHeight": capture.source_height,
            "width": capture.width,
            "height": capture.height,
            "quality": config.quality,
            "encodedFrames": capture.encoded_frames,
            "capturedFrames": capture.captured_frames,
            "audioCaptured": false,
            "cursorCaptured": false,
        },
        "analysis": {
            "strategy": "temporal-difference-keyframes",
            "changeThreshold": config.change_threshold,
            "maxKeyframes": config.max_keyframes,
            "selectedKeyframes": metadata,
            "storyboardPath": config.public_analysis_dir.join("storyboard.png"),
            "aiInputPolicy": "先分析 storyboard；只有细节不足时才读取 selectedKeyframes 中的单帧，不把完整视频逐帧送入模型。",
        }
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        // 保持既有序列化错误消息。
        .map_err(|error| {
            // 由 Adapter 私有类型选择分析写入失败码。
            VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
        })?;
    fs::write(&manifest_path, manifest_bytes)
        // 保持既有文件系统错误消息。
        .map_err(|error| {
            // 由 Adapter 私有类型选择分析写入失败码。
            VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
        })?;
    Ok(AnalysisResult {
        // 返回已写入的有界关键帧数量。
        keyframe_count: keyframes.len(),
    })
}

fn remove_owned_analysis_files(directory: &Path) -> AppResult<()> {
    for entry in fs::read_dir(directory)
        // 保持既有目录读取错误消息。
        .map_err(|error| {
            // 由 Adapter 私有类型选择分析写入失败码。
            VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
        })?
    {
        let path = entry
            .map_err(|error| {
                // 由 Adapter 私有类型选择分析写入失败码。
                VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
            })?
            .path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let owned = matches!(name, "storyboard.png" | "manifest.json")
            || (name.starts_with("frame-") && name.ends_with(".png"));
        if owned && path.is_file() {
            fs::remove_file(&path).map_err(|error| {
                // 由 Adapter 私有类型选择分析写入失败码。
                VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
            })?;
        }
    }
    Ok(())
}

fn save_rgba_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> AppResult<()> {
    image::save_buffer_with_format(
        path,
        rgba,
        width,
        height,
        image::ColorType::Rgba8,
        ImageFormat::Png,
    )
    // 保持既有图像写入错误消息。
    .map_err(|error| {
        // 由 Adapter 私有类型选择分析写入失败码。
        VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
    })
}

fn write_storyboard(path: &Path, keyframes: &[Keyframe], width: u32, height: u32) -> AppResult<()> {
    if keyframes.is_empty() {
        // 返回稳定空关键帧错误。
        return Err(VideoRecordingErrorCode::VideoAnalysisFailed.error(
            // 保持既有公开消息。
            "录制没有产生可分析的关键帧。",
        ));
    }
    let tile_width = width.min(STORYBOARD_TILE_WIDTH);
    let tile_height = u32::try_from(
        u64::from(height)
            .saturating_mul(u64::from(tile_width))
            .div_ceil(u64::from(width)),
    )
    .unwrap_or(u32::MAX)
    .max(1);
    let columns = STORYBOARD_COLUMNS.min(u32::try_from(keyframes.len()).unwrap_or(1));
    let rows = u32::try_from(keyframes.len())
        .unwrap_or(u32::MAX)
        .div_ceil(columns);
    let mut storyboard = RgbaImage::from_pixel(
        columns.saturating_mul(tile_width),
        rows.saturating_mul(tile_height),
        Rgba([24, 24, 24, 255]),
    );
    for (index, keyframe) in keyframes.iter().enumerate() {
        let frame = RgbaImage::from_raw(width, height, keyframe.rgba.clone()).ok_or_else(|| {
            // 返回稳定关键帧长度错误。
            VideoRecordingErrorCode::VideoAnalysisFailed.error("关键帧长度与 RGBA 尺寸不一致。")
        })?;
        let tile = image::imageops::resize(&frame, tile_width, tile_height, FilterType::Triangle);
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        image::imageops::overlay(
            &mut storyboard,
            &tile,
            i64::from(index % columns * tile_width),
            i64::from(index / columns * tile_height),
        );
    }
    storyboard
        .save_with_format(path, ImageFormat::Png)
        // 保持既有图像写入错误消息。
        .map_err(|error| {
            // 由 Adapter 私有类型选择分析写入失败码。
            VideoRecordingErrorCode::VideoAnalysisWriteFailed.error(error.to_string())
        })
}

// 把 Component 自有错误转换为 recording 领域错误。
fn video_commit_error(error: AtomicFileError) -> AppControlError {
    // 保持覆盖冲突与技术提交失败可区分。
    match error {
        // 目标已存在时要求独立覆盖许可。
        AtomicFileError::TargetExists => VideoRecordingErrorCode::OverwriteConfirmationRequired
            .error(
                // 不公开目标或 staging 路径。
                "视频文件已存在；请增加 overwrite=true。",
            ),
        // staging 创建失败使用稳定写入错误。
        AtomicFileError::StagingCreationFailed => VideoRecordingErrorCode::VideoWriteFailed.error(
            // 提供无路径诊断。
            "无法创建独占视频 staging 文件。",
        ),
        // durable flush 失败使用稳定写入错误。
        AtomicFileError::SyncFailed => VideoRecordingErrorCode::VideoWriteFailed.error(
            // 提供无平台类型诊断。
            "视频候选产物无法完成 durable flush。",
        ),
        // 原子移动失败使用稳定写入错误。
        AtomicFileError::CommitFailed => VideoRecordingErrorCode::VideoWriteFailed.error(
            // 明确原件仍未被预先删除。
            "视频候选产物无法原子提交。",
        ),
        // 无效目标或 staging 使用封闭参数错误。
        AtomicFileError::InvalidDestination | AtomicFileError::InvalidStaging => {
            // 不泄漏底层文件身份。
            VideoRecordingErrorCode::VideoWriteFailed.error(
                // 提供统一安全诊断。
                "视频原子提交边界无效。",
            )
        }
    }
}

// 把 Media Foundation Component 错误映射为 recording 领域错误。
fn video_encoder_error(error: MediaFoundationEncoderError) -> AppControlError {
    // 保持配置、可达性、写入和收尾语义可区分。
    match error {
        // 内部配置漂移映射为启动失败。
        MediaFoundationEncoderError::InvalidConfiguration => VideoRecordingErrorCode::VideoEncoderStartFailed.error(
            // 不公开内部配置或原生类型。
            "视频编码配置不满足 Media Foundation 边界。",
        ),
        // 运行时和编码器缺失统一为可达性失败。
        MediaFoundationEncoderError::RuntimeUnavailable
        // 合并无 H.264 编码器候选。
        | MediaFoundationEncoderError::EncoderUnavailable => VideoRecordingErrorCode::VideoEncoderUnavailable.error(
            // 不公开 HRESULT、MFT 或系统组件身份。
            "当前 Windows 环境没有可用的自有 H.264 编码链路。",
        ),
        // 媒体协商或 writer 启动失败。
        MediaFoundationEncoderError::StartFailed => VideoRecordingErrorCode::VideoEncoderStartFailed.error(
            // 提供无原生细节诊断。
            "无法启动自有 H.264/MP4 编码会话。",
        ),
        // 帧边界和写入失败统一为当前帧写入错误。
        MediaFoundationEncoderError::InvalidFrame
        // 合并原生样本写入失败。
        | MediaFoundationEncoderError::WriteFailed => VideoRecordingErrorCode::VideoEncoderWriteFailed.error(
            // 不公开内部帧缓冲或原生错误。
            "无法把捕获帧写入自有视频编码器。",
        ),
        // 容器收尾失败保持独立错误。
        MediaFoundationEncoderError::FinalizeFailed => VideoRecordingErrorCode::VideoEncoderFailed.error(
            // 不公开原生收尾错误。
            "自有 H.264 编码器未能完成 MP4 收尾。",
        ),
    }
}

#[cfg(test)]
mod tests {
    // 导入纯函数与 Component 错误映射。
    use super::{
        difference_score, output_dimensions, total_frames, video_commit_error, video_encoder_error,
    };
    // 导入封闭 Component 错误夹具。
    use crate::components::{
        // 导入原子文件生命周期错误。
        atomic_file::AtomicFileError,
        // 导入自有编码器生命周期错误。
        media_foundation_encoder::MediaFoundationEncoderError,
    };

    #[test]
    fn low_frame_rate_has_a_strict_frame_budget() {
        assert_eq!(total_frames(30_000, 2), 60);
        assert_eq!(total_frames(1_001, 2), 3);
    }

    #[test]
    fn dimensions_are_downscaled_without_upscaling_and_stay_even() -> crate::domain::AppResult<()> {
        assert_eq!(output_dimensions(1920, 1080, 960)?, (960, 540));
        assert_eq!(output_dimensions(801, 601, 960)?, (800, 600));
        Ok(())
    }

    #[test]
    fn change_score_is_normalized() {
        assert_eq!(difference_score(&[0, 0], &[0, 0]), 0.0);
        assert_eq!(difference_score(&[0, 0], &[255, 255]), 1.0);
    }

    // 验证 Atomic File Component 的全部错误保持既有录制领域映射。
    #[test]
    fn atomic_file_errors_keep_video_recording_mapping() {
        // 固定完整 Component 错误与公开码对照表。
        let mappings = [
            // 既有目标继续要求独立覆盖确认。
            (
                AtomicFileError::TargetExists,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // staging 创建失败继续映射视频写入失败。
            (AtomicFileError::StagingCreationFailed, "VIDEO_WRITE_FAILED"),
            // durable flush 失败继续映射视频写入失败。
            (AtomicFileError::SyncFailed, "VIDEO_WRITE_FAILED"),
            // 原子提交失败继续映射视频写入失败。
            (AtomicFileError::CommitFailed, "VIDEO_WRITE_FAILED"),
            // 无效最终目标继续映射视频写入失败。
            (AtomicFileError::InvalidDestination, "VIDEO_WRITE_FAILED"),
            // 无效 staging 继续映射视频写入失败。
            (AtomicFileError::InvalidStaging, "VIDEO_WRITE_FAILED"),
        ];
        // 逐项核对封闭 Component 翻译。
        for (error, expected) in mappings {
            // 公开错误码必须逐字保持。
            assert_eq!(video_commit_error(error).code, expected);
        }
    }

    // 验证 Media Foundation Encoder Component 的全部错误保持既有领域映射。
    #[test]
    fn media_foundation_errors_keep_video_recording_mapping() {
        // 固定完整 Component 错误与公开码对照表。
        let mappings = [
            // 配置漂移继续映射编码器启动失败。
            (
                MediaFoundationEncoderError::InvalidConfiguration,
                "VIDEO_ENCODER_START_FAILED",
            ),
            // 运行时缺失继续映射编码器不可用。
            (
                MediaFoundationEncoderError::RuntimeUnavailable,
                "VIDEO_ENCODER_UNAVAILABLE",
            ),
            // 编码器缺失继续映射编码器不可用。
            (
                MediaFoundationEncoderError::EncoderUnavailable,
                "VIDEO_ENCODER_UNAVAILABLE",
            ),
            // 媒体 writer 启动失败继续保持独立分类。
            (
                MediaFoundationEncoderError::StartFailed,
                "VIDEO_ENCODER_START_FAILED",
            ),
            // 无效帧继续映射编码帧写入失败。
            (
                MediaFoundationEncoderError::InvalidFrame,
                "VIDEO_ENCODER_WRITE_FAILED",
            ),
            // 原生样本写入失败继续映射编码帧写入失败。
            (
                MediaFoundationEncoderError::WriteFailed,
                "VIDEO_ENCODER_WRITE_FAILED",
            ),
            // 容器收尾失败继续保持独立编码失败。
            (
                MediaFoundationEncoderError::FinalizeFailed,
                "VIDEO_ENCODER_FAILED",
            ),
        ];
        // 逐项核对封闭 Component 翻译。
        for (error, expected) in mappings {
            // 公开错误码必须逐字保持。
            assert_eq!(video_encoder_error(error).code, expected);
        }
    }

    // 验证 recording adapter 不再恢复预删目标再 rename 的非原子流程。
    #[test]
    fn recording_commit_uses_atomic_file_component() {
        // 读取当前 adapter 源码作为迁移防漂移输入。
        let source = include_str!("video_recording.rs");
        // 排除测试断言自身包含的禁止模式字符串。
        let implementation = source
            // 只取第一个测试模块之前的生产实现。
            .split_once("#[cfg(test)]")
            // 源码结构漂移时立即失败。
            .map(|(implementation, _)| implementation)
            // 保持测试无 unwrap。
            .unwrap_or(source);
        // recording 必须从 Atomic File Component 取得 staging。
        assert!(implementation.contains("StagedFile::reserve"));
        // 禁止在覆盖前删除公开视频目标。
        assert!(!implementation.contains("fs::remove_file(output_path)"));
        // 禁止恢复直接 fs::rename 提交主视频。
        assert!(!implementation.contains("fs::rename(&self.path, output_path)"));
    }
}
