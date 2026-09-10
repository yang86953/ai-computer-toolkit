#pragma once

#include "modules/module_result.hpp"

namespace act::modules {

class WindowCloseCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_app_result(
        ModuleResult provider_result) const;
};

}  // namespace act::modules
