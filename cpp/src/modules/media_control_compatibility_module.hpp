#pragma once

#include "modules/module_result.hpp"

namespace act::modules {

class MediaControlCompatibilityModule final {
public:
    [[nodiscard]] ModuleResult map_legacy_result(
        ModuleResult provider_result) const;
};

}  // namespace act::modules
