#include "modules/window_close_compatibility_module.hpp"

#include "components/json.hpp"

namespace act::modules {

ModuleResult WindowCloseCompatibilityModule::map_app_result(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        return provider_result;
    }
    const auto* target =
        provider_result.data.find("targetId");
    const auto* closed =
        provider_result.data.find("closed");
    const auto* foreground =
        provider_result.data.find("foregroundUnchanged");
    if (target == nullptr ||
        target->string_value() == nullptr ||
        closed == nullptr ||
        closed->bool_value() == nullptr ||
        !*closed->bool_value() ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr ||
        !*foreground->bool_value()) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Window close provider result is incomplete.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "app"},
            {"verb", "close"},
            {"capability", "window.close@1"},
            {"targetId", *target},
            {"data",
             components::object({
                 {"kind", "application-window"},
                 {"state", "closed"},
                 {"closed", true},
                 {"foreground",
                  components::object({
                      {"unchanged", true},
                  })},
             })},
            {"meta",
             components::object({
                 {"foreground",
                  components::object({
                      {"unchanged", true},
                  })},
                 {"targeting", "opaque exact session"},
             })},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "provider-neutral-window-close-v1"},
        }),
    };
}

}  // namespace act::modules
