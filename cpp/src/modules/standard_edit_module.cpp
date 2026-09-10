#include "modules/standard_edit_module.hpp"

#include "components/json.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "components/static_permission_assessment.hpp"
#include "platform/windows/text_codec.hpp"

#include <algorithm>
#include <string_view>

namespace act::modules {
namespace {

components::StaticPermissionAssessment permission_assessment(
    const platform::windows::ProcessRecord* process) {
    const auto metadata_access = process == nullptr
        ? components::StaticMetadataAccess::unavailable
        : (process->metadata_access ==
                   platform::windows::ProcessMetadataAccess::available
               ? components::StaticMetadataAccess::available
               : (process->metadata_access ==
                          platform::windows::ProcessMetadataAccess::
                              permission_blocked
                      ? components::StaticMetadataAccess::
                            permission_blocked
                      : components::StaticMetadataAccess::unavailable));
    components::StaticIntegrityRelation integrity =
        components::StaticIntegrityRelation::unknown;
    if (process != nullptr) {
        using Source =
            platform::windows::IntegrityRelation;
        switch (process->integrity_relation) {
            case Source::lower:
                integrity =
                    components::StaticIntegrityRelation::lower;
                break;
            case Source::same:
                integrity =
                    components::StaticIntegrityRelation::same;
                break;
            case Source::higher:
                integrity =
                    components::StaticIntegrityRelation::higher;
                break;
            case Source::unknown:
                break;
        }
    }
    return components::assess_static_background_mutation(
        metadata_access, integrity);
}

components::Json session_json(
    const platform::windows::StandardEditRecord& control,
    const platform::windows::ProcessRecord* process) {
    const auto assessment = permission_assessment(process);
    return components::object({
        {"sessionId", control.session_id},
        {"kind", "control"},
        {"targetKind", "standard-edit-control"},
        {"applicationName", control.application_name},
        {"visible", control.visible},
        {"capability",
         components::object({
             {"id", "ui.text.input@1"},
             {"risk", "mutation"},
             {"executionDomain", "same-session-no-focus"},
             {"requiresConfirmation", true},
             {"availability", "available"},
         })},
        {"capabilities",
         components::Json(
             components::Json::Array{
                 components::object({
                     {"id", "ui.text.input@1"},
                     {"version", 1},
                     {"verb", "apply"},
                     {"availability", "available"},
                     {"execution", "same-session-no-focus"},
                     {"requiresConfirmation", true},
                     {"requiresForegroundConsent", false},
                     {"inputSchema",
                      "schema://ui/text-input/v1"},
                 }),
             })},
        {"assessment",
         components::object({
             {"decision", assessment.decision},
             {"safeToExecuteNow",
              assessment.safe_to_execute_now},
             {"requiresConfirmation",
              assessment.requires_confirmation},
             {"foregroundRequired",
              assessment.foreground_required},
             {"permissionRelation",
              assessment.permission_relation},
             {"activeWriteProbePerformed",
              assessment.active_write_probe_performed},
         })},
    });
}

const platform::windows::ProcessRecord* find_process(
    const platform::windows::ProcessInventory& inventory,
    const std::uint32_t process_id) {
    const auto found = std::find_if(
        inventory.records.begin(),
        inventory.records.end(),
        [process_id](const auto& process) {
            return process.native_process_id == process_id;
        });
    return found == inventory.records.end()
               ? nullptr
               : &*found;
}

ModuleResult failure(
    const platform::windows::BackendError& error) {
    std::optional<components::Json> details;
    if (error.code == "TIMEOUT") {
        details = components::object({
            {"outcome", "unknown"},
            {"retrySafe", false},
            {"targetMayHaveMutated", true},
            {"reason",
             "synchronous-window-message-timeout"},
        });
    }
    return ModuleResult{
        false,
        error.code,
        error.message,
        nullptr,
        std::move(details),
    };
}

}  // namespace

ModuleResult StandardEditModule::status() const {
    const auto foreground_before =
        foreground_.foreground_token();
    const auto records = backend_.enumerate(16384U);
    const auto processes =
        processes_.enumerate_processes(16384U);
    if (foreground_before !=
        foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during standard Edit status.",
            nullptr,
        };
    }
    std::size_t requires_confirmation = 0U;
    std::size_t permission_blocked = 0U;
    std::size_t indeterminate = 0U;
    for (const auto& record : records) {
        const auto assessment = permission_assessment(
            find_process(processes, record.native_process_id));
        const std::string_view decision(assessment.decision);
        if (decision == "requires-confirmation") {
            ++requires_confirmation;
        } else if (decision == "permission-blocked") {
            ++permission_blocked;
        } else {
            ++indeterminate;
        }
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"surface", "win32-control"},
            {"capability", "ui.text.input@1"},
            {"readOnly", true},
            {"runtimeDetected", true},
            {"cppExecutionEnabled", true},
            {"cppStatus",
             "available-confirmed-opaque-target"},
            {"backgroundPolicy", "guaranteed"},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
            {"runtimePathExposed", false},
            {"writesEnabled", true},
            {"activeWriteProbes", 0},
            {"controlCount",
             static_cast<std::int64_t>(records.size())},
            {"requiresConfirmationCount",
             static_cast<std::int64_t>(
                 requires_confirmation)},
            {"permissionBlockedCount",
             static_cast<std::int64_t>(
                 permission_blocked)},
            {"indeterminateCount",
             static_cast<std::int64_t>(indeterminate)},
        }),
    };
}

