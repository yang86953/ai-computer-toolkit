#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/browser_worker_backend.hpp"

#include <cstdint>
#include <string>

namespace act::modules {

class BrowserScreenshotModule final {
public:
    [[nodiscard]] ModuleResult status() const;
    [[nodiscard]] ModuleResult capture(
        const std::string& url,
        const std::string& output_path,
        bool confirmed,
        bool overwrite,
        std::uint32_t width = 1280U,
        std::uint32_t height = 720U,
        std::uint32_t timeout_ms = 30000U) const;

private:
    platform::windows::BrowserWorkerBackend backend_;
};

}  // namespace act::modules
