#include "systems/recording_command_system.hpp"

#include "components/json_input.hpp"

#include <array>
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

modules::ModuleResult invalid(
    const std::string& message) {
    return modules::ModuleResult{
        false,
        "INVALID_ARGUMENT",
        message,
        nullptr,
    };
}

std::optional<components::Json> recording_input(
    const std::vector<std::string>& arguments) {
    components::Json::Object fields;
    constexpr std::array string_fields{
        "path", "analysisDir"};
    constexpr std::array scalar_fields{
        "durationMs",
        "fps",
        "maxWidth",
        "crf",
        "maxKeyframes",
        "changeThreshold",
        "timeoutMs",
        "overwrite",
    };
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] != "--arg" ||
            index + 1U >= arguments.size()) {
            continue;
        }
        const auto& raw = arguments[index + 1U];
        const auto separator = raw.find('=');
        if (separator == std::string::npos) {
            return std::nullopt;
        }
        const std::string_view name(
            raw.data(), separator);
        bool known = false;
        for (const auto* candidate : string_fields) {
            known = known || name == candidate;
        }
        for (const auto* candidate : scalar_fields) {
            known = known || name == candidate;
        }
        if (!known) {
            return std::nullopt;
        }
    }
    for (const auto* name : string_fields) {
        const auto value =
            named_value(arguments, "--arg", name);
        if (value.has_value()) {
            fields.emplace_back(name, *value);
        }
    }
    for (const auto* name : scalar_fields) {
        const auto value =
            named_value(arguments, "--arg", name);
        if (!value.has_value()) {
            continue;
        }
        std::string error;
        auto scalar =
            components::Json::parse(*value, error);
        if (!scalar.has_value() ||
            (scalar->integer_value() == nullptr &&
             scalar->double_value() == nullptr &&
             scalar->bool_value() == nullptr)) {
            return std::nullopt;
        }
        fields.emplace_back(name, std::move(*scalar));
    }
    return components::Json(std::move(fields));
}

}  // namespace

modules::ModuleResult RecordingCommandSystem::run_desktop(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return compatibility_.map_desktop_result(
            recording_.record({}, nullptr, false));
    }
    const auto session_id =
        named_value(arguments, "--target", "sessionId");
    if (!session_id.has_value()) {
        return invalid(
            "desktop.record requires an exact sessionId target.");
    }
    if (session_id->starts_with("window:") ||
        session_id->starts_with("s1:")) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "Legacy recording targets must use the migration launcher.",
            nullptr,
        };
    }
    if (!session_id->starts_with("s2:w:")) {
        return modules::ModuleResult{
            false,
            "STALE_SESSION",
            "The exact opaque recording target is unavailable.",
            nullptr,
        };
    }
    auto input = recording_input(arguments);
    if (!input.has_value()) {
        return invalid(
            "desktop.record contains an invalid numeric or boolean arg.");
    }
    return compatibility_.map_desktop_result(
        recording_.record(*session_id, *input, true));
}

modules::ModuleResult RecordingCommandSystem::run_app(
    const std::vector<std::string>& arguments) const {
    const auto input_source =
        option_value(arguments, "--input");
    if (!input_source.has_value()) {
        if (!has_flag(arguments, "--confirm")) {
            return compatibility_.map_app_facade_result(
                recording_.record({}, nullptr, false));
        }
        return invalid("app.record requires --input <file|->.");
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
    const auto* target = input.value->find("target");
    const auto* args = input.value->find("args");
    const auto* confirmed_value =
        input.value->find("confirmed");
    const bool confirmed =
        has_flag(arguments, "--confirm") ||
        (confirmed_value != nullptr &&
         confirmed_value->bool_value() != nullptr &&
         *confirmed_value->bool_value());
    if (!confirmed) {
        return compatibility_.map_app_facade_result(
            recording_.record({}, nullptr, false));
    }
    const auto* session =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* capability =
        args == nullptr ? nullptr : args->find("capability");
    const auto* provider_input =
        args == nullptr ? nullptr : args->find("input");
    if (target == nullptr ||
        target->object_items() == nullptr ||
        args == nullptr ||
        args->object_items() == nullptr ||
        session == nullptr ||
        session->string_value() == nullptr ||
        capability == nullptr ||
        capability->string_value() == nullptr ||
        provider_input == nullptr ||
        provider_input->object_items() == nullptr) {
        return invalid(
            "app.record requires target.sessionId, capability, and input.");
    }
    if (*capability->string_value() !=
        "window.record@1") {
        return modules::ModuleResult{
            false,
            "CAPABILITY_UNSUPPORTED",
            "The requested app capability is not the recording route.",
            nullptr,
        };
    }
    const std::string& session_id =
        *session->string_value();
    if (session_id.starts_with("s1:") ||
        session_id.starts_with("window:")) {
        return modules::ModuleResult{
            false,
            "TARGET_ID_MIGRATION_REQUIRED",
            "Legacy facade recording targets require the launcher.",
            nullptr,
        };
    }
    return compatibility_.map_app_facade_result(
        recording_.record(
            session_id, *provider_input, true));
}

}  // namespace act::systems
