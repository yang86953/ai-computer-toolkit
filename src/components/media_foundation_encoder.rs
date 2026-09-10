//! 使用 Windows 内建 Media Foundation 把顶向下 RGBA 帧编码为 H.264/MP4。

// 导入裸指针、Windows 宽路径与平台无关路径类型。
use std::{
    // 导入 C 空指针类型供 COM 枚举结果释放使用。
    ffi::c_void,
    // 导入 Windows 原生路径宽字符转换契约。
    os::windows::ffi::OsStrExt,
    // 导入只读输出路径契约。
    path::Path,
    // 导入空指针构造函数。
    ptr,
    // 导入切片构造函数以管理 COM 返回的激活器数组。
    slice,
};

// 导入只在 Component 内部出现的 Media Foundation 与 COM 类型。
use windows::{
    // 导入 Media Foundation 编码接口、属性键与媒体类型。
    Win32::Media::MediaFoundation::{
        // 导入 H.264 编码器激活器接口以正确释放枚举结果。
        IMFActivate,
        // 导入 Sink Writer 所需的属性接口。
        IMFAttributes,
        // 导入持有帧字节的媒体缓冲接口。
        IMFMediaBuffer,
        // 导入通用媒体类型接口。
        IMFMediaType,
        // 导入最终写入 MP4 的 Sink Writer 接口。
        IMFSinkWriter,
        // 导入输出平均码率属性键。
        MF_MT_AVG_BITRATE,
        // 导入输入样本固定长度属性键。
        MF_MT_FIXED_SIZE_SAMPLES,
        // 导入帧率属性键。
        MF_MT_FRAME_RATE,
        // 导入帧尺寸属性键。
        MF_MT_FRAME_SIZE,
        // 导入逐行扫描属性键。
        MF_MT_INTERLACE_MODE,
        // 导入媒体主类型属性键。
        MF_MT_MAJOR_TYPE,
        // 导入 H.264 profile 属性键。
        MF_MT_MPEG2_PROFILE,
        // 导入方形像素属性键。
        MF_MT_PIXEL_ASPECT_RATIO,
        // 导入固定样本长度属性键。
        MF_MT_SAMPLE_SIZE,
        // 导入媒体子类型属性键。
        MF_MT_SUBTYPE,
        // 导入允许 Sink Writer 使用硬件转换器的属性键。
        MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
        // 导入禁用实时节流的 Sink Writer 属性键。
        MF_SINK_WRITER_DISABLE_THROTTLING,
        // 导入 Media Foundation 平台版本。
        MF_VERSION,
        // 导入创建属性集合的函数。
        MFCreateAttributes,
        // 导入创建媒体类型的函数。
        MFCreateMediaType,
        // 导入创建媒体缓冲的函数。
        MFCreateMemoryBuffer,
        // 导入创建媒体样本的函数。
        MFCreateSample,
        // 导入创建 MP4 Sink Writer 的函数。
        MFCreateSinkWriterFromURL,
        // 导入视频媒体主类型。
        MFMediaType_Video,
        // 导入完整 Media Foundation 启动模式。
        MFSTARTUP_FULL,
        // 导入关闭 Media Foundation 平台的函数。
        MFShutdown,
        // 导入启动 Media Foundation 平台的函数。
        MFStartup,
        // 导入视频编码器类别。
        MFT_CATEGORY_VIDEO_ENCODER,
        // 导入异步编码器枚举标志。
        MFT_ENUM_FLAG_ASYNCMFT,
        // 导入稳定优先级排序标志。
        MFT_ENUM_FLAG_SORTANDFILTER,
        // 导入同步编码器枚举标志。
        MFT_ENUM_FLAG_SYNCMFT,
        // 导入编码器类型约束结构。
        MFT_REGISTER_TYPE_INFO,
        // 导入枚举 H.264 编码器的函数。
        MFTEnumEx,
        // 导入 H.264 输出子类型。
        MFVideoFormat_H264,
        // 导入与 Windows BGRX 内存布局匹配的输入子类型。
        MFVideoFormat_RGB32,
        // 导入逐行扫描枚举值。
        MFVideoInterlace_Progressive,
        // 导入 H.264 Main profile 值。
        eAVEncH264VProfile_Main,
    },
    // 导入 COM apartment 生命周期与任务内存释放函数。
    Win32::System::Com::{
        // 导入多线程 COM apartment 初始化标志。
        COINIT_MULTITHREADED,
        // 导入 COM 初始化函数。
        CoInitializeEx,
        // 导入 COM 任务分配内存释放函数。
        CoTaskMemFree,
        // 导入 COM 反初始化函数。
        CoUninitialize,
    },
    // 导入仅用于传递私有宽路径的指针类型。
    core::PCWSTR,
};

