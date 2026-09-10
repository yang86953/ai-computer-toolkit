//! 在私有 Windows 边界内拥有 WGC、D3D11、WinRT 与像素读回生命周期。

// 把错误码实现保留为当前 Adapter 的普通私有类型。
#[path = "window_capture_error.rs"]
mod error_code;

use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use image::{ColorType, ImageFormat};
use windows::{
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Resource, ID3D11Texture2D,
            },
            Dxgi::{Common::DXGI_FORMAT_B8G8R8A8_UNORM, IDXGIDevice},
            Gdi::HMONITOR,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
        },
        UI::WindowsAndMessaging::{IsIconic, IsWindow, IsWindowVisible},
    },
    core::{Interface, factory},
};

// 导入当前 Adapter 私有封闭错误码。
use error_code::WindowCaptureErrorCode;

// 导入捕获预检与语言中立错误边界。
use crate::{
    // 导入零帧 eligibility 预检 Component。
    adapters::window_capture_preflight_windows,
    // 导入稳定字节摘要 Component。
    components::byte_digest,
    // 导入语言中立错误。
    domain::{AppControlError, AppResult},
};

pub struct CaptureResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub device_driver: &'static str,
    // 保存对原始 RGBA 像素计算的稳定摘要。
    pub pixel_digest: String,
}

pub(crate) struct CapturedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// 保存不触碰 surface 的首帧元数据。
pub(crate) struct CaptureFrameMetadata {
    // 保存首帧内容宽度。
    pub(crate) width: u32,
    // 保存首帧内容高度。
    pub(crate) height: u32,
    // 保存封闭 D3D 驱动分类。
    pub(crate) device_driver: &'static str,
}

pub(crate) struct WindowCapture {
    // WGC 内部使用自由线程帧池；依赖对象必须按 pool -> session -> item -> D3D 的顺序释放。
    resources: CaptureResources,
    _item: GraphicsCaptureItem,
    _winrt_device: IDirect3DDevice,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    width: u32,
    height: u32,
    device_driver: &'static str,
    _runtime: WinRtGuard,
}

// 兼容入口使用既有捕获尺寸上限。
pub fn capture_window_png(
    // 接收私有原生窗口句柄。
    hwnd: isize,
    // 接收固定 PNG 输出路径。
    path: &Path,
    // 接收首帧 deadline。
    timeout: Duration,
) -> AppResult<CaptureResult> {
    // 委托有界实现并保持既有 16384px 限制。
    capture_window_png_bounded(hwnd, path, timeout, 16_384)
}

