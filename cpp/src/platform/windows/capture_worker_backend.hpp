#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <optional>
#include <string>

namespace act::platform::windows {

struct CaptureFrameProbe {
    std::int64_t width;
    std::int64_t height;
    std::string device_driver;
    bool frame_surface_accessed;
    bool pixels_persisted;
    bool file_written;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
};

struct CaptureFrameProbeResult {
    std::optional<CaptureFrameProbe> frame;
    std::optional<BackendError> error;
};

struct ScreenshotCapture {
    std::int64_t width;
    std::int64_t height;
    std::int64_t bytes;
    std::string device_driver;
    std::string pixel_digest;
    std::string output_path;
    bool replaced_existing;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
};

struct ScreenshotCaptureResult {
    std::optional<ScreenshotCapture> screenshot;
    std::optional<BackendError> error;
};

class CaptureWorkerBackend final {
public:
    [[nodiscard]] CaptureFrameProbeResult probe_frame(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] ScreenshotCaptureResult capture_screenshot(
        const std::string& session_id,
        const std::string& output_path,
        bool overwrite,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