// 定义每秒包含的 Media Foundation 百纳秒时间单位数量。
const HUNDRED_NANOSECONDS_PER_SECOND: i64 = 10_000_000;
// 限制最小可用视频码率以避免低分辨率样本被过度压缩。
const MINIMUM_BITRATE: u64 = 250_000;
// 限制最大视频码率以避免质量参数制造无界资源消耗。
const MAXIMUM_BITRATE: u64 = 20_000_000;

// 定义无原生类型泄漏的编码 Component 错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum MediaFoundationEncoderError {
    // 编码配置不满足偶数尺寸、非零帧率或质量边界。
    #[error("Media Foundation 编码配置无效")]
    InvalidConfiguration,
    // 当前线程无法建立 COM 与 Media Foundation 生命周期。
    #[error("Media Foundation 运行时不可用")]
    RuntimeUnavailable,
    // 当前 Windows 环境没有可用的 H.264 编码器。
    #[error("Media Foundation H.264 编码器不可用")]
    EncoderUnavailable,
    // 无法建立输出媒体类型或启动 Sink Writer。
    #[error("Media Foundation 编码会话启动失败")]
    StartFailed,
    // 输入帧长度与冻结的 RGBA 契约不一致。
    #[error("Media Foundation 编码帧无效")]
    InvalidFrame,
    // 无法把当前帧写入编码器。
    #[error("Media Foundation 编码帧写入失败")]
    WriteFailed,
    // 无法完成 MP4 容器收尾。
    #[error("Media Foundation MP4 收尾失败")]
    FinalizeFailed,
}

// 定义 provider-neutral 的编码启动配置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MediaFoundationEncoderConfig {
    // 记录偶数像素宽度。
    pub(crate) width: u32,
    // 记录偶数像素高度。
    pub(crate) height: u32,
    // 记录恒定帧率。
    pub(crate) frames_per_second: u32,
    // 记录一到一百的 provider-neutral 质量等级。
    pub(crate) quality: u8,
}

// 持有当前线程配对的 COM 与 Media Foundation 生命周期。
struct MediaFoundationRuntime;

// 实现当前线程的原生媒体运行时启动契约。
impl MediaFoundationRuntime {
    // 建立 MTA COM apartment 并启动 Media Foundation。
    fn start() -> Result<Self, MediaFoundationEncoderError> {
        // 初始化或加入当前线程既有的 MTA apartment。
        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // apartment 模式冲突或初始化失败时封闭返回。
        if com_result.is_err() {
            // 不泄漏 HRESULT，只公开稳定 Component 错误。
            return Err(MediaFoundationEncoderError::RuntimeUnavailable);
        }
        // 启动完整 Media Foundation 平台服务。
        let media_result = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) };
        // Media Foundation 启动失败时回滚已经取得的 COM 引用。
        if media_result.is_err() {
            // 对成功的 CoInitializeEx 调用执行配对释放。
            unsafe { CoUninitialize() };
            // 返回稳定运行时不可用错误。
            return Err(MediaFoundationEncoderError::RuntimeUnavailable);
        }
        // 返回独占本次配对生命周期的守卫。
        Ok(Self)
    }
}

// 在所有退出路径上按逆序释放媒体运行时。
impl Drop for MediaFoundationRuntime {
    // 关闭 Media Foundation 并释放当前线程 COM 引用。
    fn drop(&mut self) {
        // 尽力关闭本 Component 启动的平台引用。
        let _ = unsafe { MFShutdown() };
        // 对成功的初始化调用执行配对释放。
        unsafe { CoUninitialize() };
    }
}

// 管理一个不可复用的 H.264/MP4 编码会话。
pub(crate) struct MediaFoundationEncoder {
    // 先持有 writer，确保字段析构时先于运行时释放。
    sink_writer: Option<IMFSinkWriter>,
    // 保存 Sink Writer 分配的视频流编号。
    stream_index: u32,
    // 冻结每帧预期 RGBA 字节数。
    frame_bytes: usize,
    // 冻结输入帧宽度供布局转换使用。
    width: usize,
    // 冻结输入帧高度供布局转换使用。
    height: usize,
    // 冻结每帧百纳秒持续时间。
    frame_duration: i64,
    // 记录下一个样本的单调序号。
    next_frame_index: i64,
    // 最后持有运行时，确保 writer 与样本先析构。
    _runtime: MediaFoundationRuntime,
}

