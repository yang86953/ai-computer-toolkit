#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/foreground_input_backend.hpp"
#include "platform/windows/process_backend.hpp"

#include <string>

namespace act::modules {

class ForegroundInputModule final {
public:
    [[nodiscard]] ModuleResult press_key(
        const std::string& session_id,
        const std::string& key,
        bool confirmed,
        bool foreground_consent) const;

private:
    platform::windows::DiscoveryBackend discovery_;
    platform::windows::ProcessBackend processes_;
    platform::windows::ForegroundInputBackend backend_;
};

}  // namespace act::modules
