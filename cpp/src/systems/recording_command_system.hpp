#pragma once

#include "modules/module_result.hpp"
#include "modules/recording_compatibility_module.hpp"
#include "modules/recording_module.hpp"

#include <string>
#include <vector>

namespace act::systems {

class RecordingCommandSystem final {
public:
    [[nodiscard]] modules::ModuleResult run_desktop(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_app(
        const std::vector<std::string>& arguments) const;

private:
    modules::RecordingModule recording_;
    modules::RecordingCompatibilityModule compatibility_;
};

}  // namespace act::systems
