#include "modules/standard_edit_compatibility_module.hpp"

#include "components/json.hpp"

namespace act::modules {

ModuleResult
StandardEditCompatibilityModule::map_run_result(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        if (provider_result.error_code == "TIMEOUT") {
            components::Json details =
                provider_result.error_details.has_value()
                    ? std::move(
                          *provider_result.error_details)
                    : components::object({
                          {"outcome", "unknown"},
                          {"retrySafe", false},
                          {"targetMayHaveMutated", true},
                      });
            auto fields = *details.object_items();
            fields.emplace_back(
                "providerErrorCode", "TIMEOUT");
            fields.emplace_back(
                "compatibilityErrorCode",
                "TARGET_HUNG_OR_UNAVAILABLE");
            return ModuleResult{
                false,
                "TARGET_HUNG_OR_UNAVAILABLE",
                "Edit control did not complete the certified "
                "message before timeout.",
                nullptr,
                components::Json(std::move(fields)),
            };
        }
        return provider_result;
    }
    const auto* session_id =
        provider_result.data.find("sessionId");
    const auto* verified =
        provider_result.data.find("verifiedByReadback");
    const auto* foreground =
        provider_result.data.find("foregroundUnchanged");
    if (session_id == nullptr ||
        session_id->string_value() == nullptr ||
        verified == nullptr ||
        verified->bool_value() == nullptr ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Standard Edit provider result is incomplete.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"app", "win32-control"},
            {"operation", "set-text"},
            {"sessionId", *session_id},
            {"foreground",
             components::object({
                 {"unchanged", *foreground},
                 {"nativeIdentifiersExposed", false},
             })},
            {"verifiedByReadback", *verified},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "secured-opaque-target-v1"},
        }),
    };
}

ModuleResult
StandardEditCompatibilityModule::map_app_result(
    ModuleResult provider_result,
    const std::string& target_id) const {
    ModuleResult legacy =
        map_run_result(std::move(provider_result));
    if (!legacy.ok) {
        return legacy;
    }
    const auto* verified =
        legacy.data.find("verifiedByReadback");
    if (verified == nullptr ||
        verified->bool_value() == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Standard Edit compatibility result is incomplete.",
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
            {"verb", "apply"},
            {"capability", "ui.text.input@1"},
            {"data",
             components::object({
                 {"kind", "standard-edit-control"},
                 {"state", "updated"},
                 {"verifiedByReadback", *verified},
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
            {"targetId", target_id},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "provider-neutral-standard-edit-v1"},
        }),
    };
}

ModuleResult
StandardEditCompatibilityModule::map_desktop_type_text_result(
    ModuleResult provider_result) const {
    ModuleResult legacy =
        map_run_result(std::move(provider_result));
    if (!legacy.ok) {
        return legacy;
    }
    const auto* session = legacy.data.find("sessionId");
    const auto* verified =
        legacy.data.find("verifiedByReadback");
    const auto* foreground = legacy.data.find("foreground");
    if (session == nullptr ||
        session->string_value() == nullptr ||
        verified == nullptr ||
        verified->bool_value() == nullptr ||
        foreground == nullptr ||
        foreground->object_items() == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Desktop type-text compatibility result is incomplete.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"app", "desktop"},
            {"operation", "type-text"},
            {"executionMode", "background-wm-settext"},
            {"target",
             components::object({
                 {"sessionId", *session->string_value()},
                 {"kind", "standard-edit-control"},
             })},
            {"verifiedByReadback", *verified},
            {"foreground", *foreground},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "secured-opaque-control-type-text-v1"},
        }),
    };
}

}  // namespace act::modules
