#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/window_close_backend.hpp"

#include <cstdint>
#include <string>

namespace act::modules {

class WindowCloseModule final {
public:
    [[nodiscard]] ModuleResult close(
        const std::string& session_id,
        bool confirmed,
        std::uint32_t timeout_ms) const;

private:
    platform::windows::DiscoveryBackend discovery_;
    platform::windows::WindowCloseBackend backend_;
};

}  // namespace act::modules
