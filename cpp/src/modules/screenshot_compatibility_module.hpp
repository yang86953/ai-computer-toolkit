#pragma once

#include "modules/module_result.hpp"

namespace act::modules {

class ScreenshotCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_desktop_result(
        ModuleResult provider_result) const;
    [[nodiscard]] ModuleResult map_app_facade_result(
        ModuleResult provider_result) const;
};

}  // namespace act::modules
