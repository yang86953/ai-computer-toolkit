#include "modules/structured_image_module.hpp"

#include "components/json.hpp"
// 复用共享的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "components/output_path_policy.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <optional>

namespace act::modules {
namespace {

components::Json capability(
    const char* id,
    const char* verb,
    const char* format) {
    return components::object({
        {"id", id},
        {"verb", verb},
        {"execution", "background-provider-may-change-foreground"},
        {"requiresForeground", false},
        {"requiresConfirmation", true},
        {"format", format},
        {"overwriteProtection", true},
        {"migrationState", "candidate-not-certified"},
    });
}

components::Json document_json(
    const platform::windows::StructuredImageDocumentRecord& document) {
    return components::object({
        {"sessionId", document.session_id},
        {"kind", "document"},
        {"title", document.name},
        {"state", document.saved ? "saved" : "modified"},
        {"document",
         components::object({
             {"width", document.width_px},
             {"height", document.height_px},
             {"resolution", document.resolution_dpi},
             {"layerCount", document.layer_count},
             {"saved", document.saved},
             {"active", document.active},
         })},
        {"capabilities",
         components::array({
             capability("artifact.save@1", "save", "psd"),
             capability("image.export@1", "export", "png"),
         })},
    });
}

// 在当前文档清单中保留零、一或多命中状态。
auto
resolve_exact(
    const platform::windows::StructuredImageInventory& inventory,
    const std::string& session_id) {
    // 完整扫描文档清单以拒绝 sessionId 碰撞。
    return components::match_opaque_target(
        inventory.documents.begin(),
        inventory.documents.end(),
        [&session_id](const auto& document) {
            return document.session_id == session_id;
        });
}

ModuleResult provider_error(
    const platform::windows::StructuredImageInventory& inventory) {
    return ModuleResult{
        false,
        inventory.error_code.value_or("OPERATION_FAILED"),
        inventory.error_message.value_or(
            "Structured image provider observation failed."),
        nullptr,
    };
}

}  // namespace

ModuleResult StructuredImageModule::status() const {
    const auto observation = observation_.status(5000U);
    if (observation.error.has_value()) {
        return ModuleResult{
            false,
            observation.error->code,
            observation.error->message,
            nullptr,
        };
    }
    const auto& status = *observation.status;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"provider", "structured-image-editor"},
            {"backend", "attach-only-com-automation"},
            {"installed", status.installed},
            {"connected", status.connected},
            {"version",
             status.version.has_value()
                 ? components::Json(*status.version)
                 : components::Json(nullptr)},
            {"arbitraryScriptAccepted", false},
            {"applicationLaunchAllowed", false},
            {"writeTimeoutReported", false},
            {"foregroundUnchanged",
             observation.foreground_unchanged},
            {"observationIsolation", "job-bounded-worker"},
            {"observationTimeoutMs", 5000},
            {"observationCancellable", true},
            {"publicRouteEnabled", false},
        }),
    };
}

ModuleResult StructuredImageModule::sessions() const {
    const auto observation = observation_.inventory(5000U);
    if (observation.error.has_value()) {
        return ModuleResult{
            false,
            observation.error->code,
            observation.error->message,
            nullptr,
        };
    }
    const auto& inventory = *observation.inventory;
    components::Json::Array sessions;
    if (inventory.application_session_id.has_value()) {
        sessions.push_back(components::object({
            {"sessionId", *inventory.application_session_id},
            {"kind", "application"},
            {"title", "Structured image editor"},
            {"state", "running"},
            {"capabilities", components::Json::Array{}},
        }));
    }
    for (const auto& document : inventory.documents) {
        sessions.push_back(document_json(document));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"provider", "structured-image-editor"},
            {"readOnly", true},
            {"foregroundUnchanged",
             observation.foreground_unchanged},
            {"observationIsolation", "job-bounded-worker"},
            {"observationTimeoutMs", 5000},
            {"observationCancellable", true},
            {"targetIdentity", "opaque-exact-session"},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"sessions", components::Json(std::move(sessions))},
            {"publicWriteRouteEnabled", false},
        }),
    };
}

