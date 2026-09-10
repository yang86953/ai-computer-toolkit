#include "modules/media_control_compatibility_module.hpp"

#include "components/json.hpp"

namespace act::modules {
namespace {

const components::Json* field(
    const components::Json& data,
    const char* name) {
    return data.find(name);
}

std::string compatibility_error(
    const ModuleResult& result) {
    if (result.error_code == "STALE_SESSION") {
        return "TARGET_NOT_FOUND";
    }
    if (result.error_code ==
        "HOST_INTERFERENCE_DETECTED") {
        return "FOREGROUND_CHANGED";
    }
    return result.error_code;
}

bool complete_provider_result(
    const components::Json& data) {
    const auto* capability = field(data, "capability");
    const auto* operation = field(data, "operation");
    const auto* accepted = field(data, "accepted");
    const auto* session = field(data, "session");
    const auto* observation =
        field(data, "sessionObservation");
    const auto* foreground =
        field(data, "foregroundUnchanged");
    return capability != nullptr &&
           capability->string_value() != nullptr &&
           *capability->string_value() ==
               "media.playback.control@1" &&
           operation != nullptr &&
           operation->string_value() != nullptr &&
           accepted != nullptr &&
           accepted->bool_value() != nullptr &&
           *accepted->bool_value() &&
           session != nullptr &&
           session->object_items() != nullptr &&
           observation != nullptr &&
           observation->string_value() != nullptr &&
           *observation->string_value() == "before-control" &&
           foreground != nullptr &&
           foreground->bool_value() != nullptr &&
           *foreground->bool_value();
}

}  // namespace

ModuleResult
MediaControlCompatibilityModule::map_legacy_result(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        return ModuleResult{
            false,
            compatibility_error(provider_result),
            provider_result.error_message,
            nullptr,
            std::move(provider_result.error_details),
        };
    }
    if (!complete_provider_result(provider_result.data)) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Media control provider result is incomplete.",
            nullptr,
        };
    }
    const auto& data = provider_result.data;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "media-session"},
            {"operation", *field(data, "operation")},
            {"executionMode", "background-media-session"},
            {"accepted", true},
            {"session", *field(data, "session")},
            {"sessionObservation", "before-control"},
            {"foreground",
             components::object({
                 {"unchanged", true},
                 {"nativeIdentifiersExposed", false},
             })},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "secured-opaque-media-session-v1"},
        }),
    };
}

}  // namespace act::modules
