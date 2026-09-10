#include "modules/window_close_module.hpp"

#include "components/json.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"

namespace act::modules {

ModuleResult WindowCloseModule::close(
    const std::string& session_id,
    const bool confirmed,
    const std::uint32_t timeout_ms) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Exact window close requires confirmation.",
            nullptr,
        };
    }
    if (!session_id.starts_with("s2:w:") ||
        timeout_ms == 0U || timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Window close requires an exact s2 window and timeout-ms "
            "1..30000.",
            nullptr,
        };
    }
    const auto windows = discovery_.enumerate_windows(16384U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto target = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 多命中时不能关闭任意窗口。
    if (target.state == components::OpaqueTargetMatchState::ambiguous) {
        // 在进入写后端前返回稳定歧义错误。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The exact window-close target resolves to multiple windows.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (target.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The exact window target is stale or unavailable.",
            nullptr,
        };
    }
    const std::string foreground_before =
        discovery_.foreground_token();
    if (foreground_before == session_id) {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact window is currently foreground; closing it would "
            "necessarily change foreground ownership.",
            nullptr,
            components::object({
                {"reason", "exact-target-currently-foreground"},
                {"closeRequested", false},
                {"safeToRetryAutomatically", false},
            }),
        };
    }
    // 唯一命中后才把窗口交给关闭后端。
    const auto result = backend_.close(*target.position, timeout_ms);
    const bool foreground_unchanged =
        foreground_before == discovery_.foreground_token();
    if (!foreground_unchanged) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed after the exact window close request.",
            nullptr,
            components::object({
                {"outcome", "unknown"},
                {"retrySafe", false},
                {"targetMayHaveClosed", true},
            }),
        };
    }
    if (result.error.has_value()) {
        std::optional<components::Json> details;
        if (result.error->code == "TIMEOUT") {
            details = components::object({
                {"outcome", "unknown"},
                {"retrySafe", false},
                {"targetMayCloseLater", true},
            });
        }
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
            std::move(details),
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.close@1"},
            {"targetId", session_id},
            {"executionDomain", "same-session-no-focus"},
            {"confirmed", true},
            {"closeRequested", true},
            {"closed", result.closed},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

}  // namespace act::modules
