#include "systems/media_command_system.hpp"

#include <charconv>
#include <cstdint>
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

std::optional<std::string> session_id(
    const std::vector<std::string>& arguments) {
    constexpr std::string_view prefix = "sessionId=";
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] == "--target" &&
            index + 1U < arguments.size() &&
            arguments[index + 1U].starts_with(prefix)) {
            return arguments[index + 1U].substr(prefix.size());
        }
    }
    return std::nullopt;
}

std::optional<std::uint32_t> timeout_ms(
    const std::vector<std::string>& arguments) {
    std::uint32_t value = 5000U;
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] != "--timeout-ms") {
            continue;
        }
        if (index + 1U >= arguments.size()) {
            return std::nullopt;
        }
        const auto& raw = arguments[index + 1U];
        const auto parsed = std::from_chars(
            raw.data(), raw.data() + raw.size(), value);
        if (parsed.ec != std::errc{} ||
            parsed.ptr != raw.data() + raw.size() ||
            value == 0U || value > 30000U) {
            return std::nullopt;
        }
    }
    return value;
}

modules::ModuleResult invalid(
    const std::string& message) {
    return modules::ModuleResult{
        false,
        "INVALID_ARGUMENT",
        message,
        nullptr,
    };
}

}  // namespace

modules::ModuleResult MediaCommandSystem::run(
    const std::vector<std::string>& arguments) const {
    if (arguments.size() < 3U ||
        arguments[0] != "run" ||
        (arguments[1] != "media-session" &&
         arguments[1] != "media")) {
        return invalid(
            "Media control requires run media-session <operation>.");
    }
    const std::string& operation = arguments[2];
    if (!has_flag(arguments, "--confirm")) {
        return compatibility_.map_legacy_result(
            media_.control({}, operation, false, 5000U));
    }
    const auto target = session_id(arguments);
    const auto timeout = timeout_ms(arguments);
    if (!target.has_value() || !timeout.has_value()) {
        return invalid(
            "Media control requires --target sessionId=<opaque> and "
            "timeout-ms 1..30000.");
    }
    if (target->starts_with("media:")) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "Legacy media targets must use the migration launcher.",
            nullptr,
        };
    }
    return compatibility_.map_legacy_result(
        media_.control(
            *target, operation, true, *timeout));
}

}  // namespace act::systems
