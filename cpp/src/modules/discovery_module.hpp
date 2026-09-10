#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/capture_preflight_backend.hpp"
#include "platform/windows/capture_worker_backend.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/observation_worker_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace act::modules {

class DiscoveryModule final {
public:
    [[nodiscard]] ModuleResult status() const;
    [[nodiscard]] ModuleResult window_status() const;
    [[nodiscard]] ModuleResult sessions(std::size_t maximum_items) const;
    [[nodiscard]] ModuleResult inspect_window(
        const std::string& session_id) const;
    [[nodiscard]] ModuleResult inspect(
        const std::string& session_id,
        std::uint32_t timeout_ms = 5000U) const;
    [[nodiscard]] ModuleResult inspect_tree(
        const std::string& session_id,
        std::size_t maximum_depth,
        std::size_t maximum_items,
        const std::string& view,
        std::uint32_t timeout_ms = 5000U) const;
    [[nodiscard]] ModuleResult capture_preflight(
        const std::string& session_id) const;
    [[nodiscard]] ModuleResult capture_frame_probe(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;

private:
    platform::windows::DiscoveryBackend backend_;
    platform::windows::ObservationWorkerBackend observation_worker_;
    platform::windows::CapturePreflightBackend capture_preflight_;
    platform::windows::CaptureWorkerBackend capture_worker_;
};

}  // namespace act::modules
