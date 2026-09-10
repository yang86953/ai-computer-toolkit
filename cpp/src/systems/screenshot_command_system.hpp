#pragma once

#include "modules/module_result.hpp"
#include "modules/screenshot_compatibility_module.hpp"
#include "modules/screenshot_module.hpp"

#include <string>
#include <vector>

namespace act::systems {

class ScreenshotCommandSystem final {
public:
    [[nodiscard]] modules::ModuleResult run_desktop(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_app(
        const std::vector<std::string>& arguments) const;

private:
    modules::ScreenshotModule screenshot_;
    modules::ScreenshotCompatibilityModule compatibility_;
};

}  // namespace act::systems