// 实现编码器探测、启动、逐帧写入与收尾契约。
impl MediaFoundationEncoder {
    // 仅枚举 H.264 转换器，不创建文件或启动真实编码会话。
    pub(crate) fn is_h264_available() -> Result<bool, MediaFoundationEncoderError> {
        // 建立探测所需的配对运行时。
        let _runtime = MediaFoundationRuntime::start()?;
        // 只约束输出为视频 H.264，由 Sink Writer 负责组合颜色转换器。
        let output_type = MFT_REGISTER_TYPE_INFO {
            // 声明视频主类型。
            guidMajorType: MFMediaType_Video,
            // 声明 H.264 输出子类型。
            guidSubtype: MFVideoFormat_H264,
        };
        // 接收由 COM 任务分配器创建的激活器数组。
        let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
        // 接收数组元素数量。
        let mut activation_count = 0_u32;
        // 枚举系统同步与异步软件转换器并使用系统排序。
        let flags = MFT_ENUM_FLAG_SYNCMFT
            // 包含异步转换器。
            | MFT_ENUM_FLAG_ASYNCMFT
            // 请求稳定的系统过滤排序。
            | MFT_ENUM_FLAG_SORTANDFILTER;
        // 执行只读编码器枚举。
        let enumeration = unsafe {
            // 把输入输出约束交给 Media Foundation。
            MFTEnumEx(
                // 只枚举视频编码器类别。
                MFT_CATEGORY_VIDEO_ENCODER,
                // 应用完整候选标志。
                flags,
                // 不要求编码器直接接收 RGB32，避免排除可由 Sink Writer 组合的编码器。
                None,
                // 传入固定 H.264 输出约束。
                Some(&output_type),
                // 接收激活器数组所有权。
                &mut activations,
                // 接收元素数量。
                &mut activation_count,
            )
        };
        // 枚举失败时避免解引用任何不可信结果。
        if enumeration.is_err() {
            // 返回稳定的编码器不可用错误。
            return Err(MediaFoundationEncoderError::EncoderUnavailable);
        }
        // 释放每个激活器及其 COM 任务数组。
        unsafe { release_activations(activations, activation_count) };
        // 仅以非零候选数判定当前运行时可用。
        Ok(activation_count > 0)
    }

    // 启动一个只接收冻结尺寸顶向下 RGBA 帧的 MP4 会话。
    pub(crate) fn start(
        // 接收只供本次会话写入的私有 staging 路径。
        output_path: &Path,
        // 接收 provider-neutral 配置。
        config: MediaFoundationEncoderConfig,
    ) -> Result<Self, MediaFoundationEncoderError> {
        // 验证静态配置并取得安全派生值。
        let derived = DerivedConfiguration::new(config)?;
        // 拒绝没有 H.264 编码器的环境，避免创建半成品。
        if !Self::is_h264_available()? {
            // 返回稳定的编码器不可用错误。
            return Err(MediaFoundationEncoderError::EncoderUnavailable);
        }
        // 为真实会话建立独立运行时引用。
        let runtime = MediaFoundationRuntime::start()?;
        // 创建 H.264 输出媒体类型。
        let output_media_type = create_output_media_type(config, derived.bitrate)?;
        // 创建 BGRX 输入媒体类型。
        let input_media_type = create_input_media_type(config, derived.frame_bytes)?;
        // 构造空结尾宽路径并保持到 writer 创建完成。
        let output_wide = encode_wide_path(output_path);
        // 创建 Sink Writer 私有属性集合。
        let writer_attributes = create_writer_attributes()?;
        // 建立直接写入 staging MP4 的 Sink Writer。
        let sink_writer = unsafe {
            // 不提供外部 byte stream，交由系统按扩展名创建 MP4 容器。
            MFCreateSinkWriterFromURL(
                // 传入生命周期受控的宽路径。
                PCWSTR(output_wide.as_ptr()),
                // 明确不注入外部 byte stream。
                None::<&windows::Win32::Media::MediaFoundation::IMFByteStream>,
                // 传入 Component 私有 writer 属性。
                &writer_attributes,
            )
        }
        // 统一映射创建失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
        // 注册 H.264 输出流并取得流编号。
        let stream_index = unsafe { sink_writer.AddStream(&output_media_type) }
            // 不泄漏原生错误。
            .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
        // 为输出流绑定 RGB32 输入媒体类型。
        unsafe {
            // 不附加 encoder-specific input attributes。
            sink_writer.SetInputMediaType(stream_index, &input_media_type, None::<&IMFAttributes>)
        }
        // 统一映射输入协商失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
        // 开始接收样本。
        unsafe { sink_writer.BeginWriting() }
            // 统一映射启动失败。
            .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
        // 返回冻结契约的单会话编码器。
        Ok(Self {
            // 持有唯一 writer。
            sink_writer: Some(sink_writer),
            // 保存协商后的流编号。
            stream_index,
            // 保存预期字节数。
            frame_bytes: derived.frame_bytes,
            // 保存宽度。
            width: config.width as usize,
            // 保存高度。
            height: config.height as usize,
            // 保存帧持续时间。
            frame_duration: derived.frame_duration,
            // 第一帧从零时间开始。
            next_frame_index: 0,
            // 最后保存运行时守卫。
            _runtime: runtime,
        })
    }

