#pragma once

#include "modules/browser_screenshot_module.hpp"
#include "modules/module_result.hpp"

#include <string>
#include <vector>

namespace act::systems {

class BrowserCommandSystem final {
public:
    [[nodiscard]] modules::ModuleResult status() const;
    [[nodiscard]] modules::ModuleResult run(
        const std::vector<std::string>& arguments) const;

private:
    modules::BrowserScreenshotModule screenshots_;
};

}  // namespace act::systems
