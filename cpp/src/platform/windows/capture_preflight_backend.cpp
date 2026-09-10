#include "platform/windows/capture_preflight_backend.hpp"

#include <windows.h>
#include <dwmapi.h>
#include <inspectable.h>
#include <roapi.h>
#include <winstring.h>

namespace act::platform::windows {
namespace {

constexpr wchar_t capture_session_class[] =
    L"Windows.Graphics.Capture.GraphicsCaptureSession";
constexpr wchar_t capture_item_class[] =
    L"Windows.Graphics.Capture.GraphicsCaptureItem";

constexpr GUID iid_capture_session_statics{
    0x2224a540,
    0x5974,
    0x49aa,
    {0xb2, 0x32, 0x08, 0x82, 0x53, 0x6f, 0x4c, 0xb5},
};
constexpr GUID iid_capture_item_interop{
    0x3628e81b,
    0x3cac,
    0x4c60,
    {0xb7, 0xf4, 0x23, 0xce, 0x0e, 0x0c, 0x33, 0x56},
};
constexpr GUID iid_capture_item{
    0x79c3f95b,
    0x31f7,
    0x4ec2,
    {0xa4, 0x64, 0x63, 0x2e, 0xf5, 0xd3, 0x07, 0x60},
};

struct WinRtSizeInt32 {
    std::int32_t width;
    std::int32_t height;
};

struct WinRtEventToken {
    std::int64_t value;
};

struct CaptureSessionStatics : IInspectable {
    virtual HRESULT STDMETHODCALLTYPE IsSupported(
        unsigned char* result) = 0;
};

struct CaptureItem : IInspectable {
    virtual HRESULT STDMETHODCALLTYPE get_DisplayName(
        HSTRING* value) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_Size(
        WinRtSizeInt32* value) = 0;
    virtual HRESULT STDMETHODCALLTYPE add_Closed(
        IUnknown* handler,
        WinRtEventToken* token) = 0;
    virtual HRESULT STDMETHODCALLTYPE remove_Closed(
        WinRtEventToken token) = 0;
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

struct WgcProbe {
    std::string runtime;
    std::string item_interop;
    bool item_nonzero_size;
    std::int32_t width;
    std::int32_t height;
};

class HString final {
public:
    explicit HString(const wchar_t* value) {
        const UINT32 length =
            static_cast<UINT32>(wcslen(value));
        status_ = WindowsCreateString(value, length, &value_);
    }
    ~HString() {
        if (value_ != nullptr) {
            WindowsDeleteString(value_);
        }
    }
    HString(const HString&) = delete;
    HString& operator=(const HString&) = delete;

    [[nodiscard]] HRESULT status() const {
        return status_;
    }
    [[nodiscard]] HSTRING get() const {
        return value_;
    }

private:
    HSTRING value_ = nullptr;
    HRESULT status_ = E_FAIL;
};

WgcProbe probe_wgc(const HWND window) {
    const HRESULT initialized = RoInitialize(RO_INIT_MULTITHREADED);
    const bool should_uninitialize = SUCCEEDED(initialized);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        return WgcProbe{
            "unavailable", "unavailable", false, 0, 0};
    }

    WgcProbe probe{
        "unavailable", "unavailable", false, 0, 0};
    HString session_class(capture_session_class);
    if (SUCCEEDED(session_class.status())) {
        CaptureSessionStatics* statics = nullptr;
        const HRESULT factory_status = RoGetActivationFactory(
            session_class.get(),
            iid_capture_session_statics,
            reinterpret_cast<void**>(&statics));
        if (SUCCEEDED(factory_status) && statics != nullptr) {
            unsigned char supported = 0U;
            const HRESULT support_status =
                statics->IsSupported(&supported);
            probe.runtime =
                SUCCEEDED(support_status)
                    ? (supported != 0U ? "supported" : "unsupported")
                    : "unavailable";
            statics->Release();
        }
    }