    // 写入一帧顶向下 RGBA 像素。
    pub(crate) fn write_rgba_frame(
        // 以可变借用推进帧序号。
        &mut self,
        // 接收严格等于冻结尺寸的 RGBA 字节。
        rgba: &[u8],
    ) -> Result<(), MediaFoundationEncoderError> {
        // 在接触原生缓冲前验证长度。
        if rgba.len() != self.frame_bytes {
            // 返回稳定的帧契约错误。
            return Err(MediaFoundationEncoderError::InvalidFrame);
        }
        // 把长度转换为 Media Foundation 的 u32 边界。
        let frame_bytes = u32::try_from(self.frame_bytes)
            // 超界视为冻结配置无效。
            .map_err(|_| MediaFoundationEncoderError::InvalidFrame)?;
        // 创建一帧固定长度媒体缓冲。
        let buffer = unsafe { MFCreateMemoryBuffer(frame_bytes) }
            // 统一映射缓冲分配失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 把顶向下 RGBA 转成 Media Foundation RGB32 所需的底向上 BGRX。
        copy_rgba_to_bottom_up_bgrx(&buffer, rgba, self.width, self.height)?;
        // 标记有效缓冲长度。
        unsafe { buffer.SetCurrentLength(frame_bytes) }
            // 统一映射缓冲状态失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 创建持有该缓冲的媒体样本。
        let sample = unsafe { MFCreateSample() }
            // 统一映射样本创建失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 把帧缓冲附加到样本。
        unsafe { sample.AddBuffer(&buffer) }
            // 统一映射缓冲附加失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 计算单调样本时间戳并检测溢出。
        let sample_time = self
            // 从当前帧序号开始。
            .next_frame_index
            // 乘以冻结帧时长。
            .checked_mul(self.frame_duration)
            // 溢出时封闭失败。
            .ok_or(MediaFoundationEncoderError::WriteFailed)?;
        // 设置样本起始时间。
        unsafe { sample.SetSampleTime(sample_time) }
            // 统一映射时间戳失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 设置样本固定持续时间。
        unsafe { sample.SetSampleDuration(self.frame_duration) }
            // 统一映射时长失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 读取仍处于活动状态的 writer。
        let writer = self
            // 借用可选 writer。
            .sink_writer
            // 转换为共享引用。
            .as_ref()
            // 缺失表示会话已被内部破坏。
            .ok_or(MediaFoundationEncoderError::WriteFailed)?;
        // 把样本提交给 H.264 流。
        unsafe { writer.WriteSample(self.stream_index, &sample) }
            // 统一映射写入失败。
            .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
        // 单调推进下一帧序号。
        self.next_frame_index = self
            // 读取当前序号。
            .next_frame_index
            // 只增加一个样本。
            .checked_add(1)
            // 溢出时封闭失败。
            .ok_or(MediaFoundationEncoderError::WriteFailed)?;
        // 报告当前帧已经由 writer 接管。
        Ok(())
    }

