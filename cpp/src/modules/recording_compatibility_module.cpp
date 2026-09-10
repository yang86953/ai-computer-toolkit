#include "modules/recording_compatibility_module.hpp"

#include "components/json.hpp"

namespace act::modules {
namespace {

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

const components::Json* field(
    const components::Json& data,
    const char* name) {
    return data.find(name);
}

bool complete_provider_result(
    const components::Json& data) {
    const auto* capability = field(data, "capability");
    const auto* target = field(data, "targetId");
    const auto* path = field(data, "path");
    const auto* analysis = field(data, "analysisDir");
    const auto* bytes = field(data, "bytes");
    const auto* source_width = field(data, "sourceWidth");
    const auto* source_height = field(data, "sourceHeight");
    const auto* width = field(data, "width");
    const auto* height = field(data, "height");
    const auto* fps = field(data, "fps");
    const auto* duration = field(data, "durationMs");
    const auto* encoded = field(data, "encodedFrames");
    const auto* captured = field(data, "capturedFrames");
    const auto* keyframes = field(data, "keyframeFiles");
    const auto* driver = field(data, "deviceDriver");
    const auto* storyboard = field(data, "storyboardPath");
    const auto* manifest = field(data, "manifestPath");
    const auto* foreground =
        field(data, "foregroundUnchanged");
    return capability != nullptr &&
           capability->string_value() != nullptr &&
           *capability->string_value() == "window.record@1" &&
           target != nullptr &&
           target->string_value() != nullptr &&
           path != nullptr && path->string_value() != nullptr &&
           analysis != nullptr &&
           analysis->string_value() != nullptr &&
           bytes != nullptr && bytes->integer_value() != nullptr &&
           source_width != nullptr &&
           source_width->integer_value() != nullptr &&
           source_height != nullptr &&
           source_height->integer_value() != nullptr &&
           width != nullptr && width->integer_value() != nullptr &&
           height != nullptr && height->integer_value() != nullptr &&
           fps != nullptr && fps->integer_value() != nullptr &&
           duration != nullptr &&
           duration->integer_value() != nullptr &&
           encoded != nullptr &&
           encoded->integer_value() != nullptr &&
           captured != nullptr &&
           captured->integer_value() != nullptr &&
           keyframes != nullptr &&
           keyframes->integer_value() != nullptr &&
           driver != nullptr &&
           driver->string_value() != nullptr &&
           storyboard != nullptr &&
           storyboard->string_value() != nullptr &&
           manifest != nullptr &&
           manifest->string_value() != nullptr &&
           foreground != nullptr &&
           foreground->bool_value() != nullptr &&
           *foreground->bool_value();
}

}  // namespace

ModuleResult
RecordingCompatibilityModule::map_desktop_result(
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
            "Recording provider result is incomplete.",
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
            {"app", "desktop"},
            {"operation", "record"},
            {"executionMode",
             "background-windows-graphics-capture"},
            {"captureMethod", "Windows.Graphics.Capture"},
            {"encoder", "ffmpeg/libx264"},
            {"target",
             components::object({
                 {"sessionId", *field(data, "targetId")},
                 {"targetKind", "application-window"},
             })},
            {"path", *field(data, "path")},
            {"bytes", *field(data, "bytes")},
            {"sourceWidth", *field(data, "sourceWidth")},
            {"sourceHeight", *field(data, "sourceHeight")},
            {"width", *field(data, "width")},
            {"height", *field(data, "height")},
            {"fps", *field(data, "fps")},
            {"durationMs", *field(data, "durationMs")},
            {"encodedFrames", *field(data, "encodedFrames")},
            {"capturedFrames", *field(data, "capturedFrames")},
            {"deviceDriver", *field(data, "deviceDriver")},
            {"audioCaptured", false},
            {"cursorCaptured", false},
            {"systemCaptureIndicatorMayAppear", true},
            {"analysis",
             components::object({
                 {"directory", *field(data, "analysisDir")},
                 {"storyboardPath",
                  *field(data, "storyboardPath")},
                 {"manifestPath", *field(data, "manifestPath")},
                 {"keyframeCount",
                  *field(data, "keyframeFiles")},
                 {"recommendedAiInput", "storyboardPath"},
                 {"fullVideoRole", "audit-only"},
             })},
            {"foreground",
             components::object({
                 {"unchanged", true},
                 {"nativeIdentifiersExposed", false},
             })},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "secured-opaque-window-recording-v1"},
        }),
    };
}

ModuleResult
RecordingCompatibilityModule::map_app_facade_result(
    ModuleResult provider_result) const {
    auto desktop =
        map_desktop_result(std::move(provider_result));
    if (!desktop.ok) {
        return desktop;
    }
    const auto* target = desktop.data.find("target");
    const auto* target_id =
        target == nullptr ? nullptr : target->find("sessionId");
    const auto* analysis = desktop.data.find("analysis");
    if (target_id == nullptr ||
        target_id->string_value() == nullptr ||
        analysis == nullptr ||
        analysis->object_items() == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Recording desktop compatibility result is incomplete.",
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
            {"verb", "record"},
            {"capability", "window.record@1"},
            {"targetId", *target_id},
            {"data",
             components::object({
                 {"path", *desktop.data.find("path")},
                 {"bytes", *desktop.data.find("bytes")},
                 {"width", *desktop.data.find("width")},
                 {"height", *desktop.data.find("height")},
                 {"fps", *desktop.data.find("fps")},
                 {"durationMs",
                  *desktop.data.find("durationMs")},
                 {"encodedFrames",
                  *desktop.data.find("encodedFrames")},
                 {"capturedFrames",
                  *desktop.data.find("capturedFrames")},
                 {"analysis", *analysis},
                 {"audioCaptured", false},
                 {"cursorCaptured", false},
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
             "provider-neutral-recording-artifact-v1"},
        }),
    };
}

}  // namespace act::modules