// 在指定单边上限内捕获并编码一个精确窗口 PNG。
pub(crate) fn capture_window_png_bounded(
    // 接收私有原生窗口句柄。
    hwnd: isize,
    // 接收只由调用方拥有的 staging 路径。
    path: &Path,
    // 接收首帧 deadline。
    timeout: Duration,
    // 接收领域契约的单边像素上限。
    maximum_dimension: u32,
) -> AppResult<CaptureResult> {
    // 冻结线程安全的输出路径。
    let path = path.to_path_buf();
    // 在固定 MTA 线程内连续完成 eligibility 复核、捕获和编码。
    thread::Builder::new()
        // 使用固定诊断线程名。
        .name("window-capture-mta".to_owned())
        // 启动只处理该精确目标的线程。
        .spawn(move || {
            // 初始化并持有整个预检与捕获生命周期的 WinRT apartment。
            let runtime = WinRtGuard::initialize()?;
            // 在不反初始化 apartment 的前提下重复 worker eligibility 预检。
            let preflight =
                window_capture_preflight_windows::inspect_in_initialized_apartment(hwnd)?;
            // 不可捕获目标禁止读取 frame surface 或写候选。
            if preflight.eligibility != "eligible-for-certified-capture-route" {
                // 失败闭合且不激活窗口。
                return Err(WindowCaptureErrorCode::CaptureTargetIneligible.error(
                    // 只公开封闭分类。
                    format!("Capture target is ineligible: {}.", preflight.eligibility),
                ));
            }
            // 在同一 apartment 内创建捕获对象，避免缓存 factory 跨 apartment 失效。
            let mut capture = WindowCapture::open_with_runtime(hwnd, runtime)?;
            // 在读取 surface 前执行领域尺寸上限。
            if capture.width() > maximum_dimension || capture.height() > maximum_dimension {
                // 拒绝超出公开截图契约的窗口。
                return Err(WindowCaptureErrorCode::CaptureTargetFailed.error(
                    // 不公开原生目标。
                    "The capture target exceeds the certified screenshot dimension limit.",
                ));
            }
            // 等待并读取唯一首帧。
            let frame = capture.next_frame(timeout)?;
            // 在编码前计算原始 RGBA 像素摘要。
            let pixel_digest = byte_digest::digest(&frame.rgba);
            // 只向调用方提供的 staging 路径编码 PNG。
            save_png(&path, &frame.rgba, frame.width, frame.height)?;
            // 读取编码后的候选文件长度。
            let bytes = fs::metadata(&path)
                // 映射候选缺失。
                .map_err(|error| {
                    // 使用封闭截图候选缺失码。
                    WindowCaptureErrorCode::ScreenshotMissing.error(error.to_string())
                })?
                // 只保留字节长度。
                .len();
            // 空候选不得建立截图事实。
            if bytes == 0 {
                // 返回稳定候选缺失错误。
                return Err(WindowCaptureErrorCode::ScreenshotMissing.error("截图文件为空。"));
            }
            // 返回不含原生目标的封闭捕获事实。
            Ok(CaptureResult {
                // 保存 staging 路径供 worker 内部核验。
                path,
                // 保存 PNG 字节数。
                bytes,
                // 保存帧宽度。
                width: frame.width,
                // 保存帧高度。
                height: frame.height,
                // 保存封闭设备分类。
                device_driver: capture.device_driver(),
                // 保存原始像素摘要。
                pixel_digest,
            })
        })
        // 映射线程创建失败。
        .map_err(|error| {
            // 使用封闭捕获启动失败码。
            WindowCaptureErrorCode::CaptureStartFailed.error(error.to_string())
        })?
        // 等待唯一 MTA 线程完成。
        .join()
        // 映射线程异常终止。
        .map_err(|_| {
            // 返回稳定 worker 失败。
            WindowCaptureErrorCode::CaptureWorkerFailed.error("后台捕获工作线程异常终止。")
        })?
}

/// 在独立 MTA 线程捕获显示器，复用 WGC 的首帧超时和资源回收；不改变隐私边框策略。
pub(crate) fn capture_monitor_frame(monitor: isize, timeout: Duration) -> AppResult<CapturedFrame> {
    thread::Builder::new()
        .name("monitor-capture-mta".to_owned())
        .spawn(move || {
            let runtime = WinRtGuard::initialize()?;
            let interop: IGraphicsCaptureItemInterop =
                factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                    .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;
            let item =
                unsafe { interop.CreateForMonitor(HMONITOR(monitor as *mut std::ffi::c_void)) }
                    .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;
            WindowCapture::open_item(item, runtime)?.next_frame(timeout)
        })
        .map_err(|error| WindowCaptureErrorCode::CaptureStartFailed.error(error.to_string()))?
        .join()
        .map_err(|_| {
            WindowCaptureErrorCode::CaptureWorkerFailed.error("Monitor capture thread failed.")
        })?
}

