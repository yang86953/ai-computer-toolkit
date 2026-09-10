#include "components/json.hpp"
#include "components/opaque_id.hpp"

#include <windows.h>

#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Media.Control.h>
#include <winrt/base.h>

#include <algorithm>
#include <cstdint>
#include <iostream>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {

constexpr const char* worker_contract =
    "act/media-observation-worker/v1";
constexpr std::int64_t maximum_sessions = 128;

struct SessionRecord {
    std::string session_id;
    std::string source_identity;
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

SessionRecord describe_session(
    const winrt::Windows::Media::Control::
        GlobalSystemMediaTransportControlsSession& session) {
    const std::string source =
        winrt::to_string(session.SourceAppUserModelId());
    const auto playback = session.GetPlaybackInfo();
    const auto controls = playback.Controls();
    const auto properties =
        session.TryGetMediaPropertiesAsync().get();
    return SessionRecord{
        act::components::opaque_id('m', source),
        source,
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

std::vector<SessionRecord> read_sessions() {
    using Manager = winrt::Windows::Media::Control::
        GlobalSystemMediaTransportControlsSessionManager;
    const auto manager = Manager::RequestAsync().get();
    const auto sessions = manager.GetSessions();
    std::vector<SessionRecord> records;
    records.reserve(sessions.Size());
    for (const auto& session : sessions) {
        records.push_back(describe_session(session));
    }
    return records;
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

int execute(const act::components::Json& request) {
    const auto* maximum_value = request.find("maximumItems");
    if (maximum_value == nullptr ||
        maximum_value->integer_value() == nullptr ||
        *maximum_value->integer_value() <= 0 ||
        *maximum_value->integer_value() > maximum_sessions) {
        return write_failure(
            "INVALID_ARGUMENT",
            "maximumItems must be from 1 through 128.");
    }
    const auto* target_value = request.find("sessionId");
    const std::string* target =
        target_value == nullptr
            ? nullptr
            : target_value->string_value();
    if (target_value != nullptr &&
        (target == nullptr || target->empty())) {
        return write_failure(
            "INVALID_ARGUMENT",
            "sessionId must be a non-empty opaque media target.");
    }

    const HWND foreground_before = GetForegroundWindow();
    std::vector<SessionRecord> records;
    try {
        records = read_sessions();
    } catch (const winrt::hresult_error& error) {
        if (foreground_before != GetForegroundWindow()) {
            return write_failure(
                "HOST_INTERFERENCE_DETECTED",
                "Foreground changed during failed media observation.");
        }
        return write_failure(
            error.code() == E_ACCESSDENIED
                ? "PERMISSION_DENIED"
                : "MEDIA_SESSION_UNAVAILABLE",
            error.code() == E_ACCESSDENIED
                ? "Windows denied media-session observation."
                : "Windows media-session observation failed.");
    }
    if (foreground_before != GetForegroundWindow()) {
        return write_failure(
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during media-session observation.");
    }
    std::unordered_map<std::string, std::size_t> source_counts;
    for (const auto& record : records) {
        ++source_counts[record.source_identity];
    }

    std::size_t ambiguous_sources = 0U;
    for (const auto& [source, count] : source_counts) {
        static_cast<void>(source);
        if (count > 1U) {
            ++ambiguous_sources;
        }
    }
    if (target != nullptr) {
        const auto matching = std::count_if(
            records.begin(),
            records.end(),
            [target](const auto& record) {
                return record.session_id == *target;
            });
        if (matching == 0) {
            return write_failure(
                "STALE_SESSION",
                "The opaque media session no longer resolves.");
        }
        if (matching > 1) {
            return write_failure(
                "AMBIGUOUS_TARGET",
                "The provider exposes multiple sessions for one identity.");
        }
        std::erase_if(
            records,
            [target](const auto& record) {
                return record.session_id != *target;
            });
    } else {
        std::erase_if(
            records,
            [&source_counts](const auto& record) {
                return source_counts[record.source_identity] > 1U;
            });
    }

    const std::size_t total = records.size();
    const std::size_t limit = static_cast<std::size_t>(
        *maximum_value->integer_value());
    if (records.size() > limit) {
        records.resize(limit);
    }
    act::components::Json::Array items;
    items.reserve(records.size());
    for (const auto& record : records) {
        items.push_back(session_json(record));
    }
    if (foreground_before != GetForegroundWindow()) {
        return write_failure(
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during media-session observation.");
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"sessions", act::components::Json(std::move(items))},
             {"count", static_cast<std::int64_t>(records.size())},
             {"total", static_cast<std::int64_t>(total)},
             {"truncated", total > records.size()},
             {"ambiguousSourcesSkipped",
              static_cast<std::int64_t>(ambiguous_sources)},
             {"foregroundUnchanged", true},
             {"metadataRead", true},
             {"writeMethodsCalled", false},
         })},
    }).dump() << '\n';
    return 0;
}

int execute_delay_fixture(
    const act::components::Json& request) {
    const auto* delay = request.find("delayMilliseconds");
    if (delay == nullptr ||
        delay->integer_value() == nullptr ||
        *delay->integer_value() <= 0 ||
        *delay->integer_value() > 1000) {
        return write_failure(
            "INVALID_ARGUMENT",
            "delayMilliseconds must be from 1 through 1000.");
    }
    Sleep(static_cast<DWORD>(*delay->integer_value()));
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"fixtureOwnedByToolkit", true},
             {"writeMethodsCalled", false},
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
            "The media observation worker requires one request.");
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
        (*operation->string_value() != "media-sessions-read" &&
         *operation->string_value() != "fixture-delay")) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The media observation request violates protocol v1.");
    }
    if (*operation->string_value() == "fixture-delay") {
        return execute_delay_fixture(*request);
    }
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        return execute(*request);
    } catch (const winrt::hresult_error& error) {
        return write_failure(
            error.code() == E_ACCESSDENIED
                ? "PERMISSION_DENIED"
                : "MEDIA_SESSION_UNAVAILABLE",
            error.code() == E_ACCESSDENIED
                ? "Windows denied media-session observation."
                : "Windows media-session observation failed.");
    }
}
