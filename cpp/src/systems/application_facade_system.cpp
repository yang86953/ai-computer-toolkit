#include "systems/application_facade_system.hpp"

#include "components/json_input.hpp"

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
    const std::string prefix = std::string(name) + '=';
    for (std::size_t index = 3U;
         index < arguments.size();
         ++index) {
        if (arguments[index] == flag &&
            index + 1U < arguments.size() &&
            arguments[index + 1U].starts_with(prefix)) {
            return arguments[index + 1U].substr(prefix.size());
        }
    }
    return std::nullopt;
}

modules::ModuleResult invalid(const char* message) {
    return modules::ModuleResult{
        false, "INVALID_ARGUMENT", message, nullptr};
}

std::optional<std::uint32_t> timeout_option(
    const std::vector<std::string>& arguments) {
    const auto raw = option_value(
        arguments, "--timeout-ms");
    if (!raw.has_value()) {
        return 2000U;
    }
    std::uint32_t value = 0U;
    const auto parsed = std::from_chars(
        raw->data(), raw->data() + raw->size(), value);
    if (parsed.ec != std::errc{} ||
        parsed.ptr != raw->data() + raw->size() ||
        value == 0U || value > 30000U) {
        return std::nullopt;
    }
    return value;
}

}  // namespace

modules::ModuleResult ApplicationFacadeSystem::status() const {
    const auto structured = structured_images_.status();
    const auto* structured_installed = structured.ok
        ? structured.data.find("installed")
        : nullptr;
    const auto* structured_connected = structured.ok
        ? structured.data.find("connected")
        : nullptr;
    return modules::ModuleResult{
        true,
        {},
        {},
        components::object({
            {"platform", "windows"},
            {"mainImplementation", "cpp"},
            {"compatibilityEntrypoint", "rust-cargo"},
            {"canDiscoverApplications", true},
            {"canReadAccessibilityRoot", true},
            {"accessibilityObservationIsolation",
             "job-bounded-worker"},
            {"accessibilityObservationTimeoutMs", 5000},
            {"accessibilityObservationCancellable", true},
            {"textDocumentCreatorAvailable",
             text_documents_.available()},
            {"writesEnabled", true},
            {"writableCapabilities",
             components::array({
                 "text.document.create@1",
                 "ui.text.input@1",
                 "window.close@1",
             })},
            {"allWritesRequireConfirmation", true},
            {"foregroundConsentRequiredForTextCreate", false},
            {"structuredImageProvider",
             components::object({
                 {"observationAvailable", structured.ok},
                 {"installed",
                  structured_installed != nullptr &&
                      structured_installed->bool_value() != nullptr &&
                      *structured_installed->bool_value()},
                 {"connected",
                  structured_connected != nullptr &&
                      structured_connected->bool_value() != nullptr &&
                      *structured_connected->bool_value()},
                 {"writesCertified", false},
                 {"observationIsolation", "job-bounded-worker"},
             })},
            {"supportLevel", "L2-confirmed-background"},
        }),
    };
}

