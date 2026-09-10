#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/capture_preflight_backend.hpp"
#include "platform/windows/capture_worker_backend.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <string>

namespace act::modules {

class ScreenshotModule final {
public:
    [[nodiscard]] ModuleResult capture(
        const std::string& session_id,
        const std::string& output_path,
        bool confirmed,
        bool overwrite,
        std::uint32_t timeout_ms = 10000U) const;

private:
    platform::windows::DiscoveryBackend discovery_;
    platform::windows::CapturePreflightBackend preflight_;
    platform::windows::CaptureWorkerBackend worker_;
};

}  // namespace act::modules