// 在独立 MTA 线程中取得首帧元数据且不访问 frame surface。
pub(crate) fn probe_window_frame_metadata(
    // 接收仅在 adapter 内存在的原生窗口句柄。
    hwnd: isize,
    // 接收有界首帧等待时间。
    timeout: Duration,
) -> AppResult<CaptureFrameMetadata> {
    // 在同一 MTA 线程内保持预检 factory 与捕获对象 apartment 生命周期连续。
    thread::Builder::new()
        // 使用固定诊断线程名。
        .name("window-frame-probe-mta".to_owned())
        // 启动只处理该目标的线程。
        .spawn(move || {
            // 初始化并持有整个预检与捕获生命周期的 WinRT apartment。
            let runtime = WinRtGuard::initialize()?;
            // 在不反初始化 apartment 的前提下重复 worker eligibility 预检。
            let preflight =
                window_capture_preflight_windows::inspect_in_initialized_apartment(hwnd)?;
            // 不可捕获目标禁止创建 frame pool 或 session。
            if preflight.eligibility != "eligible-for-certified-capture-route" {
                // 失败闭合且不激活窗口。
                return Err(WindowCaptureErrorCode::CaptureTargetIneligible.error(
                    // 只公开封闭 eligibility 分类。
                    format!("Capture target is ineligible: {}.", preflight.eligibility),
                ));
            }
            // 在同一 apartment 内创建捕获对象。
            let capture = WindowCapture::open_with_runtime(hwnd, runtime)?;
            // 只等待首个 frame 对象。
            let frame = wait_for_frame(&capture.resources.pool, timeout)?;
            // 只读取 frame 的内容尺寸元数据。
            let size = frame
                // 调用不会返回 pixel surface 的元数据 API。
                .ContentSize()
                // 收敛 provider 错误。
                .map_err(capture_error(
                    WindowCaptureErrorCode::CaptureFrameMetadataFailed,
                ))?;
            // 在所有验证前主动关闭 frame。
            frame
                // 释放 frame 对象而不调用 Surface。
                .Close()
                // 收敛关闭错误。
                .map_err(capture_error(
                    WindowCaptureErrorCode::CaptureFrameMetadataFailed,
                ))?;
            // 把有符号宽度转换为公开无符号元数据。
            let width = u32::try_from(size.Width).map_err(|_| {
                // 返回稳定尺寸错误。
                WindowCaptureErrorCode::CaptureFrameMetadataFailed.error("捕获帧宽度无效。")
            })?;
            // 把有符号高度转换为公开无符号元数据。
            let height = u32::try_from(size.Height).map_err(|_| {
                // 返回稳定尺寸错误。
                WindowCaptureErrorCode::CaptureFrameMetadataFailed.error("捕获帧高度无效。")
            })?;
            // 复用截图路径的尺寸上限校验。
            validate_dimensions(width, height)?;
            // 返回不含 surface、像素或原生目标的封闭事实。
            Ok(CaptureFrameMetadata {
                // 保存经验证宽度。
                width,
                // 保存经验证高度。
                height,
                // 保存硬件或 WARP 分类。
                device_driver: capture.device_driver(),
            })
        })
        // 映射线程创建失败。
        .map_err(|error| {
            // 使用封闭捕获启动失败码。
            WindowCaptureErrorCode::CaptureStartFailed.error(error.to_string())
        })?
        // 等待唯一 MTA 线程完成。
        .join()
        // 映射线程异常终止。
        .map_err(|_| {
            // 返回稳定 worker 失败。
            WindowCaptureErrorCode::CaptureWorkerFailed.error("后台捕获工作线程异常终止。")
        })?
}