modules::ModuleResult ApplicationFacadeSystem::sessions(
    const std::size_t maximum_items) const {
    auto windows = discovery_.sessions(4096U);
    auto controls = standard_edits_.sessions(4096U);
    auto structured = structured_images_.sessions();
    if (!windows.ok || !controls.ok) {
        return !windows.ok ? windows : controls;
    }
    const auto* window_sessions =
        windows.data.find("sessions");
    const auto* control_sessions =
        controls.data.find("sessions");
    const auto* window_total =
        windows.data.find("total");
    const auto* control_total =
        controls.data.find("total");
    if (window_sessions == nullptr ||
        window_sessions->array_items() == nullptr ||
        control_sessions == nullptr ||
        control_sessions->array_items() == nullptr ||
        window_total == nullptr ||
        window_total->integer_value() == nullptr ||
        control_total == nullptr ||
        control_total->integer_value() == nullptr) {
        return modules::ModuleResult{
            false,
            "OPERATION_FAILED",
            "Application discovery returned an invalid session directory.",
            nullptr,
        };
    }
    components::Json::Array sessions;
    components::Json::Array warnings;
    sessions.reserve(maximum_items);
    if (maximum_items > 0U) {
        sessions.push_back(
            text_documents_.session_descriptor());
    }
    std::int64_t structured_total = 0;
    bool foreground_unchanged = true;
    const components::Json::Array* structured_sessions = nullptr;
    if (structured.ok) {
        const auto* items = structured.data.find("sessions");
        const auto* count = structured.data.find("count");
        const auto* foreground =
            structured.data.find("foregroundUnchanged");
        if (items != nullptr &&
            items->array_items() != nullptr &&
            count != nullptr &&
            count->integer_value() != nullptr &&
            foreground != nullptr &&
            foreground->bool_value() != nullptr) {
            structured_sessions = items->array_items();
            structured_total = *count->integer_value();
            foreground_unchanged =
                *foreground->bool_value();
        } else {
            warnings.push_back(components::object({
                {"code", "OPERATION_FAILED"},
                {"message",
                 "Structured image provider returned an invalid directory."},
            }));
        }
    } else {
        warnings.push_back(components::object({
            {"code", structured.error_code},
            {"message", structured.error_message},
        }));
    }
    for (const auto* source : {
             structured_sessions,
             control_sessions->array_items(),
             window_sessions->array_items()}) {
        if (source == nullptr) {
            continue;
        }
        for (const auto& item : *source) {
            if (sessions.size() >= maximum_items) {
                break;
            }
            sessions.push_back(item);
        }
    }
    const std::int64_t total =
        *window_total->integer_value() +
        *control_total->integer_value() +
        structured_total + 1;
    return modules::ModuleResult{
        true,
        {},
        {},
        components::object({
            {"surface", "app"},
            {"capability", "application.session.discover@1"},
            {"readOnly", true},
            {"foregroundUnchanged", foreground_unchanged},
            {"targetIdentity", "opaque-versioned-session-id"},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"total", total},
            {"truncated",
             total > static_cast<std::int64_t>(sessions.size())},
            {"sessions", components::Json(std::move(sessions))},
            {"warnings", components::Json(std::move(warnings))},
        }),
    };
}

modules::ModuleResult ApplicationFacadeSystem::inspect(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    if (text_documents_.owns_session(session_id)) {
        return text_documents_.inspect(session_id);
    }
    if (session_id.starts_with("s2:c:")) {
        return standard_edits_.inspect(session_id);
    }
    if (session_id.starts_with("s2:d:") ||
        session_id.starts_with("s2:a:")) {
        auto structured = structured_images_.inspect(session_id);
        if (structured.ok ||
            structured.error_code != "TARGET_NOT_FOUND") {
            return structured;
        }
    }
    return discovery_.inspect(session_id, timeout_ms);
}

bool ApplicationFacadeSystem::owns_text_document_session(
    const std::string& session_id) const {
    return text_documents_.owns_session(session_id);
}

modules::ModuleResult
ApplicationFacadeSystem::assess_standard_edit(
    const std::string& capability,
    const std::string& session_id) const {
    const auto inspected =
        standard_edits_.inspect(session_id);
    if (!inspected.ok) {
        return inspected;
    }
    const auto* control =
        inspected.data.find("control");
    const auto* static_assessment =
        control == nullptr
            ? nullptr
            : control->find("assessment");
    const auto* decision =
        static_assessment == nullptr
            ? nullptr
            : static_assessment->find("decision");
    if (decision == nullptr ||
        decision->string_value() == nullptr) {
        return modules::ModuleResult{
            false,
            "OPERATION_FAILED",
            "Standard Edit assessment facts are incomplete.",
            nullptr,
        };
    }
    modules::CapabilityAvailability availability =
        modules::CapabilityAvailability::available;
    if (*decision->string_value() == "permission-blocked") {
        availability =
            modules::CapabilityAvailability::permission_blocked;
    } else if (
        *decision->string_value() != "requires-confirmation") {
        availability =
            modules::CapabilityAvailability::unavailable;
    }
    return assessment_.assess(
        capability,
        session_id,
        modules::TargetKind::standard_edit_control,
        availability);
}