    // 消耗会话并完成 MP4 索引与容器收尾。
    pub(crate) fn finish(mut self) -> Result<(), MediaFoundationEncoderError> {
        // 在 Finalize 前移出 writer，确保所有退出路径都先释放它。
        let writer = self
            // 取得唯一 writer 所有权。
            .sink_writer
            // 从会话中移出。
            .take()
            // 缺失表示内部生命周期已破坏。
            .ok_or(MediaFoundationEncoderError::FinalizeFailed)?;
        // 完成编码器与 MP4 容器。
        let finalize_result = unsafe { writer.Finalize() };
        // 显式先释放 writer，再由 self 释放运行时。
        drop(writer);
        // 把原生收尾结果映射为稳定错误。
        finalize_result.map_err(|_| MediaFoundationEncoderError::FinalizeFailed)
    }
}

// 保存配置校验后的安全派生值。
struct DerivedConfiguration {
    // 保存每帧字节数。
    frame_bytes: usize,
    // 保存每帧百纳秒时长。
    frame_duration: i64,
    // 保存质量派生的平均码率。
    bitrate: u32,
}

// 实现配置校验与有界派生。
impl DerivedConfiguration {
    // 验证偶数尺寸、帧率与 provider-neutral 质量等级。
    fn new(
        // 接收待验证配置。
        config: MediaFoundationEncoderConfig,
    ) -> Result<Self, MediaFoundationEncoderError> {
        // H.264 4:2:0 路径要求非零偶数尺寸。
        if config.width == 0
            // 拒绝零高度。
            || config.height == 0
            // 拒绝奇数宽度。
            || !config.width.is_multiple_of(2)
            // 拒绝奇数高度。
            || !config.height.is_multiple_of(2)
            // 拒绝零帧率。
            || config.frames_per_second == 0
            // 拒绝质量下界之外的值。
            || config.quality == 0
            // 拒绝质量上界之外的值。
            || config.quality > 100
        {
            // 返回统一配置错误。
            return Err(MediaFoundationEncoderError::InvalidConfiguration);
        }
        // 计算像素数量并检测平台字长溢出。
        let pixels = usize::try_from(config.width)
            // 宽度转换失败时封闭返回。
            .map_err(|_| MediaFoundationEncoderError::InvalidConfiguration)?
            // 乘以高度。
            .checked_mul(config.height as usize)
            // 检测像素数量溢出。
            .ok_or(MediaFoundationEncoderError::InvalidConfiguration)?;
        // 每个 RGBA 像素固定四字节。
        let frame_bytes = pixels
            // 乘以固定通道数。
            .checked_mul(4)
            // 检测帧长度溢出。
            .ok_or(MediaFoundationEncoderError::InvalidConfiguration)?;
        // 确保 Media Foundation 缓冲长度可由 u32 表示。
        let _ = u32::try_from(frame_bytes)
            // 超出原生缓冲边界时拒绝配置。
            .map_err(|_| MediaFoundationEncoderError::InvalidConfiguration)?;
        // 以整数百纳秒单位派生固定帧时长。
        let frame_duration = HUNDRED_NANOSECONDS_PER_SECOND
            // 按恒定帧率整除。
            / i64::from(config.frames_per_second);
        // 拒绝无法表示正时长的异常高帧率。
        if frame_duration == 0 {
            // 返回统一配置错误。
            return Err(MediaFoundationEncoderError::InvalidConfiguration);
        }
        // 从像素率与质量等级派生 provider-neutral 平均码率。
        let bitrate = derive_bitrate(config)?;
        // 返回完整派生配置。
        Ok(Self {
            // 保存安全帧长度。
            frame_bytes,
            // 保存正帧时长。
            frame_duration,
            // 保存有界码率。
            bitrate,
        })
    }
}

