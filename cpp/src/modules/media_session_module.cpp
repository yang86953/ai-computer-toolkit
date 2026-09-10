#include "modules/media_session_module.hpp"

#include "components/companion_file.hpp"
#include "components/json.hpp"

#include <array>
#include <algorithm>
#include <optional>
#include <string_view>

namespace act::modules {
namespace {

components::Json session_json(
    const platform::windows::MediaSessionRecord& session) {
    return components::object({
        {"sessionId", session.session_id},
        {"targetKind", "media-session"},
        {"playbackStatus", session.playback_status},
        {"title", session.title},
        {"artist", session.artist},
        {"albumTitle", session.album_title},
        {"availableControls",
         components::object({
             {"play", session.controls.play},
             {"pause", session.controls.pause},
             {"togglePlayPause",
              session.controls.toggle_play_pause},
             {"skipNext", session.controls.skip_next},
             {"skipPrevious",
              session.controls.skip_previous},
         })},
    });
}

ModuleResult failure(
    const platform::windows::BackendError& error) {
    return ModuleResult{
        false, error.code, error.message, nullptr};
}

}  // namespace

ModuleResult MediaSessionModule::status(
    const std::uint32_t timeout_ms) const {
    auto observed = sessions(128U, timeout_ms);
    if (!observed.ok) {
        return observed;
    }
    const auto* count = observed.data.find("count");
    const auto* total = observed.data.find("total");
    const auto* skipped =
        observed.data.find("ambiguousSourcesSkipped");
    if (count == nullptr ||
        total == nullptr ||
        skipped == nullptr) {
        return ModuleResult{
            false,
            "OPERATION_FAILED",
            "Media observation omitted status counts.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "media.session.discover@1"},
            {"readOnly", true},
            {"executionDomain", "isolated-worker"},
            {"backgroundPolicy", "guaranteed"},
            {"sessionCount", *count},
            {"total", *total},
            {"ambiguousSourcesSkipped", *skipped},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
            {"writesEnabled", false},
            {"controlCppStatus", "available-confirmed"},
            {"controlWorkerBundled",
             components::companion_file_exists(
                 "ai-computer-toolkit-media-control-worker.exe")},
            {"controlRequiresConfirmation", true},
        }),
    };
}

ModuleResult MediaSessionModule::sessions(
    const std::size_t maximum_items,
    const std::uint32_t timeout_ms) const {
    if (maximum_items == 0U || maximum_items > 128U ||
        timeout_ms == 0U || timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Media observation requires max-items 1..128 and "
            "timeout-ms 1..30000.",
            nullptr,
        };
    }
    const auto foreground_before = foreground_.foreground_token();
    const auto result = worker_.read_sessions(
        maximum_items, std::nullopt, timeout_ms);
    if (foreground_before != foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during media-session observation.",
            nullptr,
        };
    }
    if (result.error.has_value()) {
        return failure(*result.error);
    }
    const auto& snapshot = *result.snapshot;
    components::Json::Array sessions;
    sessions.reserve(snapshot.sessions.size());
    for (const auto& session : snapshot.sessions) {
        sessions.push_back(session_json(session));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "media.session.discover@1"},
            {"readOnly", true},
            {"executionDomain", "isolated-worker"},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"total",
             static_cast<std::int64_t>(snapshot.total)},
            {"truncated", snapshot.truncated},
            {"ambiguousSourcesSkipped",
             static_cast<std::int64_t>(
                 snapshot.ambiguous_sources_skipped)},
            {"foregroundUnchanged", true},
            {"sessions", components::Json(std::move(sessions))},
        }),
    };
}

ModuleResult MediaSessionModule::inspect(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    if (session_id.empty() ||
        timeout_ms == 0U || timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Media inspect requires an opaque target and timeout-ms "
            "1..30000.",
            nullptr,
        };
    }
    const auto foreground_before = foreground_.foreground_token();
    const auto result = worker_.read_sessions(
        1U, session_id, timeout_ms);
    if (foreground_before != foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during exact media-session inspection.",
            nullptr,
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
            {"capability", "media.playback.state.read@1"},
            {"readOnly", true},
            {"executionDomain", "isolated-worker"},
            {"foregroundUnchanged", true},
            {"session",
             session_json(result.snapshot->sessions.front())},
        }),
    };
}

ModuleResult MediaSessionModule::control(
    const std::string& session_id,
    const std::string& operation,
    const bool confirmed,
    const std::uint32_t timeout_ms) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Media-session control requires explicit confirmation.",
            nullptr,
        };
    }
    constexpr std::array<std::string_view, 5> controls{
        "toggle-play-pause",
        "play",
        "pause",
        "skip-next",
        "skip-previous",
    };
    if (!session_id.starts_with("s2:m:") ||
        std::find(
            controls.begin(), controls.end(), operation) ==
            controls.end() ||
        timeout_ms == 0U || timeout_ms > 30000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Media control requires an opaque s2:m target, certified "
            "operation, and timeout 1..30000.",
            nullptr,
        };
    }
    const auto foreground_before =
        foreground_.foreground_token();
    const auto result = worker_.control_session(
        session_id, operation, timeout_ms);
    if (foreground_before != foreground_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during exact media control.",
            nullptr,
        };
    }
    if (result.error.has_value()) {
        if (result.error->code == "TIMEOUT" ||
            result.error->code == "CANCELLED" ||
            result.error->code ==
                "MEDIA_CONTROL_OUTCOME_UNKNOWN") {
            return ModuleResult{
                false,
                result.error->code,
                result.error->message,
                nullptr,
                components::object({
                    {"outcome", "unknown"},
                    {"retrySafe", false},
                    {"acceptedMayHaveOccurred", true},
                }),
            };
        }
        return failure(*result.error);
    }
    const auto& evidence = *result.evidence;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "media.playback.control@1"},
            {"targetId", session_id},
            {"targetKind", "media-session"},
            {"operation", operation},
            {"executionDomain", "isolated-worker"},
            {"accepted", evidence.accepted},
            {"confirmationRequired", true},
            {"confirmationSatisfied",
             evidence.confirmation_satisfied},
            {"foregroundUnchanged",
             evidence.foreground_unchanged},
            {"session", session_json(evidence.session)},
            {"sessionObservation", "before-control"},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

}  // namespace act::modules