modules::ModuleResult ApplicationFacadeSystem::run_app_create(
    const std::vector<std::string>& arguments) const {
    const auto source = option_value(arguments, "--input");
    if (!source.has_value()) {
        if (!has_flag(arguments, "--confirm")) {
            return text_documents_.create({}, false);
        }
        return invalid("app.create requires --input <file|->.");
    }
    auto input = components::read_json_input(*source);
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
    const auto* session =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* capability =
        args == nullptr ? nullptr : args->find("capability");
    const auto* provider_input =
        args == nullptr ? nullptr : args->find("input");
    const auto* text =
        provider_input == nullptr
            ? nullptr
            : provider_input->find("text");
    const auto* confirmed_value =
        input.value->find("confirmed");
    const bool confirmed =
        has_flag(arguments, "--confirm") ||
        (confirmed_value != nullptr &&
         confirmed_value->bool_value() != nullptr &&
         *confirmed_value->bool_value());
    if (!confirmed) {
        return text_documents_.create({}, false);
    }
    if (session == nullptr ||
        session->string_value() == nullptr ||
        capability == nullptr ||
        capability->string_value() == nullptr ||
        text == nullptr ||
        text->string_value() == nullptr) {
        return invalid(
            "app.create requires exact sessionId, capability, and input.text.");
    }
    if (*capability->string_value() !=
        "text.document.create@1") {
        return modules::ModuleResult{
            false,
            "CAPABILITY_UNSUPPORTED",
            "The requested app capability is not the text create route.",
            nullptr,
        };
    }
    const std::string& target_id =
        *session->string_value();
    if (!text_documents_.owns_session(target_id)) {
        if (target_id.starts_with("s1:")) {
            return modules::ModuleResult{
                false,
                "TARGET_ID_MIGRATION_REQUIRED",
                "Legacy facade targets must use the migration launcher.",
                nullptr,
            };
        }
        return modules::ModuleResult{
            false,
            "TARGET_NOT_FOUND",
            "The text document session is stale or unavailable.",
            nullptr,
        };
    }
    return compatibility_.map_app(
        text_documents_.create(
            *text->string_value(), true),
        target_id);
}