pub(crate) fn with_window_capture<T, F>(hwnd: isize, worker_name: &str, work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut WindowCapture) -> AppResult<T> + Send + 'static,
{
    let worker_name = worker_name.to_owned();
    thread::Builder::new()
        .name(worker_name)
        .spawn(move || {
            // 在同一个 MTA apartment 内拥有 worker 二次预检与完整捕获生命周期。
            let runtime = WinRtGuard::initialize()?;
            // 避免 WinRT factory 缓存跨越已反初始化 apartment 和线程。
            let preflight =
                window_capture_preflight_windows::inspect_in_initialized_apartment(hwnd)?;
            // worker 必须在触碰 frame surface 前独立重验资格。
            if preflight.eligibility != "eligible-for-certified-capture-route" {
                // 返回封闭资格分类且不创建捕获 session。
                return Err(WindowCaptureErrorCode::CaptureTargetIneligible.error(
                    // 只公开封闭分类。
                    format!("Capture target is ineligible: {}.", preflight.eligibility),
                ));
            }
            // 在同一 apartment 中接管 runtime 并创建 WGC 对象。
            let mut capture = WindowCapture::open_with_runtime(hwnd, runtime)?;
            work(&mut capture)
        })
        // 映射线程创建失败。
        .map_err(|error| {
            // 使用封闭捕获启动失败码。
            WindowCaptureErrorCode::CaptureStartFailed.error(error.to_string())
        })?
        .join()
        // 映射线程异常终止。
        .map_err(|_| {
            // 使用封闭捕获 worker 失败码。
            WindowCaptureErrorCode::CaptureWorkerFailed.error("后台捕获工作线程异常终止。")
        })?
}

impl WindowCapture {
    // 在调用方已经初始化并拥有 WinRT apartment 时创建 WGC 捕获对象。
    fn open_with_runtime(
        // 接收私有窗口句柄。
        hwnd: isize,
        // 接管整个对象生命周期的 apartment guard。
        runtime: WinRtGuard,
    ) -> AppResult<Self> {
        // 在创建 WGC 对象前再次验证目标。
        validate_target(hwnd)?;
        Self::open_item(capture_item(hwnd)?, runtime)
    }

