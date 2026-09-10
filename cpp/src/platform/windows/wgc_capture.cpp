#include "platform/windows/wgc_capture.hpp"
#include "components/pixel_buffer.hpp"
#include "platform/windows/png_encoder.hpp"
#include "platform/windows/png_file_output.hpp"
#include <windows.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <dxgi.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <winrt/base.h>
#include <algorithm>
#include <chrono>
#include <iterator>
#include <memory>
#include <thread>
using AbiInspectable =
    winrt::impl::abi_t<winrt::Windows::Foundation::IInspectable>;
extern "C" HRESULT WINAPI CreateDirect3D11DeviceFromDXGIDevice(
    IDXGIDevice* dxgi_device,
    AbiInspectable** graphics_device);
namespace act::platform::windows {
namespace {
constexpr GUID capture_item_iid{
    0x79c3f95b,
    0x31f7,
    0x4ec2,
    {0xa4, 0x64, 0x63, 0x2e, 0xf5, 0xd3, 0x07, 0x60},
};
constexpr GUID capture_item_interop_iid{
    0x3628e81b,
    0x3cac,
    0x4c60,
    {0xb7, 0xf4, 0x23, 0xce, 0x0e, 0x0c, 0x33, 0x56},
};
struct CaptureItemInterop : IUnknown {
    virtual HRESULT STDMETHODCALLTYPE CreateForWindow(
        HWND window,
        REFIID iid,
        void** result) = 0;
    virtual HRESULT STDMETHODCALLTYPE CreateForMonitor(
        HMONITOR monitor,
        REFIID iid,
        void** result) = 0;
};
constexpr GUID dxgi_interface_access_iid{
    0xa9b3d012,
    0x3df2,
    0x4ee3,
    {0xb8, 0xd1, 0x86, 0x95, 0xf4, 0x57, 0xd3, 0xc1},
};

struct DxgiInterfaceAccess : IUnknown {
    virtual HRESULT STDMETHODCALLTYPE GetInterface(
        REFIID iid,
        void** result) = 0;
};
LRESULT CALLBACK fixture_window_proc(
    const HWND window,
    const UINT message,
    const WPARAM wparam,
    const LPARAM lparam) {
    if (message == WM_NCCREATE) {
        const auto* create =
            reinterpret_cast<const CREATESTRUCTW*>(lparam);
        SetWindowLongPtrW(
            window,
            GWLP_USERDATA,
            reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    }
    if (message == WM_PAINT) {
        PAINTSTRUCT paint{};
        const HDC context = BeginPaint(window, &paint);
        const auto* phase = reinterpret_cast<const std::uint32_t*>(
            GetWindowLongPtrW(window, GWLP_USERDATA));
        const std::uint32_t value =
            phase == nullptr ? 0U : *phase;
        const HBRUSH brush = CreateSolidBrush(RGB(
            32U + (value * 53U) % 192U,
            64U + (value * 71U) % 160U,
            96U + (value * 37U) % 128U));
        FillRect(context, &paint.rcPaint, brush);
        DeleteObject(brush);
        EndPaint(window, &paint);
        return 0;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}
class FixtureWindow final {
public:
    FixtureWindow() {
        instance_ = GetModuleHandleW(nullptr);
        class_name_ =
            L"ActWgcCaptureWorker-" +
            std::to_wstring(GetCurrentProcessId());
        WNDCLASSW window_class{};
        window_class.lpfnWndProc = fixture_window_proc;
        window_class.hInstance = instance_;
        window_class.lpszClassName = class_name_.c_str();
        atom_ = RegisterClassW(&window_class);
        if (atom_ == 0U) {
            winrt::throw_last_error();
        }
        window_ = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name_.c_str(),
            L"ACT WGC isolated capture fixture",
            WS_POPUP,
            16,
            16,
            128,
            96,
            nullptr,
            nullptr,
            instance_,
            &phase_);
        if (window_ == nullptr) {
            winrt::throw_last_error();
        }
        ShowWindow(window_, SW_SHOWNOACTIVATE);
        SetWindowPos(
            window_,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        UpdateWindow(window_);
        DwmFlush();
    }

    ~FixtureWindow() {
        if (window_ != nullptr) {
            DestroyWindow(window_);
        }
        if (atom_ != 0U) {
            UnregisterClassW(class_name_.c_str(), instance_);
        }
    }

    FixtureWindow(const FixtureWindow&) = delete;
    FixtureWindow& operator=(const FixtureWindow&) = delete;

    [[nodiscard]] HWND get() const {
        return window_;
    }

    void set_phase(const std::uint32_t phase) {
        phase_ = phase;
        InvalidateRect(window_, nullptr, FALSE);
        UpdateWindow(window_);
        DwmFlush();
    }

private:
    HINSTANCE instance_ = nullptr;
    std::wstring class_name_;
    ATOM atom_ = 0U;
    HWND window_ = nullptr;
    std::uint32_t phase_ = 0U;
};
winrt::Windows::Graphics::Capture::GraphicsCaptureItem capture_item(
    const HWND window) {
    using Item = winrt::Windows::Graphics::Capture::GraphicsCaptureItem;
    const auto factory = winrt::get_activation_factory<Item>();
    winrt::com_ptr<CaptureItemInterop> interop;
    auto* factory_unknown =
        reinterpret_cast<IUnknown*>(winrt::get_abi(factory));
    winrt::check_hresult(factory_unknown->QueryInterface(
        capture_item_interop_iid,
        interop.put_void()));
    Item item{nullptr};
    winrt::check_hresult(interop->CreateForWindow(
        window,
        capture_item_iid,
        winrt::put_abi(item)));
    return item;
}

struct D3dResources {
    winrt::com_ptr<ID3D11Device> device;
    winrt::com_ptr<ID3D11DeviceContext> context;
    winrt::Windows::Graphics::DirectX::Direct3D11::IDirect3DDevice
        winrt_device{nullptr};
    const char* driver = "none";
};

D3dResources create_d3d_resources() {
    D3dResources resources;
    D3D_FEATURE_LEVEL feature_level{};
    HRESULT status = D3D11CreateDevice(
        nullptr,
        D3D_DRIVER_TYPE_HARDWARE,
        nullptr,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr,
        0U,
        D3D11_SDK_VERSION,
        resources.device.put(),
        &feature_level,
        resources.context.put());
    if (FAILED(status)) {
        status = D3D11CreateDevice(
            nullptr,
            D3D_DRIVER_TYPE_WARP,
            nullptr,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr,
            0U,
            D3D11_SDK_VERSION,
            resources.device.put(),
            &feature_level,
            resources.context.put());
        winrt::check_hresult(status);
        resources.driver = "warp";
    } else {
        resources.driver = "hardware";
    }

    const auto dxgi_device = resources.device.as<IDXGIDevice>();
    winrt::com_ptr<AbiInspectable> inspectable;
    winrt::check_hresult(CreateDirect3D11DeviceFromDXGIDevice(
        dxgi_device.get(), inspectable.put()));
    winrt::check_hresult(inspectable->QueryInterface(
        winrt::guid_of<
            winrt::Windows::Graphics::DirectX::Direct3D11::
                IDirect3DDevice>(),
        winrt::put_abi(resources.winrt_device)));
    return resources;
}

void pump_fixture_messages() {
    MSG message{};
    while (PeekMessageW(
               &message, nullptr, 0U, 0U, PM_REMOVE) != FALSE) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

CaptureFrameResult capture(
    const HWND window,
    const bool fixture_owned,
    const bool read_surface,
    const bool encode_png) {
    if (IsWindow(window) == FALSE) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "STALE_SESSION",
                "The exact window no longer exists in the capture worker.",
            },
        };
    }
    if (!fixture_owned &&
        (IsWindowVisible(window) == FALSE ||
         IsIconic(window) != FALSE)) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "BACKGROUND_OPERATION_UNAVAILABLE",
                "The exact window is not visible and restored for capture.",
            },
        };
    }

    const HWND foreground_before = GetForegroundWindow();
    winrt::init_apartment(winrt::apartment_type::multi_threaded);
    if (!winrt::Windows::Graphics::Capture::
            GraphicsCaptureSession::IsSupported()) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "BACKGROUND_OPERATION_UNAVAILABLE",
                "Windows Graphics Capture is unsupported.",
            },
        };
    }

    const auto item = capture_item(window);
    const auto size = item.Size();
    if (size.Width <= 0 || size.Height <= 0) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "BACKGROUND_OPERATION_UNAVAILABLE",
                "The exact window has no capturable extent.",
            },
        };
    }
    auto d3d = create_d3d_resources();
    using namespace winrt::Windows::Graphics;
    auto pool = Capture::Direct3D11CaptureFramePool::CreateFreeThreaded(
        d3d.winrt_device,
        winrt::Windows::Graphics::DirectX::
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size);
    auto session = pool.CreateCaptureSession(item);
    session.IsCursorCaptureEnabled(false);
    session.StartCapture();

    Capture::Direct3D11CaptureFrame frame{nullptr};
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < deadline) {
        if (fixture_owned) {
            pump_fixture_messages();
        }
        frame = pool.TryGetNextFrame();
        if (frame != nullptr) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (frame == nullptr) {
        session.Close();
        pool.Close();
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "TIMEOUT",
                "Windows Graphics Capture did not produce a frame.",
            },
        };
    }
    const auto content_size = frame.ContentSize();
    std::int64_t pixel_bytes_read = 0;
    std::int64_t row_pitch = 0;
    std::string pixel_digest;
    std::string pixel_format;
    bool png_encoded = false;
    std::int64_t png_bytes = 0;
    std::string png_digest;
    bool png_signature_valid = false;
    std::vector<std::uint8_t> encoded_png;
    if (read_surface) {
        constexpr std::uint32_t maximum_dimension = 4096U;
        constexpr std::uint64_t maximum_bytes =
            64ULL * 1024ULL * 1024ULL;
        const auto surface = frame.Surface();
        winrt::com_ptr<DxgiInterfaceAccess> access;
        auto* surface_unknown =
            reinterpret_cast<IUnknown*>(winrt::get_abi(surface));
        winrt::check_hresult(surface_unknown->QueryInterface(
            dxgi_interface_access_iid, access.put_void()));
        winrt::com_ptr<ID3D11Texture2D> source;
        winrt::check_hresult(access->GetInterface(
            __uuidof(ID3D11Texture2D), source.put_void()));

        D3D11_TEXTURE2D_DESC description{};
        source->GetDesc(&description);
        const std::uint64_t row_bytes =
            static_cast<std::uint64_t>(description.Width) * 4ULL;
        const std::uint64_t total_bytes =
            row_bytes * static_cast<std::uint64_t>(description.Height);
        if (description.Width == 0U ||
            description.Height == 0U ||
            description.Width > maximum_dimension ||
            description.Height > maximum_dimension ||
            total_bytes > maximum_bytes ||
            description.Format != DXGI_FORMAT_B8G8R8A8_UNORM) {
            frame.Close();
            session.Close();
            pool.Close();
            return CaptureFrameResult{
                std::nullopt,
                BackendError{
                    "RESOURCE_LIMIT_EXCEEDED",
                    "The fixture surface violates the certified readback "
                    "bounds or pixel format.",
                },
            };
        }

        D3D11_TEXTURE2D_DESC staging_description = description;
        staging_description.BindFlags = 0U;
        staging_description.MiscFlags = 0U;
        staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        staging_description.Usage = D3D11_USAGE_STAGING;
        winrt::com_ptr<ID3D11Texture2D> staging;
        winrt::check_hresult(d3d.device->CreateTexture2D(
            &staging_description, nullptr, staging.put()));
        d3d.context->CopyResource(staging.get(), source.get());

        D3D11_MAPPED_SUBRESOURCE mapped{};
        winrt::check_hresult(d3d.context->Map(
            staging.get(),
            0U,
            D3D11_MAP_READ,
            0U,
            &mapped));
        if (mapped.RowPitch < row_bytes || mapped.pData == nullptr) {
            d3d.context->Unmap(staging.get(), 0U);
            frame.Close();
            session.Close();
            pool.Close();
            return CaptureFrameResult{
                std::nullopt,
                BackendError{
                    "OPERATION_FAILED",
                    "The fixture staging texture has an invalid row pitch.",
                },
            };
        }
        const auto* bytes =
            static_cast<const std::uint8_t*>(mapped.pData);
        auto converted = components::convert_bgra_to_rgba(
            bytes,
            static_cast<std::size_t>(mapped.RowPitch),
            description.Width,
            description.Height);
        d3d.context->Unmap(staging.get(), 0U);
        if (!converted.image.has_value()) {
            frame.Close();
            session.Close();
            pool.Close();
            return CaptureFrameResult{
                std::nullopt,
                BackendError{
                    converted.error_code,
                    converted.error_message,
                },
            };
        }
        auto& rgba = *converted.image;
        pixel_bytes_read =
            static_cast<std::int64_t>(rgba.pixels.size());
        row_pitch = static_cast<std::int64_t>(mapped.RowPitch);
        pixel_digest = components::byte_digest(
            rgba.pixels.data(), rgba.pixels.size());
        pixel_format = "rgba8";
        if (encode_png) {
            auto png = encode_rgba_png(
                rgba.width, rgba.height, rgba.pixels);
            if (!png.image.has_value()) {
                frame.Close();
                session.Close();
                pool.Close();
                return CaptureFrameResult{
                    std::nullopt,
                    BackendError{
                        png.error_code,
                        png.error_message,
                    },
                };
            }
            const auto& encoded = png.image->bytes;
            constexpr std::uint8_t signature[]{
                0x89U, 0x50U, 0x4eU, 0x47U,
                0x0dU, 0x0aU, 0x1aU, 0x0aU,
            };
            png_signature_valid =
                encoded.size() >= std::size(signature) &&
                std::equal(
                    std::begin(signature),
                    std::end(signature),
                    encoded.begin());
            if (!png_signature_valid) {
                frame.Close();
                session.Close();
                pool.Close();
                return CaptureFrameResult{
                    std::nullopt,
                    BackendError{
                        "OPERATION_FAILED",
                        "WIC returned data without a valid PNG signature.",
                    },
                };
            }
            png_encoded = true;
            png_bytes =
                static_cast<std::int64_t>(encoded.size());
            png_digest = components::byte_digest(
                encoded.data(), encoded.size());
            encoded_png = encoded;
        }
    }
    frame.Close();
    session.Close();
    pool.Close();

    return CaptureFrameResult{
        CaptureFrameMetadata{
            content_size.Width,
            content_size.Height,
            d3d.driver,
            fixture_owned,
            read_surface,
            false,
            false,
            foreground_before == GetForegroundWindow(),
            true,
            pixel_bytes_read,
            row_pitch,
            std::move(pixel_digest),
            std::move(pixel_format),
            png_encoded,
            png_bytes,
            std::move(png_digest),
            png_signature_valid,
            std::move(encoded_png),
            {},
            false,
        },
        std::nullopt,
    };
}