ModuleResult StandardEditModule::sessions(
    const std::size_t maximum_items) const {
    if (maximum_items == 0U ||
        maximum_items > 4096U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Standard Edit discovery requires max-items 1..4096.",
            nullptr,
        };
    }
    const auto foreground_before =
        foreground_.foreground_token();
    const auto records =
        backend_.enumerate(16384U);
    const auto processes =
        processes_.enumerate_processes(16384U);
    if (foreground_before !=
        foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during standard Edit discovery.",
            nullptr,
        };
    }
    components::Json::Array sessions;
    const std::size_t count =
        std::min(maximum_items, records.size());
    sessions.reserve(count);
    for (std::size_t index = 0; index < count; ++index) {
        sessions.push_back(session_json(
            records[index],
            find_process(
                processes,
                records[index].native_process_id)));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "ui.text.input@1"},
            {"readOnly", true},
            {"candidateOnly", false},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"total",
             static_cast<std::int64_t>(records.size())},
            {"truncated", records.size() > count},
            {"sessions", components::Json(std::move(sessions))},
        }),
    };
}

ModuleResult StandardEditModule::set_text(
    const std::string& session_id,
    const std::string& text,
    const bool confirmed,
    const std::uint32_t timeout_ms) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Standard Edit text mutation requires confirmation.",
            nullptr,
        };
    }
    if (session_id.empty() ||
        text.size() > 65536U ||
        timeout_ms == 0U ||
        timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Standard Edit mutation requires an exact target, "
            "at most 65536 UTF-8 bytes, and timeout-ms 1..30000.",
            nullptr,
        };
    }
    const std::wstring wide_text =
        platform::windows::wide(text);
    if (!text.empty() && wide_text.empty()) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Standard Edit text must be valid UTF-8.",
            nullptr,
        };
    }
    const auto records = backend_.enumerate(16384U);
    // 完整扫描控件清单以检测 sessionId 碰撞。
    const auto target = components::match_opaque_target(
        records.begin(),
        records.end(),
        [&session_id](const auto& record) {
            return record.session_id == session_id;
        });
    // 多命中时不允许向任意控件写入文本。
    if (target.state == components::OpaqueTargetMatchState::ambiguous) {
        // 在权限评估和写后端前返回稳定歧义错误。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The exact standard Edit target resolves to multiple controls.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (target.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The exact standard Edit target is unavailable.",
            nullptr,
        };
    }
    // 唯一命中后才读取目标控件及其进程关联。
    const auto& control = *target.position;
    const auto processes =
        processes_.enumerate_processes(16384U);
    const auto assessment = permission_assessment(
        find_process(
            processes,
            control.native_process_id));
    const std::string_view decision(
        assessment.decision);
    if (decision == "permission-blocked") {
        return ModuleResult{
            false,
            "PERMISSION_DENIED",
            "The exact standard Edit target is outside the certified "
            "same-integrity mutation boundary.",
            nullptr,
            components::object({
                {"permissionRelation",
                 assessment.permission_relation},
                {"activeWriteProbePerformed", false},
                {"safeToRetryAutomatically", false},
            }),
        };
    }
    if (decision != "requires-confirmation") {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact standard Edit target permission relation is "
            "indeterminate.",
            nullptr,
            components::object({
                {"permissionRelation",
                 assessment.permission_relation},
                {"activeWriteProbePerformed", false},
                {"safeToRetryAutomatically", false},
            }),
        };
    }
    const auto foreground_before =
        foreground_.foreground_token();
    const auto result = backend_.set_text(
        control, wide_text, timeout_ms);
    if (foreground_before !=
        foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during standard Edit mutation.",
            nullptr,
            components::object({
                {"outcome", "completed"},
                {"retrySafe", false},
                {"targetMayHaveMutated", true},
            }),
        };
    }
    if (result.error.has_value()) {
        return failure(*result.error);
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "ui.text.input@1"},
            {"sessionId", session_id},
            {"executionDomain", "same-session-no-focus"},
            {"confirmed", true},
            {"verifiedByReadback", result.verified},
            {"textBytes",
             static_cast<std::int64_t>(text.size())},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

ModuleResult StandardEditModule::inspect(
    const std::string& session_id) const {
    if (session_id.empty()) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Standard Edit inspect requires an exact opaque target.",
            nullptr,
        };
    }
    const auto foreground_before =
        foreground_.foreground_token();
    const auto records = backend_.enumerate(16384U);
    const auto processes =
        processes_.enumerate_processes(16384U);
    // 完整扫描控件清单以检测 sessionId 碰撞。
    const auto target = components::match_opaque_target(
        records.begin(),
        records.end(),
        [&session_id](const auto& record) {
            return record.session_id == session_id;
        });
    // 多命中时不返回任意控件的观测结果。
    if (target.state == components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定歧义错误且不暴露候选记录。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The exact standard Edit target resolves to multiple controls.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (target.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The exact standard Edit target is unavailable.",
            nullptr,
        };
    }
    // 唯一命中后才构造控件观测结果。
    const auto& control = *target.position;
    if (foreground_before !=
        foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during standard Edit inspection.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "ui.text.input@1"},
            {"readOnly", true},
            {"candidateOnly", false},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
            {"control",
             session_json(
                 control,
                 find_process(
                     processes,
                     control.native_process_id))},
        }),
    };
}

}  // namespace act::modules