    fn open_item(item: GraphicsCaptureItem, runtime: WinRtGuard) -> AppResult<Self> {
        if !GraphicsCaptureSession::IsSupported()
            // 把 runtime 探测失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureUnavailable))?
        {
            // 返回稳定 WGC 不可用错误。
            return Err(WindowCaptureErrorCode::CaptureUnavailable.error(
                // 保持既有公开消息。
                "当前 Windows 版本不支持 Windows Graphics Capture。",
            ));
        }

        let (device, context, device_driver) = create_d3d_device()?;
        let size = item
            .Size()
            // 把 item 尺寸读取失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureTargetFailed))?;
        if size.Width <= 0 || size.Height <= 0 {
            // 返回稳定非正尺寸错误。
            return Err(WindowCaptureErrorCode::CaptureTargetFailed.error(
                // 保持既有公开消息。
                "目标窗口没有可捕获的正尺寸表面。",
            ));
        }
        let width = u32::try_from(size.Width)
            // 把宽度转换失败收敛到封闭类型。
            .map_err(|_| WindowCaptureErrorCode::CaptureTargetFailed.error("窗口宽度无效。"))?;
        let height = u32::try_from(size.Height)
            // 把高度转换失败收敛到封闭类型。
            .map_err(|_| WindowCaptureErrorCode::CaptureTargetFailed.error("窗口高度无效。"))?;
        validate_dimensions(width, height)?;

        let inspectable = unsafe {
            CreateDirect3D11DeviceFromDXGIDevice(
                &device
                    .cast::<IDXGIDevice>()
                    // 把 DXGI device 转换失败收敛到封闭类型。
                    .map_err(capture_error(WindowCaptureErrorCode::CaptureDeviceFailed))?,
            )
        }
        // 把 WinRT device 创建失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureDeviceFailed))?;
        let winrt_device: IDirect3DDevice = inspectable
            .cast()
            // 把 inspectable 转换失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureDeviceFailed))?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )
        // 把自由线程帧池创建失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;
        let session = pool
            .CreateCaptureSession(&item)
            // 把 WGC session 创建失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;
        let resources = CaptureResources { pool, session };
        resources
            .session
            .SetIsCursorCaptureEnabled(false)
            // 把固定光标策略配置失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;
        resources
            .session
            .StartCapture()
            // 把 WGC 启动失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureStartFailed))?;

        Ok(Self {
            resources,
            _item: item,
            _winrt_device: winrt_device,
            context,
            device,
            width,
            height,
            device_driver,
            _runtime: runtime,
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn device_driver(&self) -> &'static str {
        self.device_driver
    }

    pub(crate) fn next_frame(&mut self, timeout: Duration) -> AppResult<CapturedFrame> {
        let frame = wait_for_frame(&self.resources.pool, timeout)?;
        self.read_and_close(frame)
    }

    pub(crate) fn try_latest_frame(&mut self) -> AppResult<Option<CapturedFrame>> {
        let mut latest = None;
        while let Ok(frame) = self.resources.pool.TryGetNextFrame() {
            if let Some(stale) = latest.replace(frame) {
                stale
                    .Close()
                    // 把旧帧关闭失败收敛到封闭类型。
                    .map_err(capture_error(WindowCaptureErrorCode::CaptureCleanupFailed))?;
            }
        }
        latest.map(|frame| self.read_and_close(frame)).transpose()
    }

    fn read_and_close(&self, frame: Direct3D11CaptureFrame) -> AppResult<CapturedFrame> {
        let content_size = frame
            .ContentSize()
            // 把内容尺寸读取失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
        let result = if content_size.Width == i32::try_from(self.width).unwrap_or_default()
            && content_size.Height == i32::try_from(self.height).unwrap_or_default()
        {
            read_frame_rgba(&self.device, &self.context, &frame, self.width, self.height).map(
                |rgba| CapturedFrame {
                    rgba,
                    width: self.width,
                    height: self.height,
                },
            )
        } else {
            // 返回带安全尺寸详情的稳定错误。
            Err(WindowCaptureErrorCode::CaptureSizeChanged.with_details(
                // 保持既有公开消息。
                "录制期间目标窗口尺寸发生变化；为避免畸变和错误坐标，已停止捕获。",
                // 只公开尺寸，不公开原生目标。
                serde_json::json!({
                    "initialWidth": self.width,
                    "initialHeight": self.height,
                    "currentWidth": content_size.Width,
                    "currentHeight": content_size.Height,
                }),
            ))
        };
        let close_result = frame
            .Close()
            // 把当前帧关闭失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureCleanupFailed));
        match result {
            Err(error) => Err(error),
            Ok(frame) => {
                close_result?;
                Ok(frame)
            }
        }
    }
}

fn validate_target(hwnd: isize) -> AppResult<()> {
    let hwnd = HWND(hwnd as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        // 返回稳定目标失效错误。
        return Err(WindowCaptureErrorCode::TargetNotFound.error(
            // 保持既有公开消息。
            "目标 HWND 已失效。",
        ));
    }
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        // 返回稳定隐藏目标错误。
        return Err(WindowCaptureErrorCode::CaptureTargetHidden.error(
            // 保持既有公开消息。
            "目标窗口不可见；后台截图不会显示或激活隐藏窗口。",
        ));
    }
    if unsafe { IsIconic(hwnd) }.as_bool() {
        // 返回稳定最小化目标错误。
        return Err(WindowCaptureErrorCode::CaptureTargetMinimized.error(
            // 保持既有公开消息。
            "目标窗口已最小化，没有可保证的新合成帧；请先由用户恢复窗口。",
        ));
    }
    Ok(())
}

fn validate_dimensions(width: u32, height: u32) -> AppResult<()> {
    const MAX_DIMENSION: u32 = 16_384;
    const MAX_PIXELS: u64 = 100_000_000;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        // 把像素计数溢出收敛到封闭类型。
        .ok_or_else(|| WindowCaptureErrorCode::CaptureTargetFailed.error("截图尺寸溢出。"))?;
    if width > MAX_DIMENSION || height > MAX_DIMENSION || pixels > MAX_PIXELS {
        // 返回稳定尺寸上限错误。
        return Err(WindowCaptureErrorCode::CaptureTargetFailed.error(
            // 保持既有公开消息。
            "截图尺寸超过 16384px 单边或 1 亿像素限制。",
        ));
    }
    Ok(())
}

fn create_d3d_device() -> AppResult<(ID3D11Device, ID3D11DeviceContext, &'static str)> {
    match create_d3d_device_with_driver(D3D_DRIVER_TYPE_HARDWARE) {
        Ok((device, context)) => Ok((device, context, "hardware")),
        Err(hardware_error) => match create_d3d_device_with_driver(D3D_DRIVER_TYPE_WARP) {
            Ok((device, context)) => Ok((device, context, "warp")),
            // 两种设备均失败时返回安全聚合详情。
            Err(warp_error) => Err(WindowCaptureErrorCode::CaptureDeviceFailed.with_details(
                // 保持既有公开消息。
                "D3D11 Hardware 与 WARP 截图设备均创建失败。",
                // 只公开既有平台错误消息，不公开对象身份。
                serde_json::json!({
                    "hardware": hardware_error.message,
                    "warp": warp_error.message,
                }),
            )),
        },
    }
}

fn create_d3d_device_with_driver(
    driver_type: windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE,
) -> AppResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    // 把 D3D11 device 创建失败收敛到封闭类型。
    .map_err(capture_error(WindowCaptureErrorCode::CaptureDeviceFailed))?;
    let device = device
        // 把缺失 device 收敛到封闭类型。
        .ok_or_else(|| {
            WindowCaptureErrorCode::CaptureDeviceFailed.error("D3D11 未返回 device。")
        })?;
    let context = context.ok_or_else(|| {
        // 把缺失 context 收敛到封闭类型。
        WindowCaptureErrorCode::CaptureDeviceFailed.error("D3D11 未返回 device context。")
    })?;
    Ok((device, context))
}

