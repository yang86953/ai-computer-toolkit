#include "components/json.hpp"
#include "modules/recording_compatibility_module.hpp"

#include <iostream>

int main() {
    const act::modules::RecordingCompatibilityModule mapper;
    const act::modules::ModuleResult success{
        true,
        {},
        {},
        act::components::object({
            {"capability", "window.record@1"},
            {"targetId", "s2:w:0123456789abcdef"},
            {"path", "C:\\owned\\record.mp4"},
            {"analysisDir", "C:\\owned\\record.analysis"},
            {"bytes", 4096},
            {"sourceWidth", 406},
            {"sourceHeight", 273},
            {"width", 406},
            {"height", 272},
            {"fps", 2},
            {"durationMs", 2000},
            {"encodedFrames", 4},
            {"capturedFrames", 1},
            {"keyframeFiles", 1},
            {"deviceDriver", "hardware"},
            {"storyboardPath",
             "C:\\owned\\record.analysis\\storyboard.png"},
            {"manifestPath",
             "C:\\owned\\record.analysis\\manifest.json"},
            {"foregroundUnchanged", true},
        }),
    };
    const auto desktop = mapper.map_desktop_result(success);
    const auto app = mapper.map_app_facade_result(success);
    const auto stale = mapper.map_desktop_result(
        act::modules::ModuleResult{
            false,
            "STALE_SESSION",
            "stale",
            nullptr,
        });
    const auto interference = mapper.map_app_facade_result(
        act::modules::ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "changed",
            nullptr,
        });
    const auto* desktop_shape =
        desktop.data.find("compatibilityShape");
    const auto* app_shape =
        app.data.find("compatibilityShape");
    if (!desktop.ok ||
        desktop_shape == nullptr ||
        desktop_shape->string_value() == nullptr ||
        *desktop_shape->string_value() !=
            "secured-opaque-window-recording-v1" ||
        !app.ok ||
        app_shape == nullptr ||
        app_shape->string_value() == nullptr ||
        *app_shape->string_value() !=
            "provider-neutral-recording-artifact-v1" ||
        stale.ok ||
        stale.error_code != "TARGET_NOT_FOUND" ||
        interference.ok ||
        interference.error_code != "FOREGROUND_CHANGED") {
        std::cerr << "Recording compatibility mapping failed.\n";
        return 1;
    }
    return 0;
}
