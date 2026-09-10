#pragma once

#include "modules/application_launch_compatibility_module.hpp"
#include "modules/application_launch_module.hpp"
#include "modules/module_result.hpp"

#include <string>
#include <vector>

namespace act::systems {

class ApplicationLaunchCommandSystem final {
public:
    [[nodiscard]] modules::ModuleResult run_desktop(
        const std::vector<std::string>& arguments) const;

private:
    modules::ApplicationLaunchModule launch_;
    modules::ApplicationLaunchCompatibilityModule compatibility_;
};

}  // namespace act::systems