// 从像素率和一到一百质量等级派生有界平均码率。
fn derive_bitrate(
    // 接收已经通过静态边界验证的配置。
    config: MediaFoundationEncoderConfig,
) -> Result<u32, MediaFoundationEncoderError> {
    // 计算每秒像素数量。
    let pixels_per_second = u64::from(config.width)
        // 乘以高度。
        .checked_mul(u64::from(config.height))
        // 检测尺寸乘法溢出。
        .and_then(|value| value.checked_mul(u64::from(config.frames_per_second)))
        // 溢出时拒绝配置。
        .ok_or(MediaFoundationEncoderError::InvalidConfiguration)?;
    // 把质量映射到每像素每帧 0.10 到 0.60 比特区间。
    let milli_bits_per_pixel = 95_u64
        // 每级质量增加五毫比特。
        .checked_add(u64::from(config.quality) * 5)
        // 理论溢出时拒绝配置。
        .ok_or(MediaFoundationEncoderError::InvalidConfiguration)?;
    // 计算未裁剪平均码率。
    let raw_bitrate = pixels_per_second
        // 应用质量系数。
        .checked_mul(milli_bits_per_pixel)
        // 检测乘法溢出。
        .ok_or(MediaFoundationEncoderError::InvalidConfiguration)?
        // 从毫比特恢复比特。
        / 1_000;
    // 在明确资源边界内裁剪。
    let bounded_bitrate = raw_bitrate.clamp(MINIMUM_BITRATE, MAXIMUM_BITRATE);
    // 转换到 Media Foundation 的 u32 属性边界。
    u32::try_from(bounded_bitrate)
        // 理论超界时拒绝配置。
        .map_err(|_| MediaFoundationEncoderError::InvalidConfiguration)
}

