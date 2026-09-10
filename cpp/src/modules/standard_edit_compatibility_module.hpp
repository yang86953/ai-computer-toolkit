#pragma once

#include <string>

#include "modules/module_result.hpp"

namespace act::modules {

class StandardEditCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_run_result(
        ModuleResult provider_result) const;
    [[nodiscard]] ModuleResult map_app_result(
        ModuleResult provider_result,
        const std::string& target_id) const;
    [[nodiscard]] ModuleResult map_desktop_type_text_result(
        ModuleResult provider_result) const;
};

}  // namespace act::modules
