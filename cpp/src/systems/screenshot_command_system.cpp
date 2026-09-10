#include "systems/screenshot_command_system.hpp"

#include "components/json_input.hpp"

#include <charconv>
#include <cstdint>
#include <limits>
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

std::optional<std::string> named_value(
    const std::vector<std::string>& arguments,
    const std::string_view flag,
    const std::string_view name) {
    const std::string prefix =
        std::string(name) + '=';
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] == flag &&
            index + 1U < arguments.size() &&
            arguments[index + 1U].starts_with(prefix)) {
            return arguments[index + 1U].substr(
                prefix.size());
        }
    }
    return std::nullopt;
}

std::optional<std::string> option_value(
    const std::vector<std::string>& arguments,
    const std::string_view flag) {
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] == flag &&
            index + 1U < arguments.size()) {
            return arguments[index + 1U];
        }
    }
    return std::nullopt;
}

std::optional<std::uint32_t> timeout_value(
    const std::vector<std::string>& arguments) {
    const auto raw =
        named_value(arguments, "--arg", "timeoutMs");
    if (!raw.has_value()) {
        return 5000U;
    }
    std::uint32_t value = 0U;
    const auto result = std::from_chars(
        raw->data(), raw->data() + raw->size(), value);
    if (result.ec != std::errc{} ||
        result.ptr != raw->data() + raw->size()) {
        return std::nullopt;
    }
    return value;
}

std::optional<bool> overwrite_value(
    const std::vector<std::string>& arguments) {
    const auto raw =
        named_value(arguments, "--arg", "overwrite");
    if (!raw.has_value() || *raw == "false") {
        return false;
    }
    if (*raw == "true") {
        return true;
    }
    return std::nullopt;
}

}  // namespace

modules::ModuleResult
ScreenshotCommandSystem::run_desktop(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return compatibility_.map_desktop_result(
            screenshot_.capture({}, {}, false, false));
    }
    const auto session_id =
        named_value(arguments, "--target", "sessionId");
    if (!session_id.has_value()) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "desktop.screenshot requires an exact sessionId target.",
            nullptr,
        };
    }
    if (session_id->starts_with("window:")) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "Legacy native window targets must use the migration launcher; "
            "the C++ core only accepts opaque s2:w targets.",
            nullptr,
        };
    }
    if (!session_id->starts_with("s2:w:")) {
        return modules::ModuleResult{
            false,
            "STALE_SESSION",
            "The exact opaque window target is unavailable.",
            nullptr,
        };
    }
    const auto path =
        named_value(arguments, "--arg", "path");
    const auto timeout = timeout_value(arguments);
    const auto overwrite = overwrite_value(arguments);
    if (!path.has_value() ||
        !timeout.has_value() ||
        !overwrite.has_value()) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "desktop.screenshot requires path, boolean overwrite, and "
            "integer timeoutMs arguments.",
            nullptr,
        };
    }
    return compatibility_.map_desktop_result(
        screenshot_.capture(
            *session_id,
            *path,
            true,
            *overwrite,
            *timeout));
}

modules::ModuleResult ScreenshotCommandSystem::run_app(
    const std::vector<std::string>& arguments) const {
    const auto input_source =
        option_value(arguments, "--input");
    if (!input_source.has_value()) {
        if (!has_flag(arguments, "--confirm")) {
            return compatibility_.map_app_facade_result(
                screenshot_.capture({}, {}, false, false));
        }
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "app.screenshot requires --input <file|->.",
            nullptr,
        };
    }
    auto input =
        components::read_json_input(*input_source);
    if (!input.value.has_value()) {
        return modules::ModuleResult{
            false,
            std::move(input.error_code),
            std::move(input.error_message),
            nullptr,
        };
    }
    const auto* root = input.value->object_items();
    const auto* target = input.value->find("target");
    const auto* args = input.value->find("args");
    const auto* target_object =
        target == nullptr ? nullptr : target->object_items();
    const auto* args_object =
        args == nullptr ? nullptr : args->object_items();
    if (root == nullptr ||
        target_object == nullptr ||
        args_object == nullptr) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "app.screenshot input requires target and args objects.",
            nullptr,
        };
    }
    const auto* confirmed_value =
        input.value->find("confirmed");
    const bool confirmed =
        has_flag(arguments, "--confirm") ||
        (confirmed_value != nullptr &&
         confirmed_value->bool_value() != nullptr &&
         *confirmed_value->bool_value());
    if (!confirmed) {
        return compatibility_.map_app_facade_result(
            screenshot_.capture({}, {}, false, false));
    }
    const auto* session_value = target->find("sessionId");
    const auto* capability_value = args->find("capability");
    const auto* provider_input = args->find("input");
    const auto* provider_object =
        provider_input == nullptr
            ? nullptr
            : provider_input->object_items();
    if (session_value == nullptr ||
        session_value->string_value() == nullptr ||
        capability_value == nullptr ||
        capability_value->string_value() == nullptr ||
        provider_object == nullptr) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "app.screenshot requires exact sessionId, capability, and "
            "args.input.",
            nullptr,
        };
    }
    if (*capability_value->string_value() !=
        "window.screenshot@1") {
        return modules::ModuleResult{
            false,
            "CAPABILITY_UNSUPPORTED",
            "The requested app capability is not the screenshot route.",
            nullptr,
        };
    }
    const std::string& session_id =
        *session_value->string_value();
    if (session_id.starts_with("s1:") ||
        session_id.starts_with("window:")) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "Legacy facade targets must use the migration launcher.",
            nullptr,
        };
    }
    const auto* path_value = provider_input->find("path");
    const auto* overwrite =
        provider_input->find("overwrite");
    const auto* timeout =
        provider_input->find("timeoutMs");
    if (path_value == nullptr ||
        path_value->string_value() == nullptr ||
        (overwrite != nullptr &&
         overwrite->bool_value() == nullptr) ||
        (timeout != nullptr &&
         timeout->integer_value() == nullptr)) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Screenshot input requires path, optional boolean overwrite, "
            "and optional integer timeoutMs.",
            nullptr,
        };
    }
    const std::int64_t timeout_raw =
        timeout == nullptr
            ? 5000
            : *timeout->integer_value();
    if (timeout_raw < 0 ||
        timeout_raw >
            std::numeric_limits<std::uint32_t>::max()) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Screenshot timeoutMs is outside the integer range.",
            nullptr,
        };
    }
    return compatibility_.map_app_facade_result(
        screenshot_.capture(
            session_id,
            *path_value->string_value(),
            true,
            overwrite != nullptr &&
                *overwrite->bool_value(),
            static_cast<std::uint32_t>(timeout_raw)));
}

}  // namespace act::systems
