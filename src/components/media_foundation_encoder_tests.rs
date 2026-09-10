//! 动态验证项目自有 Media Foundation H.264/MP4 编码 Component。

// 导入临时文件、路径与唯一序列工具。
use std::{
    // 导入文件读取与删除接口。
    fs,
    // 导入临时文件路径类型。
    path::{Path, PathBuf},
    // 导入进程内唯一测试序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入 Media Foundation Source Reader 动态回读接口。
use windows::{
    // 导入 Source Reader、媒体属性与 H.264 常量。
    Win32::Media::MediaFoundation::{
        // 导入媒体属性接口用于读取原生子类型。
        IMFAttributes,
        // 导入原生媒体子类型属性键。
        MF_MT_SUBTYPE,
        // 导入首个视频流索引。
        MF_SOURCE_READER_FIRST_VIDEO_STREAM,
        // 导入流结束标志。
        MF_SOURCE_READERF_ENDOFSTREAM,
        // 导入从 URL 创建 Source Reader 的函数。
        MFCreateSourceReaderFromURL,
        // 导入 H.264 子类型。
        MFVideoFormat_H264,
    },
    // 导入媒体类型属性基接口转换能力。
    core::Interface,
    // 导入私有宽路径指针。
    core::PCWSTR,
};

// 导入被测 Component 与同模块私有运行时工具。
use super::{
    // 导入编码会话。
    MediaFoundationEncoder,
    // 导入 provider-neutral 配置。
    MediaFoundationEncoderConfig,
    // 导入媒体运行时守卫。
    MediaFoundationRuntime,
    // 导入 Windows 路径编码函数。
    encode_wide_path,
};

// 为并行测试提供进程内唯一序列。
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 在测试退出时尽力删除临时媒体文件。
struct TemporaryMediaFile {
    // 保存唯一临时路径。
    path: PathBuf,
}

// 实现临时媒体文件生命周期。
impl TemporaryMediaFile {
    // 创建尚不存在的唯一 MP4 路径。
    fn new() -> Self {
        // 取得进程内唯一序号。
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录构造明确 MP4 扩展名。
        let path = std::env::temp_dir().join(format!(
            // 使用固定前缀、进程号与序号避免冲突。
            "ai-computer-toolkit-mf-{}-{sequence}.mp4",
            // 插入当前测试进程号。
            std::process::id()
        ));
        // 清理此前异常退出可能遗留的同名文件。
        let _ = fs::remove_file(&path);
        // 返回临时文件所有者。
        Self { path }
    }

    // 只读借出测试路径。
    fn path(&self) -> &Path {
        // 返回借用而不转移清理职责。
        &self.path
    }
}

// 确保失败路径也不会遗留测试媒体。
impl Drop for TemporaryMediaFile {
    // 尽力删除测试文件。
    fn drop(&mut self) {
        // 忽略文件尚未创建或已经删除的情况。
        let _ = fs::remove_file(&self.path);
    }
}

// 验证合成 RGBA 帧可生成并由 Media Foundation 回读为 H.264 样本。
#[test]
fn synthetic_rgba_frames_produce_readable_h264_mp4() -> Result<(), Box<dyn std::error::Error>> {
    // 当前环境必须公开至少一个 H.264 编码器候选。
    assert!(MediaFoundationEncoder::is_h264_available()?);
    // 创建自动清理的唯一输出路径。
    let output = TemporaryMediaFile::new();
    // 冻结小尺寸低帧率测试配置。
    let config = MediaFoundationEncoderConfig {
        // 使用偶数宽度。
        width: 320,
        // 使用偶数高度。
        height: 240,
        // 使用两帧每秒以降低测试成本。
        frames_per_second: 2,
        // 使用中高 provider-neutral 质量。
        quality: 75,
    };
    // 启动真实 Media Foundation Sink Writer。
    let mut encoder = MediaFoundationEncoder::start(output.path(), config)?;
    // 写入六帧具有时序变化的合成图像。
    for frame_index in 0_u8..6 {
        // 生成当前顶向下 RGBA 帧。
        let frame = synthetic_frame(config.width, config.height, frame_index);
        // 写入当前帧。
        encoder.write_rgba_frame(&frame)?;
    }
    // 完成 H.264 与 MP4 容器收尾。
    encoder.finish()?;
    // 读取完整文件以验证真实非空容器事实。
    let bytes = fs::read(output.path())?;
    // MP4 必须明显大于空 box 头。
    assert!(bytes.len() > 1_024);
    // 文件前部必须包含 MP4 的 ftyp box 标记。
    assert!(bytes.windows(4).take(64).any(|window| window == b"ftyp"));
    // 使用 Media Foundation Source Reader 验证原生 H.264 流与至少一个样本。
    assert_media_foundation_reads_h264_sample(output.path())?;
    // 报告动态编码与回读验证成功。
    Ok(())
}

// 验证配置与帧长度错误在接触写入边界前封闭失败。
#[test]
fn invalid_configuration_and_frame_length_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建不会提交到正式目录的临时路径。
    let output = TemporaryMediaFile::new();
    // 构造奇数宽度配置。
    let invalid_config = MediaFoundationEncoderConfig {
        // 奇数宽度违反 H.264 4:2:0 边界。
        width: 319,
        // 保持有效高度。
        height: 240,
        // 保持有效帧率。
        frames_per_second: 2,
        // 保持有效质量。
        quality: 75,
    };
    // 启动必须返回稳定配置错误。
    assert!(matches!(
        // 调用被测启动入口。
        MediaFoundationEncoder::start(output.path(), invalid_config),
        // 匹配无原生细节的错误变体。
        Err(super::MediaFoundationEncoderError::InvalidConfiguration)
    ));
    // 构造有效配置。
    let valid_config = MediaFoundationEncoderConfig {
        // 使用偶数宽度。
        width: 320,
        // 使用偶数高度。
        height: 240,
        // 使用有效帧率。
        frames_per_second: 2,
        // 使用有效质量。
        quality: 75,
    };
    // 启动真实会话。
    let mut encoder = MediaFoundationEncoder::start(output.path(), valid_config)?;
    // 写入短一字节的帧必须封闭失败。
    assert_eq!(
        // 传入错误帧长度。
        encoder.write_rgba_frame(&vec![0; 320 * 240 * 4 - 1]),
        // 匹配稳定帧错误。
        Err(super::MediaFoundationEncoderError::InvalidFrame)
    );
    // 写入一帧有效数据以允许容器正常收尾。
    encoder.write_rgba_frame(&synthetic_frame(320, 240, 0))?;
    // 完成会话并验证失败尝试没有破坏生命周期。
    encoder.finish()?;
    // 报告封闭错误与恢复路径验证成功。
    Ok(())
}