// 创建 H.264 输出媒体类型。
fn create_output_media_type(
    // 接收冻结会话配置。
    config: MediaFoundationEncoderConfig,
    // 接收有界平均码率。
    bitrate: u32,
) -> Result<IMFMediaType, MediaFoundationEncoderError> {
    // 创建空媒体类型。
    let media_type = unsafe { MFCreateMediaType() }
        // 统一映射创建失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 声明视频主类型。
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 声明 H.264 输出子类型。
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 写入打包后的帧尺寸。
    set_ratio(&media_type, &MF_MT_FRAME_SIZE, config.width, config.height)?;
    // 写入打包后的恒定帧率。
    set_ratio(
        // 目标媒体类型。
        &media_type,
        // 帧率属性键。
        &MF_MT_FRAME_RATE,
        // 帧率分子。
        config.frames_per_second,
        // 帧率分母。
        1,
    )?;
    // 声明方形像素。
    set_ratio(&media_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;
    // 声明逐行扫描视频。
    unsafe {
        // 写入 Media Foundation 的逐行扫描枚举值。
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
    }
    // 统一映射属性失败。
    .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 写入有界平均码率。
    unsafe { media_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 请求广泛可用的 H.264 Main profile。
    unsafe {
        // 写入编码 profile 数值。
        media_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)
    }
    // 统一映射属性失败。
    .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 返回完整输出类型。
    Ok(media_type)
}

// 创建与底向上 BGRX 缓冲匹配的输入媒体类型。
fn create_input_media_type(
    // 接收冻结会话配置。
    config: MediaFoundationEncoderConfig,
    // 接收固定样本字节数。
    frame_bytes: usize,
) -> Result<IMFMediaType, MediaFoundationEncoderError> {
    // 创建空媒体类型。
    let media_type = unsafe { MFCreateMediaType() }
        // 统一映射创建失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 声明视频主类型。
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 声明 RGB32/BGRX 内存子类型。
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 写入打包后的帧尺寸。
    set_ratio(&media_type, &MF_MT_FRAME_SIZE, config.width, config.height)?;
    // 写入打包后的恒定帧率。
    set_ratio(
        // 目标媒体类型。
        &media_type,
        // 帧率属性键。
        &MF_MT_FRAME_RATE,
        // 帧率分子。
        config.frames_per_second,
        // 帧率分母。
        1,
    )?;
    // 声明方形像素。
    set_ratio(&media_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;
    // 声明逐行扫描视频。
    unsafe {
        // 写入 Media Foundation 的逐行扫描枚举值。
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
    }
    // 统一映射属性失败。
    .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 标记所有输入样本长度固定。
    unsafe { media_type.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 转换固定样本长度。
    let frame_bytes = u32::try_from(frame_bytes)
        // 超界时封闭返回。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 写入固定样本长度。
    unsafe { media_type.SetUINT32(&MF_MT_SAMPLE_SIZE, frame_bytes) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 返回完整输入类型。
    Ok(media_type)
}

// 创建只影响本地 Sink Writer 的属性集合。
fn create_writer_attributes() -> Result<IMFAttributes, MediaFoundationEncoderError> {
    // 接收创建后的属性接口。
    let mut attributes = None;
    // 为两项固定属性预留空间。
    unsafe { MFCreateAttributes(&mut attributes, 2) }
        // 统一映射创建失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 取得已创建属性接口。
    let attributes = attributes.ok_or(MediaFoundationEncoderError::StartFailed)?;
    // 固定使用进程内软件转换器，避免驱动异步生命周期越过 worker 收尾。
    unsafe { attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 禁用实时节流，使离线测试和录制按调用方时序写入。
    unsafe { attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)?;
    // 返回完整属性集合。
    Ok(attributes)
}

// 按 Media Foundation 约定打包两个 u32 属性分量。
fn set_ratio(
    // 接收目标媒体类型。
    media_type: &IMFMediaType,
    // 接收目标属性键。
    key: &windows::core::GUID,
    // 接收高三十二位分量。
    numerator: u32,
    // 接收低三十二位分量。
    denominator: u32,
) -> Result<(), MediaFoundationEncoderError> {
    // 把两个分量无损打包为 u64。
    let packed = (u64::from(numerator) << 32) | u64::from(denominator);
    // 写入媒体类型属性。
    unsafe { media_type.SetUINT64(key, packed) }
        // 统一映射属性失败。
        .map_err(|_| MediaFoundationEncoderError::StartFailed)
}

// 把一帧顶向下 RGBA 转为底向上 BGRX 并写入锁定缓冲。
fn copy_rgba_to_bottom_up_bgrx(
    // 接收目标 Media Foundation 缓冲。
    buffer: &IMFMediaBuffer,
    // 接收源 RGBA 字节。
    rgba: &[u8],
    // 接收冻结宽度。
    width: usize,
    // 接收冻结高度。
    height: usize,
) -> Result<(), MediaFoundationEncoderError> {
    // 接收锁定缓冲起始地址。
    let mut destination = ptr::null_mut();
    // 锁定缓冲并只请求地址。
    unsafe { buffer.Lock(&mut destination, None, None) }
        // 统一映射锁定失败。
        .map_err(|_| MediaFoundationEncoderError::WriteFailed)?;
    // 按行和像素执行无分配布局转换。
    for destination_y in 0..height {
        // RGB32 正 stride 对应底向上行序。
        let source_y = height - 1 - destination_y;
        // 遍历当前行的每个像素。
        for x in 0..width {
            // 计算源 RGBA 像素偏移。
            let source_index = (source_y * width + x) * 4;
            // 计算目标 BGRX 像素偏移。
            let destination_index = (destination_y * width + x) * 4;
            // 写入蓝色通道。
            unsafe {
                destination
                    .add(destination_index)
                    .write(rgba[source_index + 2])
            };
            // 写入绿色通道。
            unsafe {
                destination
                    .add(destination_index + 1)
                    .write(rgba[source_index + 1])
            };
            // 写入红色通道。
            unsafe {
                destination
                    .add(destination_index + 2)
                    .write(rgba[source_index])
            };
            // 固定忽略 alpha 并写入不透明填充值。
            unsafe { destination.add(destination_index + 3).write(255) };
        }
    }
    // 解除缓冲锁定并确保失败可见。
    unsafe { buffer.Unlock() }
        // 统一映射解锁失败。
        .map_err(|_| MediaFoundationEncoderError::WriteFailed)
}

// 释放 MFTEnumEx 返回的每个 COM 接口及任务内存数组。
unsafe fn release_activations(
    // 接收 COM 任务分配的数组指针。
    activations: *mut Option<IMFActivate>,
    // 接收数组元素数量。
    activation_count: u32,
) {
    // 空指针不需要释放。
    if activations.is_null() {
        // 直接结束清理。
        return;
    }
    // 把原生数组恢复为有界可变切片。
    let activation_slice = unsafe {
        // 元素数量来自成功的 MFTEnumEx 调用。
        slice::from_raw_parts_mut(activations, activation_count as usize)
    };
    // 逐一释放每个 IMFActivate 引用。
    for activation in activation_slice {
        // 移出并立即析构可选 COM 接口。
        drop(activation.take());
    }
    // 释放承载元素的 COM 任务内存数组。
    unsafe { CoTaskMemFree(Some(activations.cast::<c_void>())) };
}

// 把 Windows 路径编码为以空字符结尾的 UTF-16。
fn encode_wide_path(path: &Path) -> Vec<u16> {
    // 保留原始 Windows 路径编码并追加终止符。
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// 在同模块内注册动态媒体回读测试。
#[cfg(test)]
#[path = "media_foundation_encoder_tests.rs"]
mod tests;
