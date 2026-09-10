#pragma once

#include "modules/application_discovery_module.hpp"
#include "modules/module_result.hpp"

#include <string>

namespace act::modules {

enum class CapabilityAvailability {
    available,
    permission_blocked,
    unavailable,
};

class CapabilityAssessmentModule final {
public:
    [[nodiscard]] ModuleResult assess(
        const std::string& capability,
        const std::string& target_id,
        TargetKind target_kind,
        CapabilityAvailability availability =
            CapabilityAvailability::available) const;
};

}  // namespace act::modules
