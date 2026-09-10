#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <optional>
#include <string>

namespace act::platform::windows {

struct BrowserScreenshot {
    std::string output_path;
    std::int64_t bytes;
    std::int64_t width;
    std::int64_t height;
    bool replaced_existing;
    bool foreground_unchanged;
};

struct BrowserScreenshotResult {
    std::optional<BrowserScreenshot> screenshot;
    std::optional<BackendError> error;
};

class BrowserWorkerBackend final {
public:
    [[nodiscard]] bool runtime_available() const;
    [[nodiscard]] BrowserScreenshotResult capture(
        const std::string& url,
        const std::string& output_path,
        bool overwrite,
        std::uint32_t width,
        std::uint32_t height,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
