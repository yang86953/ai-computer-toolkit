#include "modules/foreground_input_module.hpp"

#include "components/json.hpp"
#include "components/key_chord.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"

#include <algorithm>

namespace act::modules {

ModuleResult ForegroundInputModule::press_key(
    const std::string& session_id,
    const std::string& key,
    const bool confirmed,
    const bool foreground_consent) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Foreground key input requires explicit operation confirmation.",
            nullptr,
        };
    }
    if (!foreground_consent) {
        return ModuleResult{
            false,
            "FOREGROUND_CONSENT_REQUIRED",
            "Foreground key input requires explicit foreground consent.",
            nullptr,
            components::object({
                {"requiredFlags",
                 components::Json(
                     components::Json::Array{
                         "--confirm",
                         "--allow-foreground",
                     })},
                {"execution", "foreground"},
            }),
        };
    }
    const auto parsed = components::parse_key_chord(key);
    if (!session_id.starts_with("s2:w:") ||
        !parsed.chord.has_value()) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            parsed.chord.has_value()
                ? "Foreground key input requires an opaque s2:w target."
                : parsed.error,
            nullptr,
        };
    }
    const auto windows = discovery_.enumerate_windows(16384U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto found = components::match_opaque_target(
        windows.begin(), windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 多命中时不得向任意窗口发送前台输入。
    if (found.state == components::OpaqueTargetMatchState::ambiguous) {
        // 在权限检查和输入派发前返回稳定歧义错误。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The exact foreground-input target resolves to multiple windows.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (found.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The exact foreground-input target no longer resolves.",
            nullptr,
        };
    }
    // 唯一命中后才读取目标进程关联。
    const auto& target = *found.position;
    const auto processes =
        processes_.enumerate_processes(16384U);
    const auto process = std::find_if(
        processes.records.begin(),
        processes.records.end(),
        [&target](const auto& candidate) {
            return candidate.native_process_id ==
                   target.native_process_id;
        });
    if (process == processes.records.end() ||
        process->metadata_access !=
            platform::windows::ProcessMetadataAccess::available ||
        process->integrity_relation ==
            platform::windows::IntegrityRelation::higher) {
        return ModuleResult{
            false,
            "PERMISSION_DENIED",
            "The target permission relation is not certified for "
            "foreground input.",
            nullptr,
        };
    }
    const auto result =
        backend_.press_key(target, *parsed.chord);
    if (result.error.has_value()) {
        std::optional<components::Json> details;
        if (result.error->code == "INPUT_OUTCOME_UNKNOWN") {
            details = components::object({
                {"outcome", "unknown"},
                {"retrySafe", false},
                {"inputMayHaveBeenPartiallyDispatched", true},
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
            {"capability", "ui.input.key@1"},
            {"targetId", session_id},
            {"targetKind", "application-window"},
            {"key", parsed.chord->normalized},
            {"executionDomain", "foreground-consent"},
            {"confirmationSatisfied", true},
            {"foregroundConsentSatisfied", true},
            {"targetAcquiredBeforeDispatch",
             result.evidence->target_acquired_before_dispatch},
            {"fullInputDispatched",
             result.evidence->full_input_dispatched},
            {"targetStillForegroundAfter",
             result.evidence->target_still_foreground_after},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

}  // namespace act::modules
