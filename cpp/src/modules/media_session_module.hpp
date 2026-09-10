#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/media_worker_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace act::modules {

class MediaSessionModule final {
public:
    [[nodiscard]] ModuleResult status(
        std::uint32_t timeout_ms) const;
    [[nodiscard]] ModuleResult sessions(
        std::size_t maximum_items,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] ModuleResult inspect(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] ModuleResult control(
        const std::string& session_id,
        const std::string& operation,
        bool confirmed,
        std::uint32_t timeout_ms) const;

private:
    platform::windows::DiscoveryBackend foreground_;
    platform::windows::MediaWorkerBackend worker_;
};

}  // namespace act::modules