struct FrameReadbackResult {
    std::optional<components::RgbaImage> image;
    std::optional<BackendError> error;
};

FrameReadbackResult read_frame_pixels(
    const winrt::Windows::Graphics::Capture::
        Direct3D11CaptureFrame& frame,
    D3dResources& d3d) {
    constexpr std::uint32_t maximum_dimension = 4096U;
    constexpr std::uint64_t maximum_bytes =
        64ULL * 1024ULL * 1024ULL;
    const auto surface = frame.Surface();
    winrt::com_ptr<DxgiInterfaceAccess> access;
    auto* surface_unknown =
        reinterpret_cast<IUnknown*>(winrt::get_abi(surface));
    winrt::check_hresult(surface_unknown->QueryInterface(
        dxgi_interface_access_iid, access.put_void()));
    winrt::com_ptr<ID3D11Texture2D> source;
    winrt::check_hresult(access->GetInterface(
        __uuidof(ID3D11Texture2D), source.put_void()));

    D3D11_TEXTURE2D_DESC description{};
    source->GetDesc(&description);
    const std::uint64_t row_bytes =
        static_cast<std::uint64_t>(description.Width) * 4ULL;
    const std::uint64_t total_bytes =
        row_bytes * static_cast<std::uint64_t>(
                        description.Height);
    if (description.Width == 0U ||
        description.Height == 0U ||
        description.Width > maximum_dimension ||
        description.Height > maximum_dimension ||
        total_bytes > maximum_bytes ||
        description.Format != DXGI_FORMAT_B8G8R8A8_UNORM) {
        return FrameReadbackResult{
            std::nullopt,
            BackendError{
                "RESOURCE_LIMIT_EXCEEDED",
                "A recording probe frame violates the certified "
                "readback bounds or pixel format.",
            },
        };
    }

    D3D11_TEXTURE2D_DESC staging_description = description;
    staging_description.BindFlags = 0U;
    staging_description.MiscFlags = 0U;
    staging_description.CPUAccessFlags =
        D3D11_CPU_ACCESS_READ;
    staging_description.Usage = D3D11_USAGE_STAGING;
    winrt::com_ptr<ID3D11Texture2D> staging;
    winrt::check_hresult(d3d.device->CreateTexture2D(
        &staging_description, nullptr, staging.put()));
    d3d.context->CopyResource(staging.get(), source.get());

    D3D11_MAPPED_SUBRESOURCE mapped{};
    winrt::check_hresult(d3d.context->Map(
        staging.get(),
        0U,
        D3D11_MAP_READ,
        0U,
        &mapped));
    if (mapped.RowPitch < row_bytes ||
        mapped.pData == nullptr) {
        d3d.context->Unmap(staging.get(), 0U);
        return FrameReadbackResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "A recording probe frame has an invalid row pitch.",
            },
        };
    }
    const auto* bytes =
        static_cast<const std::uint8_t*>(mapped.pData);
    auto converted = components::convert_bgra_to_rgba(
        bytes,
        static_cast<std::size_t>(mapped.RowPitch),
        description.Width,
        description.Height);
    d3d.context->Unmap(staging.get(), 0U);
    if (!converted.image.has_value()) {
        return FrameReadbackResult{
            std::nullopt,
            BackendError{
                converted.error_code,
                converted.error_message,
            },
        };
    }
    return FrameReadbackResult{
        std::move(converted.image),
        std::nullopt,
    };
}

}  // namespace

