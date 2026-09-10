#pragma once

#include "modules/media_control_compatibility_module.hpp"
#include "modules/media_session_module.hpp"
#include "modules/module_result.hpp"

#include <string>
#include <vector>

namespace act::systems {

class MediaCommandSystem final {
public:
    [[nodiscard]] modules::ModuleResult run(
        const std::vector<std::string>& arguments) const;

private:
    modules::MediaSessionModule media_;
    modules::MediaControlCompatibilityModule compatibility_;
};

}  // namespace act::systems