fn capture_item(hwnd: isize) -> AppResult<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        // 把 interop factory 缺失收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureUnavailable))?;
    unsafe { interop.CreateForWindow(HWND(hwnd as *mut std::ffi::c_void)) }
        // 把 WGC item 创建失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureTargetFailed))
}

fn wait_for_frame(
    pool: &Direct3D11CaptureFramePool,
    timeout: Duration,
) -> AppResult<Direct3D11CaptureFrame> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(frame) = pool.TryGetNextFrame() {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            // 返回稳定首帧等待超时。
            return Err(WindowCaptureErrorCode::CaptureTimeout.error(
                // 保持既有有界 deadline 消息。
                format!("目标窗口在 {}ms 内没有提供合成帧。", timeout.as_millis()),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_frame_rgba(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &Direct3D11CaptureFrame,
    width: u32,
    height: u32,
) -> AppResult<Vec<u8>> {
    let surface = frame
        .Surface()
        // 把 surface 获取失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let access: IDirect3DDxgiInterfaceAccess = surface
        .cast()
        // 把 DXGI access 转换失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let texture: ID3D11Texture2D =
        // 把 D3D11 texture 获取失败收敛到封闭类型。
        unsafe { access.GetInterface() }
            .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM || desc.Width < width || desc.Height < height {
        // 返回带安全格式与尺寸详情的稳定错误。
        return Err(WindowCaptureErrorCode::CaptureReadbackFailed.with_details(
            // 保持既有公开消息。
            "截图帧格式或尺寸不符合 BGRA8 读取契约。",
            // 只公开格式与尺寸，不公开原生对象。
            serde_json::json!({
                "format": desc.Format.0,
                "textureWidth": desc.Width,
                "textureHeight": desc.Height,
                "contentWidth": width,
                "contentHeight": height,
            }),
        ));
    }

    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }
        // 把 staging texture 创建失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let staging = staging.ok_or_else(|| {
        // 把缺失 staging texture 收敛到封闭类型。
        WindowCaptureErrorCode::CaptureReadbackFailed.error("D3D11 未返回 staging texture。")
    })?;
    let source: ID3D11Resource = texture
        .cast()
        // 把 source resource 转换失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let destination: ID3D11Resource = staging
        .cast()
        // 把 staging resource 转换失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    unsafe { context.CopyResource(&destination, &source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(&destination, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        // 把 CPU 映射失败收敛到封闭类型。
        .map_err(capture_error(WindowCaptureErrorCode::CaptureReadbackFailed))?;
    let result = copy_bgra_rows(&mapped, width, height);
    unsafe { context.Unmap(&destination, 0) };
    result
}

