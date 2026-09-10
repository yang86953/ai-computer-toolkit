#include "systems/application_launch_command_system.hpp"

#include <optional>
#include <string_view>

namespace act::systems {
namespace {

bool has_flag(
    const std::vector<std::string>& arguments,
    const std::string_view flag) {
    for (const auto& argument : arguments) {
        if (argument == flag) {
            return true;
        }
    }
    return false;
}

std::optional<std::string> target(
    const std::vector<std::string>& arguments) {
    constexpr std::string_view prefix = "sessionId=";
    for (std::size_t index = 3U;
         index + 1U < arguments.size();
         ++index) {
        if (arguments[index] == "--target" &&
            arguments[index + 1U].starts_with(prefix)) {
            return arguments[index + 1U].substr(prefix.size());
        }
    }
    return std::nullopt;
}

}  // namespace

modules::ModuleResult
ApplicationLaunchCommandSystem::run_desktop(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return compatibility_.map_desktop_result(
            launch_.launch({}, false));
    }
    const auto session_id = target(arguments);
    if (!session_id.has_value()) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "C++ application launch requires an exact discovered "
            "application target; legacy path launch uses compatibility.",
            nullptr,
        };
    }
    return compatibility_.map_desktop_result(
        launch_.launch(*session_id, true));
}

}  // namespace act::systems
