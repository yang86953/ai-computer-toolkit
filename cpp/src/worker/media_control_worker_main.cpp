#include "components/json.hpp"
#include "components/opaque_id.hpp"

#include <windows.h>

#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Media.Control.h>
#include <winrt/base.h>

#include <iostream>
#include <string>
#include <vector>

namespace {

constexpr const char* worker_contract =
    "act/media-control-worker/v1";

using Session = winrt::Windows::Media::Control::
    GlobalSystemMediaTransportControlsSession;

struct SessionRecord {
    std::string session_id;
    std::string playback_status;
    std::string title;
    std::string artist;
    std::string album_title;
    bool supports_play;
    bool supports_pause;
    bool supports_toggle;
    bool supports_next;
    bool supports_previous;
};

int write_failure(
    const char* code,
    const std::string& message) {
    std::cout << act::components::object({
        {"ok", false},
        {"contractVersion", worker_contract},
        {"error",
         act::components::object({
             {"code", code},
             {"message", message},
         })},
    }).dump() << '\n';
    return 2;
}

std::string playback_status(
    const winrt::Windows::Media::Control::
        GlobalSystemMediaTransportControlsSessionPlaybackStatus status) {
    using Status = winrt::Windows::Media::Control::
        GlobalSystemMediaTransportControlsSessionPlaybackStatus;
    switch (status) {
        case Status::Closed:
            return "closed";
        case Status::Opened:
            return "opened";
        case Status::Changing:
            return "changing";
        case Status::Stopped:
            return "stopped";
        case Status::Playing:
            return "playing";
        case Status::Paused:
            return "paused";
        default:
            return "unknown";
    }
}

SessionRecord describe_session(const Session& session) {
    const std::string source =
        winrt::to_string(session.SourceAppUserModelId());
    const auto playback = session.GetPlaybackInfo();
    const auto controls = playback.Controls();
    const auto properties =
        session.TryGetMediaPropertiesAsync().get();
    return SessionRecord{
        act::components::opaque_id('m', source),
        playback_status(playback.PlaybackStatus()),
        winrt::to_string(properties.Title()),
        winrt::to_string(properties.Artist()),
        winrt::to_string(properties.AlbumTitle()),
        controls.IsPlayEnabled(),
        controls.IsPauseEnabled(),
        controls.IsPlayPauseToggleEnabled(),
        controls.IsNextEnabled(),
        controls.IsPreviousEnabled(),
    };
}

act::components::Json session_json(
    const SessionRecord& record) {
    return act::components::object({
        {"sessionId", record.session_id},
        {"targetKind", "media-session"},
        {"playbackStatus", record.playback_status},
        {"title", record.title},
        {"artist", record.artist},
        {"albumTitle", record.album_title},
        {"availableControls",
         act::components::object({
             {"play", record.supports_play},
             {"pause", record.supports_pause},
             {"togglePlayPause", record.supports_toggle},
             {"skipNext", record.supports_next},
             {"skipPrevious", record.supports_previous},
         })},
    });
}

bool known_control(const std::string& operation) {
    return operation == "toggle-play-pause" ||
           operation == "play" ||
           operation == "pause" ||
           operation == "skip-next" ||
           operation == "skip-previous";
}

bool supported_control(
    const SessionRecord& session,
    const std::string& operation) {
    if (operation == "toggle-play-pause") {
        return session.supports_toggle;
    }
    if (operation == "play") {
        return session.supports_play;
    }
    if (operation == "pause") {
        return session.supports_pause;
    }
    if (operation == "skip-next") {
        return session.supports_next;
    }
    return session.supports_previous;
}

bool execute_control(
    const Session& session,
    const std::string& operation) {
    if (operation == "toggle-play-pause") {
        return session.TryTogglePlayPauseAsync().get();
    }
    if (operation == "play") {
        return session.TryPlayAsync().get();
    }
    if (operation == "pause") {
        return session.TryPauseAsync().get();
    }
    if (operation == "skip-next") {
        return session.TrySkipNextAsync().get();
    }
    return session.TrySkipPreviousAsync().get();
}

int run(const act::components::Json& request) {
    const auto* confirmed = request.find("confirmed");
    if (confirmed == nullptr ||
        confirmed->bool_value() == nullptr ||
        !*confirmed->bool_value()) {
        return write_failure(
            "CONFIRMATION_REQUIRED",
            "Media control requires explicit confirmation.");
    }
    const auto* target_value = request.find("sessionId");
    const auto* operation_value = request.find("control");
    if (target_value == nullptr ||
        target_value->string_value() == nullptr ||
        !target_value->string_value()->starts_with("s2:m:") ||
        operation_value == nullptr ||
        operation_value->string_value() == nullptr ||
        !known_control(*operation_value->string_value())) {
        return write_failure(
            "INVALID_ARGUMENT",
            "Media control requires an exact opaque target and "
            "certified operation.");
    }

    const HWND foreground_before = GetForegroundWindow();
    using Manager = winrt::Windows::Media::Control::
        GlobalSystemMediaTransportControlsSessionManager;
    const auto manager = Manager::RequestAsync().get();
    const auto sessions = manager.GetSessions();
    std::vector<Session> matches;
    for (const auto& session : sessions) {
        const std::string source =
            winrt::to_string(session.SourceAppUserModelId());
        if (act::components::opaque_id('m', source) ==
            *target_value->string_value()) {
            matches.push_back(session);
        }
    }
    if (matches.empty()) {
        return write_failure(
            "STALE_SESSION",
            "The opaque media session no longer resolves.");
    }
    if (matches.size() != 1U) {
        return write_failure(
            "AMBIGUOUS_TARGET",
            "Multiple media sessions resolve to one opaque identity.");
    }

    const auto& session = matches.front();
    const auto observed_before = describe_session(session);
    const std::string& operation =
        *operation_value->string_value();
    if (!supported_control(observed_before, operation)) {
        return write_failure(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact media session does not expose this control.");
    }
    bool accepted = false;
    try {
        accepted = execute_control(session, operation);
    } catch (const winrt::hresult_error&) {
        return write_failure(
            "MEDIA_CONTROL_OUTCOME_UNKNOWN",
            "The media provider failed after control dispatch; "
            "automatic retry is unsafe.");
    }
    if (foreground_before != GetForegroundWindow()) {
        return write_failure(
            "MEDIA_CONTROL_OUTCOME_UNKNOWN",
            "Foreground changed after control dispatch; automatic "
            "retry is unsafe.");
    }
    if (!accepted) {
        return write_failure(
            "MEDIA_OPERATION_REJECTED",
            "The system media session rejected the control.");
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"operation", operation},
             {"accepted", true},
             {"session", session_json(observed_before)},
             {"sessionObservation", "before-control"},
             {"confirmationSatisfied", true},
             {"foregroundUnchanged", true},
             {"writeMethodsCalled", true},
         })},
    }).dump() << '\n';
    return 0;
}

}  // namespace

int main() {
    std::string request_text;
    if (!std::getline(std::cin, request_text)) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The media control worker requires one request.");
    }
    std::string parse_error;
    auto request =
        act::components::Json::parse(request_text, parse_error);
    const auto* contract =
        request.has_value()
            ? request->find("contractVersion")
            : nullptr;
    const auto* operation =
        request.has_value() ? request->find("operation") : nullptr;
    if (!request.has_value() ||
        request->object_items() == nullptr ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        operation == nullptr ||
        operation->string_value() == nullptr ||
        *operation->string_value() != "media-control") {
        return write_failure(
            "INVALID_ARGUMENT",
            "The media control request violates protocol v1.");
    }
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        return run(*request);
    } catch (const winrt::hresult_error& error) {
        return write_failure(
            error.code() == E_ACCESSDENIED
                ? "PERMISSION_DENIED"
                : "MEDIA_SESSION_UNAVAILABLE",
            error.code() == E_ACCESSDENIED
                ? "Windows denied media-session access."
                : "Windows media-session access failed before dispatch.");
    }
}