fn copy_bgra_rows(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
) -> AppResult<Vec<u8>> {
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        // 把行长度溢出收敛到封闭类型。
        .ok_or_else(|| WindowCaptureErrorCode::CaptureReadbackFailed.error("截图行长度溢出。"))?;
    let row_pitch = usize::try_from(mapped.RowPitch)
        // 把 row pitch 转换失败收敛到封闭类型。
        .map_err(|_| {
            WindowCaptureErrorCode::CaptureReadbackFailed.error("截图 row pitch 无效。")
        })?;
    if row_pitch < row_bytes || mapped.pData.is_null() {
        // 返回稳定映射结果错误。
        return Err(WindowCaptureErrorCode::CaptureReadbackFailed.error(
            // 保持既有公开消息。
            "截图映射结果为空或 row pitch 小于内容宽度。",
        ));
    }
    let total = row_bytes
        .checked_mul(usize::try_from(height).unwrap_or_default())
        // 把缓冲长度溢出收敛到封闭类型。
        .ok_or_else(|| WindowCaptureErrorCode::CaptureReadbackFailed.error("截图缓冲长度溢出。"))?;
    let mut rgba = vec![0_u8; total];
    for row in 0..usize::try_from(height).unwrap_or_default() {
        let source = unsafe {
            std::slice::from_raw_parts((mapped.pData as *const u8).add(row * row_pitch), row_bytes)
        };
        let start = row * row_bytes;
        rgba[start..start + row_bytes].copy_from_slice(source);
    }
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(rgba)
}