namespace detail {

RecordingStreamResult capture_recording_frames_impl(
    const std::uint32_t frame_count,
    const std::uint32_t interval_ms,
    const std::uintptr_t native_window,
    const RecordingFrameConsumer& consumer) {
    if (frame_count < 2U || frame_count > 3000U ||
        interval_ms < 10U || interval_ms > 1000U) {
        return RecordingStreamResult{
            std::nullopt,
            BackendError{
                "INVALID_ARGUMENT",
                "The recording stream requires 2..3000 frames and a "
                "10..1000 ms interval.",
            },
        };
    }
    std::unique_ptr<FixtureWindow> fixture;
    HWND window =
        reinterpret_cast<HWND>(native_window);
    if (window == nullptr) {
        fixture = std::make_unique<FixtureWindow>();
        window = fixture->get();
    } else if (IsWindow(window) == FALSE ||
               IsWindowVisible(window) == FALSE ||
               IsIconic(window) != FALSE) {
        return RecordingStreamResult{
            std::nullopt,
            BackendError{
                "BACKGROUND_OPERATION_UNAVAILABLE",
                "The exact recording target is not visible and restored.",
            },
        };
    }
    const HWND foreground_before = GetForegroundWindow();
    winrt::init_apartment(winrt::apartment_type::multi_threaded);
    if (!winrt::Windows::Graphics::Capture::
            GraphicsCaptureSession::IsSupported()) {
        return RecordingStreamResult{
            std::nullopt,
            BackendError{
                "BACKGROUND_OPERATION_UNAVAILABLE",
                "Windows Graphics Capture is unsupported.",
            },
        };
    }
    const auto item = capture_item(window);
    const auto size = item.Size();
    auto d3d = create_d3d_resources();
    using namespace winrt::Windows::Graphics;
    auto pool =
        Capture::Direct3D11CaptureFramePool::CreateFreeThreaded(
            d3d.winrt_device,
            winrt::Windows::Graphics::DirectX::
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size);
    auto session = pool.CreateCaptureSession(item);
    session.IsCursorCaptureEnabled(false);
    session.StartCapture();
    std::optional<components::RgbaImage> last_frame;
    std::string last_digest;
    std::int32_t width = 0;
    std::int32_t height = 0;
    std::uint32_t captured_frames = 0U;
    const auto started =
        std::chrono::steady_clock::now();
    for (std::uint32_t index = 0U;
         index < frame_count;
         ++index) {
        if (index > 0U) {
            std::this_thread::sleep_until(
                started +
                std::chrono::milliseconds(
                    static_cast<std::uint64_t>(interval_ms) *
                    index));
        }
        if (fixture != nullptr) {
            fixture->set_phase(index + 1U);
        }
        const auto deadline =
            std::chrono::steady_clock::now() +
            std::chrono::milliseconds(
                fixture != nullptr ? 1500 : 500);
        bool accepted = false;
        while (std::chrono::steady_clock::now() < deadline) {
            pump_fixture_messages();
            auto frame = pool.TryGetNextFrame();
            if (frame == nullptr) {
                std::this_thread::sleep_for(
                    std::chrono::milliseconds(10));
                continue;
            }
            auto readback = read_frame_pixels(frame, d3d);
            frame.Close();
            if (readback.error.has_value()) {
                session.Close();
                pool.Close();
                return RecordingStreamResult{
                    std::nullopt, std::move(readback.error)};
            }
            const auto& image = *readback.image;
            if (last_frame.has_value() &&
                (image.width != last_frame->width ||
                 image.height != last_frame->height)) {
                session.Close();
                pool.Close();
                return RecordingStreamResult{
                    std::nullopt,
                    BackendError{
                        "CAPTURE_SIZE_CHANGED",
                        "The exact recording target changed size.",
                    },
                };
            }
            const auto digest = components::byte_digest(
                image.pixels.data(), image.pixels.size());
            if (!last_frame.has_value() ||
                digest != last_digest) {
                width = static_cast<std::int32_t>(image.width);
                height = static_cast<std::int32_t>(image.height);
                last_digest = digest;
                last_frame = *readback.image;
                ++captured_frames;
                if (!consumer(
                        std::move(*readback.image))) {
                    session.Close();
                    pool.Close();
                    return RecordingStreamResult{
                        std::nullopt,
                        BackendError{
                            "OPERATION_FAILED",
                            "The recording frame consumer failed.",
                        },
                    };
                }
                accepted = true;
                break;
            }
        }
        if (!accepted &&
            fixture == nullptr &&
            last_frame.has_value()) {
            if (!consumer(
                    components::RgbaImage(*last_frame))) {
                session.Close();
                pool.Close();
                return RecordingStreamResult{
                    std::nullopt,
                    BackendError{
                        "OPERATION_FAILED",
                        "The recording frame consumer failed.",
                    },
                };
            }
            accepted = true;
        }
        if (!accepted) {
            session.Close();
            pool.Close();
            return RecordingStreamResult{
                std::nullopt,
                BackendError{
                    "TIMEOUT",
                    "Windows Graphics Capture did not expose the "
                    "next changed fixture frame.",
                },
            };
        }
    }
    session.Close();
    pool.Close();
    return RecordingStreamResult{
        RecordingStreamMetadata{
            width,
            height,
            captured_frames,
            d3d.driver,
            fixture != nullptr,
            true,
            foreground_before == GetForegroundWindow(),
            true,
        },
        std::nullopt,
    };
}
}  // namespace detail
CaptureFrameResult capture_fixture_frame() {
    try {
        FixtureWindow fixture;
        return capture(fixture.get(), true, false, false);
    } catch (const winrt::hresult_error&) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The capture worker received a WGC or D3D failure.",
            },
        };
    }
}
CaptureFrameResult capture_fixture_surface_readback() {
    try {
        FixtureWindow fixture;
        return capture(fixture.get(), true, true, false);
    } catch (const winrt::hresult_error&) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The fixture surface readback received a WGC or D3D failure.",
            },
        };
    }
}
CaptureFrameResult capture_fixture_memory_png() {
    try {
        FixtureWindow fixture;
        return capture(fixture.get(), true, true, true);
    } catch (const winrt::hresult_error&) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The fixture PNG probe received a WGC, D3D, or WIC failure.",
            },
        };
    }
}
CaptureFrameResult capture_window_frame(
    const std::uintptr_t native_window) {
    try {
        return capture(
            reinterpret_cast<HWND>(native_window),
            false,
            false,
            false);
    } catch (const winrt::hresult_error& error) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                error.code() == E_ACCESSDENIED
                    ? "PERMISSION_DENIED"
                    : "OPERATION_FAILED",
                error.code() == E_ACCESSDENIED
                    ? "Windows denied capture of the exact target."
                    : "The capture worker received a WGC or D3D failure.",
            },
        };
    }
}
CaptureFrameResult capture_window_png_file(
    const std::uintptr_t native_window,
    const std::string& output_path,
    const bool overwrite) {
    const HWND foreground_before = GetForegroundWindow();
    auto result = capture(
        reinterpret_cast<HWND>(native_window),
        false,
        true,
        true);
    if (!result.metadata.has_value()) {
        return result;
    }
    auto& frame = *result.metadata;
    auto output = write_png_atomically(
        output_path, frame.encoded_png, overwrite);
    if (!output.output.has_value()) {
        return CaptureFrameResult{
            std::nullopt,
            BackendError{
                output.error_code,
                output.error_message,
            },
        };
    }
    frame.file_written = true;
    frame.pixels_persisted = true;
    frame.output_path = output.output->normalized_path;
    frame.replaced_existing =
        output.output->replaced_existing;
    frame.foreground_unchanged =
        frame.foreground_unchanged &&
        foreground_before == GetForegroundWindow();
    frame.encoded_png.clear();
    frame.encoded_png.shrink_to_fit();
    return result;
}
}  // namespace act::platform::windows