// 构造具有背景渐变与移动矩形的顶向下 RGBA 帧。
fn synthetic_frame(width: u32, height: u32, frame_index: u8) -> Vec<u8> {
    // 为全部像素预分配 RGBA 字节。
    let mut frame = vec![0_u8; width as usize * height as usize * 4];
    // 遍历顶向下行坐标。
    for y in 0..height {
        // 遍历列坐标。
        for x in 0..width {
            // 计算当前像素 RGBA 偏移。
            let index = ((y * width + x) * 4) as usize;
            // 判断像素是否位于移动矩形内。
            let moving_rectangle = x >= u32::from(frame_index) * 24
                // 限制矩形右边界。
                && x < u32::from(frame_index) * 24 + 80
                // 限制矩形垂直范围。
                && (72..168).contains(&y);
            // 写入随横坐标变化的红色通道。
            frame[index] = if moving_rectangle { 240 } else { x as u8 };
            // 写入随纵坐标变化的绿色通道。
            frame[index + 1] = if moving_rectangle { 48 } else { y as u8 };
            // 写入随帧序号变化的蓝色通道。
            frame[index + 2] = if moving_rectangle {
                // 移动矩形使用高亮蓝色。
                180
            } else {
                // 背景随时间变化。
                frame_index.saturating_mul(30)
            };
            // 固定 alpha 为不透明。
            frame[index + 3] = 255;
        }
    }
    // 返回完整合成帧。
    frame
}

// 使用 Media Foundation Source Reader 验证 H.264 原生类型和样本。
fn assert_media_foundation_reads_h264_sample(
    // 接收已经完成收尾的 MP4 路径。
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // 为回读会话建立独立媒体运行时。
    let _runtime = MediaFoundationRuntime::start()?;
    // 构造空结尾宽路径。
    let wide_path = encode_wide_path(path);
    // 创建 Source Reader 而不请求解码转换。
    let reader = unsafe {
        // 直接从已完成 MP4 文件打开。
        MFCreateSourceReaderFromURL(PCWSTR(wide_path.as_ptr()), None::<&IMFAttributes>)
    }?;
    // 把首个视频流常量转换为 ABI u32。
    let video_stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    // 读取容器公开的首个原生视频媒体类型。
    let native_media_type = unsafe { reader.GetNativeMediaType(video_stream, 0) }?;
    // 转换为属性接口以读取子类型 GUID。
    let native_attributes: IMFAttributes = native_media_type.cast()?;
    // 读取原生视频子类型。
    let subtype = unsafe { native_attributes.GetGUID(&MF_MT_SUBTYPE) }?;
    // 原生子类型必须是 H.264。
    assert_eq!(subtype, MFVideoFormat_H264);
    // 在有界次数内读取至少一个压缩视频样本。
    for _ in 0..16 {
        // 接收流标志。
        let mut flags = 0_u32;
        // 接收可选媒体样本。
        let mut sample = None;
        // 同步读取下一个视频样本。
        unsafe {
            // 不请求额外控制标志。
            reader.ReadSample(
                // 读取首个视频流。
                video_stream,
                // 使用默认读取行为。
                0,
                // 不需要实际流编号。
                None,
                // 接收结束等流标志。
                Some(&mut flags),
                // 不需要时间戳。
                None,
                // 接收可选样本。
                Some(&mut sample),
            )
        }?;
        // 取得任意一个真实样本即验证成功。
        if sample.is_some() {
            // 返回成功。
            return Ok(());
        }
        // 到达流末尾时停止读取。
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            // 跳出有界读取循环。
            break;
        }
    }
    // 没有任何视频样本时返回测试错误。
    Err("Media Foundation 未从生成的 MP4 读出 H.264 样本".into())
}
