#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/capture_preflight_backend.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/recording_worker_backend.hpp"

#include <string>

namespace act::modules {

class RecordingModule final {
public:
    [[nodiscard]] ModuleResult record(
        const std::string& session_id,
        const components::Json& input,
        bool confirmed) const;

private:
    platform::windows::DiscoveryBackend discovery_;
    platform::windows::CapturePreflightBackend preflight_;
    platform::windows::RecordingWorkerBackend worker_;
};

}  // namespace act::modules
