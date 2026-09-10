#pragma once

#include "modules/module_result.hpp"

namespace act::modules {

class ApplicationLaunchCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_desktop_result(
        ModuleResult provider_result) const;
};

}  // namespace act::modules
