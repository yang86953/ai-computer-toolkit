#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/process_backend.hpp"
#include "platform/windows/standard_edit_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace act::modules {

class StandardEditModule final {
public:
    [[nodiscard]] ModuleResult status() const;
    [[nodiscard]] ModuleResult sessions(
        std::size_t maximum_items) const;
    [[nodiscard]] ModuleResult inspect(
        const std::string& session_id) const;
    [[nodiscard]] ModuleResult set_text(
        const std::string& session_id,
        const std::string& text,
        bool confirmed,
        std::uint32_t timeout_ms) const;

private:
    platform::windows::DiscoveryBackend foreground_;
    platform::windows::ProcessBackend processes_;
    platform::windows::StandardEditBackend backend_;
};

}  // namespace act::modules
