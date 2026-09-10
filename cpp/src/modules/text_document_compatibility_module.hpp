#pragma once

#include "modules/module_result.hpp"

#include <string>

namespace act::modules {

class TextDocumentCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_legacy(
        ModuleResult provider_result) const;
    [[nodiscard]] ModuleResult map_app(
        ModuleResult provider_result,
        const std::string& target_id) const;
};

}  // namespace act::modules
