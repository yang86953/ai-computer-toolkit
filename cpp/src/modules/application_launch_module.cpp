#include "modules/application_launch_module.hpp"

#include "components/json.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"

namespace act::modules {

ModuleResult ApplicationLaunchModule::launch(
    const std::string& session_id,
    const bool confirmed) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Application launch requires explicit confirmation.",
            nullptr,
        };
    }
    if (!session_id.starts_with("s2:a:")) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Application launch requires an exact opaque s2:a target.",
            nullptr,
        };
    }
    const auto snapshot =
        discovery_.capture(4096U, 1U, 1U);
    if (!snapshot.foreground_unchanged) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during application target discovery.",
            nullptr,
        };
    }
    // 完整扫描应用清单以拒绝哈希碰撞或重复身份。
    const auto found = components::match_opaque_target(
        snapshot.applications.begin(),
        snapshot.applications.end(),
        [&session_id](const auto& application) {
            return application.session_id == session_id;
        });
    // 多命中时关闭失败，不能启动任意应用。
    if (found.state == components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定的跨实现歧义错误。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The exact installed application resolves to multiple records.",
            nullptr,
        };
    }
    // 零命中保持既有的过期目标语义。
    if (found.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The exact installed application no longer resolves.",
            nullptr,
        };
    }
    // 唯一命中后才解引用应用记录。
    const auto& application = *found.position;
    if (application.launch_provider !=
        platform::windows::ApplicationLaunchProvider::shell_item) {
        return ModuleResult{
            false,
            "CAPABILITY_UNAVAILABLE",
            "The exact installed application has no certified public "
            "Shell launch capability.",
            nullptr,
        };
    }
    const std::string foreground_before =
        foreground_.foreground_token();
    const auto result = backend_.launch(application);
    const bool foreground_unchanged =
        foreground_before == foreground_.foreground_token();
    if (result.error.has_value()) {
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "application.open@1"},
            {"targetId", session_id},
            {"targetKind", "installed-application"},
            {"launchDispatched",
             result.evidence->launch_dispatched},
            {"executionDomain", "windows-public-shell"},
            {"confirmationSatisfied", true},
            {"foregroundUnchanged", foreground_unchanged},
            {"foregroundMayChange", true},
            {"nativeIdentifiersExposed", false},
            {"runtimePathExposed", false},
        }),
    };
}

}  // namespace act::modules
