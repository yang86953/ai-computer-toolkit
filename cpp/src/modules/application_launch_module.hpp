#pragma once

#include "modules/application_discovery_module.hpp"
#include "modules/module_result.hpp"
#include "platform/windows/application_launch_backend.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <string>

namespace act::modules {

class ApplicationLaunchModule final {
public:
    [[nodiscard]] ModuleResult launch(
        const std::string& session_id,
        bool confirmed) const;

private:
    ApplicationDiscoveryModule discovery_;
    platform::windows::DiscoveryBackend foreground_;
    platform::windows::ApplicationLaunchBackend backend_;
};

}  // namespace act::modules
