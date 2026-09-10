#include "modules/screenshot_compatibility_module.hpp"

#include "components/json.hpp"

#include <string>

namespace act::modules {
namespace {

std::string compatibility_error(const ModuleResult& result) {
    if (result.error_code == "STALE_SESSION") {
        return "TARGET_NOT_FOUND";
    }
    if (result.error_code == "TIMEOUT") {
        return "CAPTURE_TIMEOUT";
    }
    if (result.error_code == "RESOURCE_LIMIT_EXCEEDED" ||
        result.error_code == "CAPTURE_READBACK_FAILED") {
        return "CAPTURE_READBACK_FAILED";
    }
    if (result.error_code == "HOST_INTERFERENCE_DETECTED") {
        return "FOREGROUND_CHANGED";
    }
    if (result.error_code ==
            "BACKGROUND_OPERATION_UNAVAILABLE" &&
        result.error_details.has_value()) {
        const auto* state =
            result.error_details->find("targetState");
        if (state != nullptr &&
            state->string_value() != nullptr) {
            if (*state->string_value() == "hidden") {
                return "CAPTURE_TARGET_HIDDEN";
            }
            if (*state->string_value() == "minimized") {
                return "CAPTURE_TARGET_MINIMIZED";
            }
        }
    }
    return result.error_code;
}

}  // namespace

ModuleResult ScreenshotCompatibilityModule::map_desktop_result(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        const std::string code =
            compatibility_error(provider_result);
        return ModuleResult{
            false,
            code,
            provider_result.error_message,
            nullptr,
            std::move(provider_result.error_details),
        };
    }
    const auto* capability =
        provider_result.data.find("capability");
    const auto* target =
        provider_result.data.find("targetId");
    const auto* path = provider_result.data.find("path");
    const auto* bytes = provider_result.data.find("bytes");
    const auto* width = provider_result.data.find("width");
    const auto* height = provider_result.data.find("height");
    const auto* driver =
        provider_result.data.find("deviceDriver");
    const auto* foreground =
        provider_result.data.find("foregroundUnchanged");
    if (capability == nullptr ||
        capability->string_value() == nullptr ||
        *capability->string_value() != "window.screenshot@1" ||
        target == nullptr ||
        target->string_value() == nullptr ||
        path == nullptr ||
        path->string_value() == nullptr ||
        bytes == nullptr ||
        bytes->integer_value() == nullptr ||
        width == nullptr ||
        width->integer_value() == nullptr ||
        height == nullptr ||
        height->integer_value() == nullptr ||
        driver == nullptr ||
        driver->string_value() == nullptr ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr ||
        !*foreground->bool_value()) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Screenshot provider result is incomplete.",
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
            {"operation", "screenshot"},
            {"executionMode",
             "background-windows-graphics-capture"},
            {"captureMethod", "Windows.Graphics.Capture"},
            {"target",
             components::object({
                 {"sessionId", *target},
                 {"targetKind", "application-window"},
             })},
            {"path", *path},
            {"bytes", *bytes},
            {"width", *width},
            {"height", *height},
            {"deviceDriver", *driver},
            {"cursorCaptured", false},
            {"systemCaptureIndicatorMayAppear", true},
            {"foreground",
             components::object({
                 {"unchanged", true},
                 {"nativeIdentifiersExposed", false},
             })},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "secured-opaque-window-v1"},
        }),
    };
}

ModuleResult ScreenshotCompatibilityModule::map_app_facade_result(
    ModuleResult provider_result) const {
    ModuleResult desktop =
        map_desktop_result(std::move(provider_result));
    if (!desktop.ok) {
        return desktop;
    }
    const auto* target = desktop.data.find("target");
    const auto* target_id =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* path = desktop.data.find("path");
    const auto* bytes = desktop.data.find("bytes");
    const auto* width = desktop.data.find("width");
    const auto* height = desktop.data.find("height");
    const auto* cursor =
        desktop.data.find("cursorCaptured");
    if (target_id == nullptr ||
        target_id->string_value() == nullptr ||
        path == nullptr ||
        bytes == nullptr ||
        width == nullptr ||
        height == nullptr ||
        cursor == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Screenshot desktop compatibility result is incomplete.",
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
            {"verb", "screenshot"},
            {"capability", "window.screenshot@1"},
            {"targetId", *target_id},
            {"data",
             components::object({
                 {"path", *path},
                 {"bytes", *bytes},
                 {"width", *width},
                 {"height", *height},
                 {"cursorCaptured", *cursor},
                 {"foregroundUnchanged", true},
             })},
            {"meta",
             components::object({
                 {"foreground",
                  components::object({
                      {"unchanged", true},
                      {"nativeIdentifiersExposed", false},
                  })},
                 {"targeting", "opaque exact session"},
             })},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "provider-neutral-artifact-v1"},
        }),
    };
}

}  // namespace act::modules