modules::ModuleResult ApplicationFacadeSystem::run_app_apply(
    const std::vector<std::string>& arguments) const {
    const auto source = option_value(arguments, "--input");
    if (!source.has_value()) {
        return standard_edits_.set_text(
            {}, {}, false, 2000U);
    }
    auto input = components::read_json_input(*source);
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
    const auto* provider_input =
        args == nullptr ? nullptr : args->find("input");
    const auto* session =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* capability =
        args == nullptr ? nullptr : args->find("capability");
    const auto* text =
        provider_input == nullptr
            ? nullptr
            : provider_input->find("text");
    const auto* timeout =
        provider_input == nullptr
            ? nullptr
            : provider_input->find("timeoutMs");
    const auto* confirmed_value =
        input.value->find("confirmed");
    const bool confirmed =
        has_flag(arguments, "--confirm") ||
        (confirmed_value != nullptr &&
         confirmed_value->bool_value() != nullptr &&
         *confirmed_value->bool_value());
    if (!confirmed) {
        return standard_edits_.set_text(
            {}, {}, false, 2000U);
    }
    if (session == nullptr ||
        session->string_value() == nullptr ||
        capability == nullptr ||
        capability->string_value() == nullptr ||
        text == nullptr ||
        text->string_value() == nullptr ||
        (timeout != nullptr &&
         timeout->integer_value() == nullptr)) {
        return invalid(
            "app.apply requires exact sessionId, capability, and input.text.");
    }
    if (*capability->string_value() != "ui.text.input@1") {
        return modules::ModuleResult{
            false,
            "CAPABILITY_UNSUPPORTED",
            "The requested app capability is not the standard Edit route.",
            nullptr,
        };
    }
    const std::int64_t timeout_ms =
        timeout == nullptr ? 2000 : *timeout->integer_value();
    if (timeout_ms <= 0 || timeout_ms > 30000) {
        return invalid(
            "Standard Edit timeoutMs must be 1..30000.");
    }
    const std::string& target_id =
        *session->string_value();
    if (!target_id.starts_with("s2:c:")) {
        return modules::ModuleResult{
            false,
            target_id.starts_with("s1:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "TARGET_NOT_FOUND",
            "The standard Edit session is stale or requires migration.",
            nullptr,
        };
    }
    return standard_edit_compatibility_.map_app_result(
        standard_edits_.set_text(
            target_id,
            *text->string_value(),
            true,
            static_cast<std::uint32_t>(timeout_ms)),
        target_id);
}

modules::ModuleResult ApplicationFacadeSystem::run_app_close(
    const std::vector<std::string>& arguments) const {
    const auto source = option_value(arguments, "--input");
    if (!source.has_value()) {
        return window_close_.close({}, false, 2000U);
    }
    auto input = components::read_json_input(*source);
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
    const auto* provider_input =
        args == nullptr ? nullptr : args->find("input");
    const auto* session =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* capability =
        args == nullptr ? nullptr : args->find("capability");
    const auto* timeout =
        provider_input == nullptr
            ? nullptr
            : provider_input->find("timeoutMs");
    const auto* confirmed_value =
        input.value->find("confirmed");
    const bool confirmed =
        has_flag(arguments, "--confirm") ||
        (confirmed_value != nullptr &&
         confirmed_value->bool_value() != nullptr &&
         *confirmed_value->bool_value());
    if (!confirmed) {
        return window_close_.close({}, false, 2000U);
    }
    if (session == nullptr ||
        session->string_value() == nullptr ||
        capability == nullptr ||
        capability->string_value() == nullptr ||
        provider_input == nullptr ||
        provider_input->object_items() == nullptr ||
        (timeout != nullptr &&
         timeout->integer_value() == nullptr)) {
        return invalid(
            "app.close requires exact sessionId, capability, and input.");
    }
    if (*capability->string_value() != "window.close@1") {
        return modules::ModuleResult{
            false,
            "CAPABILITY_UNSUPPORTED",
            "The requested app capability is not the exact window close "
            "route.",
            nullptr,
        };
    }
    const std::int64_t timeout_ms =
        timeout == nullptr ? 2000 : *timeout->integer_value();
    if (timeout_ms <= 0 || timeout_ms > 30000) {
        return invalid(
            "Window close timeoutMs must be 1..30000.");
    }
    const std::string& target_id =
        *session->string_value();
    if (!target_id.starts_with("s2:w:")) {
        return modules::ModuleResult{
            false,
            target_id.starts_with("s1:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "TARGET_NOT_FOUND",
            "The exact window session is stale or requires migration.",
            nullptr,
        };
    }
    return window_close_compatibility_.map_app_result(
        window_close_.close(
            target_id,
            true,
            static_cast<std::uint32_t>(timeout_ms)));
}

modules::ModuleResult
ApplicationFacadeSystem::run_legacy_standard_edit(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return standard_edit_compatibility_.map_run_result(
            standard_edits_.set_text({}, {}, false, 2000U));
    }
    const auto session =
        named_value(arguments, "--target", "sessionId");
    const auto text =
        named_value(arguments, "--arg", "text");
    const auto timeout = timeout_option(arguments);
    if (!session.has_value() ||
        !text.has_value() ||
        !timeout.has_value()) {
        return invalid(
            "win32-control.set-text requires exact sessionId, text, "
            "and timeout-ms 1..30000.");
    }
    if (!session->starts_with("s2:c:")) {
        return modules::ModuleResult{
            false,
            session->starts_with("win32-control:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "STALE_SESSION",
            "The standard Edit target is stale or requires migration.",
            nullptr,
        };
    }
    return standard_edit_compatibility_.map_run_result(
        standard_edits_.set_text(
            *session, *text, true, *timeout));
}

modules::ModuleResult
ApplicationFacadeSystem::run_legacy_type_text(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return standard_edit_compatibility_
            .map_desktop_type_text_result(
                standard_edits_.set_text(
                    {}, {}, false, 2000U));
    }
    const auto session =
        named_value(arguments, "--target", "sessionId");
    const auto text =
        named_value(arguments, "--arg", "text");
    const auto timeout = timeout_option(arguments);
    if (!session.has_value() ||
        !text.has_value() ||
        !timeout.has_value()) {
        return invalid(
            "desktop.type-text requires exact sessionId, text, "
            "and timeout-ms 1..30000.");
    }
    if (!session->starts_with("s2:c:")) {
        return modules::ModuleResult{
            false,
            session->starts_with("s1:") ||
                    session->starts_with("s2:w:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "STALE_SESSION",
            "C++ background type-text requires an exact discovered "
            "standard Edit control; legacy window targets remain on "
            "the migration launcher.",
            nullptr,
        };
    }
    return standard_edit_compatibility_
        .map_desktop_type_text_result(
            standard_edits_.set_text(
                *session, *text, true, *timeout));
}

modules::ModuleResult
ApplicationFacadeSystem::run_legacy_notepad(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return compatibility_.map_legacy(
            text_documents_.create({}, false));
    }
    const auto text =
        named_value(arguments, "--arg", "text");
    if (!text.has_value()) {
        return invalid(
            "notepad.open-and-write-text requires args.text.");
    }
    return compatibility_.map_legacy(
        text_documents_.create(*text, true));
}

}  // namespace act::systems
