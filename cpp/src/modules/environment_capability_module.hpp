#pragma once

#include "modules/module_result.hpp"

#include <string>

namespace act::modules {

class EnvironmentCapabilityModule final {
public:
    [[nodiscard]] ModuleResult status(
        const std::string& surface) const;
};

}  // namespace act::modules
