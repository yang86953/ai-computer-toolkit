#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <optional>
#include <string>

namespace act::platform::windows {

struct CapturePreflight {
    bool visible;
    bool minimized;
    bool cloaked;
    bool nonzero_extent;
    bool desktop_composition_enabled;
    std::string content_protection;
    std::string wgc_runtime;
    std::string wgc_item_interop;
    bool capture_item_nonzero_size;
    std::int32_t capture_width;
    std::int32_t capture_height;
    std::string eligibility;
};

struct CapturePreflightResult {
    std::optional<CapturePreflight> preflight;
    std::optional<BackendError> error;
};

class CapturePreflightBackend final {
public:
    [[nodiscard]] CapturePreflightResult inspect(
        const WindowRecord& window) const;
};

}  // namespace act::platform::windows
