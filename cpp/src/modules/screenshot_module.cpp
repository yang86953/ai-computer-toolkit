#include "modules/screenshot_module.hpp"

#include "components/json.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "platform/windows/png_file_output.hpp"

namespace act::modules {

ModuleResult ScreenshotModule::capture(
    const std::string& session_id,
    const std::string& output_path,
    const bool confirmed,
    const bool overwrite,
    const std::uint32_t timeout_ms) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Window screenshot requires explicit confirmation.",
            nullptr,
        };
    }

    const auto foreground_before = discovery_.foreground_token();
    const auto windows = discovery_.enumerate_windows(4096U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 多命中时不允许捕获任意窗口。
    if (match.state == components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定的歧义错误而不进入预检或 worker。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The opaque application-window session resolves to multiple windows.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (match.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The opaque application-window session no longer resolves.",
            nullptr,
        };
    }
    if (timeout_ms < 250U || timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Window screenshot timeout must be from 250 through 30000 ms.",
            nullptr,
        };
    }

    const auto output_plan =
        platform::windows::validate_png_output_path(
            output_path, overwrite);
    if (!output_plan.plan.has_value()) {
        return ModuleResult{
            false,
            output_plan.error_code,
            output_plan.error_message,
            nullptr,
        };
    }

    // 唯一命中后才把窗口交给捕获预检。
    const auto readiness = preflight_.inspect(*match.position);
    if (readiness.error.has_value()) {
        return ModuleResult{
            false,
            readiness.error->code,
            readiness.error->message,
            nullptr,
        };
    }
    if (readiness.preflight->eligibility !=
        "eligible-for-certified-capture-route") {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact target is not eligible for background capture: " +
                readiness.preflight->eligibility + ".",
            nullptr,
        };
    }

    const auto result = worker_.capture_screenshot(
        session_id,
        output_plan.plan->normalized_path,
        overwrite,
        timeout_ms);
    const auto foreground_after = discovery_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The screenshot file may exist, but foreground changed during "
            "isolated capture.",
            nullptr,
        };
    }
    if (result.error.has_value()) {
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
        };
    }

    const auto& screenshot = *result.screenshot;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.screenshot@1"},
            {"targetId", session_id},
            {"targetKind", "application-window"},
            {"executionDomain", "isolated-worker"},
            {"confirmationRequired", true},
            {"confirmationSatisfied", true},
            {"overwriteConfirmed", overwrite},
            {"path", screenshot.output_path},
            {"bytes", screenshot.bytes},
            {"width", screenshot.width},
            {"height", screenshot.height},
            {"deviceDriver", screenshot.device_driver},
            {"pixelDigest", screenshot.pixel_digest},
            {"atomicOutput", true},
            {"cursorCaptured", false},
            {"systemCaptureIndicatorMayAppear", true},
            {"foregroundUnchanged", true},
        }),
    };
}

}  // namespace act::modules
