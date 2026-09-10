#include "modules/application_launch_compatibility_module.hpp"

#include "components/json.hpp"

namespace act::modules {

ModuleResult
ApplicationLaunchCompatibilityModule::map_desktop_result(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        const std::string code =
            provider_result.error_code == "STALE_SESSION"
                ? "TARGET_NOT_FOUND"
                : provider_result.error_code;
        return ModuleResult{
            false,
            code,
            provider_result.error_message,
            nullptr,
            std::move(provider_result.error_details),
        };
    }
    const auto* target =
        provider_result.data.find("targetId");
    const auto* dispatched =
        provider_result.data.find("launchDispatched");
    const auto* foreground =
        provider_result.data.find("foregroundUnchanged");
    if (target == nullptr ||
        target->string_value() == nullptr ||
        dispatched == nullptr ||
        dispatched->bool_value() == nullptr ||
        !*dispatched->bool_value() ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Application launch provider result is incomplete.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "desktop"},
            {"operation", "launch"},
            {"executionMode", "exact-installed-application-shell"},
            {"target",
             components::object({
                 {"sessionId", *target},
                 {"targetKind", "installed-application"},
             })},
            {"launchDispatched", true},
            {"foreground",
             components::object({
                 {"unchanged", *foreground},
                 {"mayChange", true},
                 {"nativeIdentifiersExposed", false},
             })},
            {"nativeIdentifiersExposed", false},
            {"runtimePathExposed", false},
            {"compatibilityShape",
             "secured-opaque-application-launch-v1"},
        }),
    };
}

}  // namespace act::modules