ModuleResult StructuredImageModule::inspect(
    const std::string& session_id) const {
    if (!session_id.starts_with("s2:a:") &&
        !session_id.starts_with("s2:d:")) {
        return ModuleResult{
            false,
            session_id.starts_with("s1:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "TARGET_NOT_FOUND",
            "The structured image session is unavailable.",
            nullptr,
        };
    }
    const auto observation = observation_.inventory(5000U);
    if (observation.error.has_value()) {
        return ModuleResult{
            false,
            observation.error->code,
            observation.error->message,
            nullptr,
        };
    }
    const auto& inventory = *observation.inventory;
    if (inventory.application_session_id == session_id) {
        const auto provider_status = status();
        if (!provider_status.ok) {
            return provider_status;
        }
        return ModuleResult{
            true,
            {},
            {},
            components::object({
                {"sessionId", session_id},
                {"kind", "application"},
                {"providerStatus", provider_status.data},
            }),
        };
    }
    const auto document =
        resolve_exact(inventory, session_id);
    // 多命中时不返回任意文档的观测结果。
    if (document.state == components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定歧义错误且不暴露候选记录。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The structured image session resolves to multiple documents.",
            nullptr,
        };
    }
    // 零命中保持现有只读入口错误映射。
    if (document.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "TARGET_NOT_FOUND",
            "The structured image session is stale or unavailable.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"session", document_json(*document.position)},
            {"readOnly", true},
            {"foregroundUnchanged",
             observation.foreground_unchanged},
            {"observationIsolation", "job-bounded-worker"},
        }),
    };
}

ModuleResult StructuredImageModule::save(
    const std::string& session_id,
    const std::filesystem::path& path,
    const bool overwrite,
    const bool confirmed) const {
    return write(
        "artifact.save@1",
        session_id,
        path,
        "psd",
        overwrite,
        confirmed);
}

ModuleResult StructuredImageModule::export_png(
    const std::string& session_id,
    const std::filesystem::path& path,
    const bool overwrite,
    const bool confirmed) const {
    return write(
        "image.export@1",
        session_id,
        path,
        "png",
        overwrite,
        confirmed);
}

ModuleResult StructuredImageModule::write(
    const std::string& capability_id,
    const std::string& session_id,
    const std::filesystem::path& path,
    const std::string& extension,
    const bool overwrite,
    const bool confirmed) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Structured image writes require explicit confirmation.",
            nullptr,
        };
    }
    if (!session_id.starts_with("s2:d:")) {
        return ModuleResult{
            false,
            session_id.starts_with("s1:")
                ? "TARGET_ID_MIGRATION_REQUIRED"
                : "TARGET_NOT_FOUND",
            "An exact current C++ document session is required.",
            nullptr,
        };
    }
    const auto inventory = backend_.inventory();
    if (inventory.error_code.has_value()) {
        return provider_error(inventory);
    }
    const auto document =
        resolve_exact(inventory, session_id);
    // 多命中时不允许向任意文档写入。
    if (document.state == components::OpaqueTargetMatchState::ambiguous) {
        // 在输出路径检查和 COM 写入前返回稳定歧义错误。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The image document session resolves to multiple documents.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (document.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The image document session no longer exists or changed.",
            nullptr,
        };
    }
    const auto path_validation =
        components::validate_output_path(
            path, extension, overwrite);
    if (!path_validation.ok) {
        return ModuleResult{
            false,
            path_validation.error_code,
            path_validation.error_message,
            nullptr,
        };
    }
    const platform::windows::DiscoveryBackend foreground;
    const std::string before = foreground.foreground_token();
    const auto evidence = capability_id == "artifact.save@1"
        ? backend_.save(*document.position, path)
        : backend_.export_png(*document.position, path);
    const std::string after = foreground.foreground_token();
    if (evidence.error_code.has_value()) {
        return ModuleResult{
            false,
            *evidence.error_code,
            evidence.error_message.value_or(
                "Structured image write failed."),
            nullptr,
            evidence.outcome_unknown
                ? std::optional(components::object({
                      {"outcome", "unknown"},
                      {"retrySafe", false},
                      {"providerMayHaveWritten", true},
                      {"foregroundUnchanged", before == after},
                  }))
                : std::nullopt,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", capability_id},
            {"targetId", session_id},
            {"outputPath", path.string()},
            {"dispatched", evidence.dispatched},
            {"verified", evidence.verified},
            {"foregroundUnchanged", before == after},
            {"currentSessionId",
             evidence.current_session_id.has_value()
                 ? components::Json(*evidence.current_session_id)
                 : components::Json(nullptr)},
        }),
    };
}

}  // namespace act::modules