    if (probe.runtime == "supported") {
        HString item_class(capture_item_class);
        CaptureItemInterop* interop = nullptr;
        const HRESULT interop_status =
            SUCCEEDED(item_class.status())
                ? RoGetActivationFactory(
                      item_class.get(),
                      iid_capture_item_interop,
                      reinterpret_cast<void**>(&interop))
                : item_class.status();
        if (SUCCEEDED(interop_status) && interop != nullptr) {
            CaptureItem* item = nullptr;
            const HRESULT item_status = interop->CreateForWindow(
                window,
                iid_capture_item,
                reinterpret_cast<void**>(&item));
            if (SUCCEEDED(item_status) && item != nullptr) {
                WinRtSizeInt32 size{};
                const HRESULT size_status = item->get_Size(&size);
                probe.item_interop = "available";
                probe.item_nonzero_size =
                    SUCCEEDED(size_status) &&
                    size.width > 0 &&
                    size.height > 0;
                if (probe.item_nonzero_size) {
                    probe.width = size.width;
                    probe.height = size.height;
                }
                item->Release();
            } else {
                probe.item_interop =
                    item_status == E_ACCESSDENIED
                        ? "permission-blocked"
                        : "unavailable";
            }
            interop->Release();
        }
    } else if (probe.runtime == "unsupported") {
        probe.item_interop = "unsupported";
    }

    if (should_uninitialize) {
        RoUninitialize();
    }
    return probe;
}

std::string content_protection(const HWND window) {
    DWORD affinity = WDA_NONE;
    if (GetWindowDisplayAffinity(window, &affinity) == FALSE) {
        return "unknown";
    }
    if ((affinity & WDA_EXCLUDEFROMCAPTURE) != 0U) {
        return "excluded-from-capture";
    }
    if ((affinity & WDA_MONITOR) != 0U) {
        return "monitor-only";
    }
    return "none";
}

}  // namespace

CapturePreflightResult CapturePreflightBackend::inspect(
    const WindowRecord& window) const {
    const HWND native =
        reinterpret_cast<HWND>(window.native_window);
    if (IsWindow(native) == FALSE) {
        return CapturePreflightResult{
            std::nullopt,
            BackendError{
                "STALE_SESSION",
                "The exact window no longer exists during capture preflight.",
            },
        };
    }

    const bool visible = IsWindowVisible(native) != FALSE;
    const bool minimized = IsIconic(native) != FALSE;
    DWORD cloaked_value = 0U;
    const HRESULT cloak_status = DwmGetWindowAttribute(
        native,
        DWMWA_CLOAKED,
        &cloaked_value,
        sizeof(cloaked_value));
    const bool cloaked =
        SUCCEEDED(cloak_status) && cloaked_value != 0U;

    RECT rectangle{};
    const bool has_rectangle =
        GetWindowRect(native, &rectangle) != FALSE;
    const bool nonzero_extent =
        has_rectangle &&
        rectangle.right > rectangle.left &&
        rectangle.bottom > rectangle.top;

    BOOL composition = FALSE;
    const bool composition_enabled =
        SUCCEEDED(DwmIsCompositionEnabled(&composition)) &&
        composition != FALSE;
    const std::string protection = content_protection(native);
    const WgcProbe wgc = probe_wgc(native);

    std::string eligibility = "eligible-for-certified-capture-route";
    if (!visible) {
        eligibility = "window-not-visible";
    } else if (minimized) {
        eligibility = "window-minimized";
    } else if (cloaked) {
        eligibility = "window-cloaked";
    } else if (!nonzero_extent) {
        eligibility = "window-has-no-visible-extent";
    } else if (protection == "excluded-from-capture") {
        eligibility = "capture-excluded-by-window";
    } else if (!composition_enabled) {
        eligibility = "desktop-composition-unavailable";
    } else if (protection == "unknown") {
        eligibility = "capture-protection-unknown";
    } else if (wgc.runtime == "unsupported") {
        eligibility = "wgc-runtime-unsupported";
    } else if (wgc.runtime != "supported") {
        eligibility = "wgc-runtime-unavailable";
    } else if (wgc.item_interop == "permission-blocked") {
        eligibility = "wgc-item-permission-blocked";
    } else if (
        wgc.item_interop != "available" ||
        !wgc.item_nonzero_size) {
        eligibility = "wgc-item-unavailable";
    }

    return CapturePreflightResult{
        CapturePreflight{
            visible,
            minimized,
            cloaked,
            nonzero_extent,
            composition_enabled,
            protection,
            wgc.runtime,
            wgc.item_interop,
            wgc.item_nonzero_size,
            wgc.width,
            wgc.height,
            eligibility,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
