#pragma once

#include "modules/module_result.hpp"

#include <optional>
#include <string>

namespace act::systems {

class ReadOnlyDiagnosticSystem final {
public:
    [[nodiscard]] modules::ModuleResult doctor(
        const std::optional<std::string>& surface) const;
};

}  // namespace act::systems