fn save_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> AppResult<()> {
    image::save_buffer_with_format(
        path,
        rgba,
        width,
        height,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    // 把 PNG 编码失败收敛到封闭类型。
    .map_err(|error| WindowCaptureErrorCode::ScreenshotWriteFailed.error(error.to_string()))
}

// 把平台错误转换为调用点选择的封闭 Adapter 错误。
fn capture_error(
    code: WindowCaptureErrorCode,
) -> impl FnOnce(windows::core::Error) -> AppControlError {
    // 保持既有平台错误消息并隐藏平台类型。
    move |error| code.error(error.to_string())
}

struct CaptureResources {
    // 先声明帧池，使自动字段析构同样先释放内部 worker 所有者。
    pool: Direct3D11CaptureFramePool,
    // 后声明会话，使其引用在帧池完全释放之后析构。
    session: GraphicsCaptureSession,
}

impl Drop for CaptureResources {
    fn drop(&mut self) {
        // 先关闭自由线程帧池，阻止内部 worker 继续派发或保留帧。
        let _ = self.pool.Close();
        // 再关闭捕获会话，避免帧池 worker 观察到已卸载的 GraphicsCapture runtime。
        let _ = self.session.Close();
    }
}

struct WinRtGuard;

impl WinRtGuard {
    fn initialize() -> AppResult<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            // 把 apartment 初始化失败收敛到封闭类型。
            .map_err(capture_error(WindowCaptureErrorCode::CaptureUnavailable))?;
        Ok(Self)
    }
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

// 声明首帧元数据路径的静态安全测试。
#[cfg(test)]
mod frame_metadata_probe_tests {
    // 验证探针函数不会静默扩展为像素读取或文件写入。
    #[test]
    fn probe_function_does_not_access_surface_or_persist_pixels() {
        // 读取当前 Rust 源码作为静态契约夹具。
        let source = include_str!("window_capture.rs");
        // 从探针函数开始切片。
        let Some(probe_source) = source
            // 定位探针入口。
            .split("pub(crate) fn probe_window_frame_metadata")
            // 取得函数及其后续文本。
            .nth(1)
        else {
            // 源码漂移时立即失败。
            panic!("probe function must exist");
        };
        // 截取探针函数边界。
        let Some(probe) = probe_source
            // 在下一个公共捕获生命周期入口前结束。
            .split("pub(crate) fn with_window_capture")
            // 取得仅包含探针函数的片段。
            .next()
        else {
            // 切片边界漂移时立即失败。
            panic!("probe function boundary must exist");
        };
        // 禁止取得 frame surface。
        assert!(!probe.contains(".Surface("));
        // 禁止进入 CPU readback。
        assert!(!probe.contains("read_frame_rgba("));
        // 禁止进入 PNG 写入。
        assert!(!probe.contains("save_png("));
        // 必须显式读取且只读取帧尺寸元数据。
        assert!(probe.contains(".ContentSize()"));
        // 必须显式关闭首帧对象。
        assert!(probe.contains(".Close()"));
    }
}

#[cfg(test)]
mod tests {
    use super::{D3D11_MAPPED_SUBRESOURCE, copy_bgra_rows, validate_dimensions};

    // 验证自由线程帧池的显式关闭与字段析构都早于 capture session。
    #[test]
    fn free_threaded_capture_resources_close_in_dependency_order() {
        // 读取当前 adapter 源码作为生命周期契约夹具。
        let source = include_str!("window_capture.rs");
        // 截取资源结构定义。
        let structure = source
            // 从精确结构声明后开始。
            .split("struct CaptureResources")
            // 结构声明必须存在。
            .nth(1)
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("CaptureResources structure must exist"))
            // 在 Drop 实现前结束。
            .split("impl Drop for CaptureResources")
            // 取得唯一结构片段。
            .next()
            // split 总会产生首段。
            .unwrap_or_default();
        // 定位帧池字段以锁定自动析构顺序。
        let pool_field = structure
            // 查找精确帧池字段。
            .find("pool: Direct3D11CaptureFramePool")
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("capture pool field must exist"));
        // 定位会话字段以锁定自动析构顺序。
        let session_field = structure
            // 查找精确会话字段。
            .find("session: GraphicsCaptureSession")
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("capture session field must exist"));
        // Rust 字段必须按帧池、会话顺序自动析构。
        assert!(pool_field < session_field);
        // 截取资源 Drop 实现。
        let drop_body = source
            // 从精确 Drop 声明后开始。
            .split("impl Drop for CaptureResources")
            // Drop 实现必须存在。
            .nth(1)
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("CaptureResources drop must exist"))
            // 在 WinRT apartment guard 前结束。
            .split("struct WinRtGuard")
            // 取得唯一 Drop 片段。
            .next()
            // split 总会产生首段。
            .unwrap_or_default();
        // 定位显式帧池关闭调用。
        let pool_close = drop_body
            // 查找精确 Close 调用。
            .find("self.pool.Close()")
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("capture pool close must exist"));
        // 定位显式会话关闭调用。
        let session_close = drop_body
            // 查找精确 Close 调用。
            .find("self.session.Close()")
            // 缺失时提供明确漂移诊断。
            .unwrap_or_else(|| panic!("capture session close must exist"));
        // 显式关闭也必须先停止帧池内部 worker。
        assert!(pool_close < session_close);
    }

    #[test]
    fn converts_pitched_bgra_rows_to_rgba() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = [1_u8, 2, 3, 255, 0, 0, 0, 0, 4, 5, 6, 255, 0, 0, 0, 0];
        let mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: bytes.as_ptr().cast_mut().cast(),
            RowPitch: 8,
            DepthPitch: 16,
        };
        let rgba = copy_bgra_rows(&mapped, 1, 2)?;
        assert_eq!(rgba, [3, 2, 1, 255, 6, 5, 4, 255]);
        Ok(())
    }

    #[test]
    fn rejects_unbounded_capture_dimensions() {
        assert!(validate_dimensions(20_000, 1).is_err());
        assert!(validate_dimensions(10_001, 10_001).is_err());
    }
}
