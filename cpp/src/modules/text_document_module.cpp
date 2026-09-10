#include "modules/text_document_module.hpp"

#include "components/opaque_id.hpp"
#include "components/utf8.hpp"
#include "platform/windows/text_codec.hpp"

namespace act::modules {
namespace {

components::Json capability_descriptor() {
    return components::object({
        {"id", "text.document.create@1"},
        {"version", 1},
        {"verb", "create"},
        {"availability", "available"},
        {"execution", "background-preferred"},
        {"requiresConfirmation", true},
        {"requiresForegroundConsent", false},
        {"inputSchema", "schema://text/document-create/v1"},
        {"constraints",
         components::object({
             {"encoding", "utf-8"},
             {"artifactMediaType", "text/plain"},
             {"required", components::array({"text"})},
         })},
    });
}

}  // namespace

std::string TextDocumentModule::session_id() const {
    return components::opaque_id(
        'a', "text-document-creator");
}

bool TextDocumentModule::owns_session(
    const std::string_view candidate) const {
    return candidate == session_id();
}

bool TextDocumentModule::available() const {
    return backend_.runtime_available();
}

components::Json TextDocumentModule::session_descriptor() const {
    return components::object({
        {"sessionId", session_id()},
        {"kind", "application"},
        {"title", "Text document creator"},
        {"state", available() ? "available" : "unavailable"},
        {"capabilities",
         components::Json(
             components::Json::Array{capability_descriptor()})},
    });
}

ModuleResult TextDocumentModule::inspect(
    const std::string& candidate) const {
    if (!owns_session(candidate)) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The text document application session is unavailable.",
            nullptr,
        };
    }
    if (!available()) {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The system text document application is unavailable.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"kind", "application"},
            {"state", "available"},
            {"capabilities",
             components::array({"text.document.create@1"})},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

ModuleResult TextDocumentModule::create(
    const std::string_view text,
    const bool confirmed) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "State-changing operations require explicit --confirm.",
            nullptr,
        };
    }
    if (text.size() > 1048576U ||
        !components::valid_utf8(text)) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Text must be valid UTF-8 no larger than 1 MiB.",
            nullptr,
        };
    }
    if (backend_.existing_notepad_process()) {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "An existing Notepad process may absorb the new document; "
            "attachment to existing application sessions is not certified.",
            nullptr,
            components::object({
                {"reason",
                 "existing-application-session-attachment-not-certified"},
                {"artifactCreated", false},
                {"foregroundChanged", false},
                {"safeToRetryAutomatically", false},
            }),
        };
    }
    auto artifact =
        backend_.create_temporary_artifact(text);
    if (!artifact.ok) {
        return ModuleResult{
            false,
            std::move(artifact.error_code),
            std::move(artifact.error_message),
            nullptr,
        };
    }
    auto launch = backend_.open_in_notepad(artifact.path);
    if (!launch.ok) {
        backend_.remove_owned_artifact(artifact.path);
        return ModuleResult{
            false,
            std::move(launch.error_code),
            std::move(launch.error_message),
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "text.document.create@1"},
            {"path",
             platform::windows::utf8(
                 artifact.path.native())},
            {"text", std::move(artifact.verified_text)},
            {"launcherProcessId",
             static_cast<std::int64_t>(launch.process_id)},
            {"foregroundUnchanged",
             launch.foreground_unchanged},
            {"encoding", "utf-8"},
            {"mediaType", "text/plain"},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

}  // namespace act::modules
