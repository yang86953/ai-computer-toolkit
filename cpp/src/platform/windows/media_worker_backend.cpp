#include "platform/windows/media_worker_backend.hpp"

#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <algorithm>
#include <limits>
#include <string_view>

namespace act::platform::windows {
namespace {

constexpr const char* observation_worker_contract =
    "act/media-observation-worker/v1";
constexpr const char* observation_worker_name =
    "ai-computer-toolkit-media-worker.exe";
constexpr const char* control_worker_contract =
    "act/media-control-worker/v1";
constexpr const char* control_worker_name =
    "ai-computer-toolkit-media-control-worker.exe";

BackendError process_error(
    const components::WorkerProcessResult& result) {
    using Completion = components::WorkerCompletion;
    switch (result.completion) {
        case Completion::timed_out:
            return BackendError{"TIMEOUT", result.error_message};
        case Completion::cancelled:
            return BackendError{"CANCELLED", result.error_message};
        case Completion::unavailable:
            return BackendError{
                "ISOLATED_WORKER_UNAVAILABLE", result.error_message};
        case Completion::protocol_failure:
            return BackendError{
                "OPERATION_FAILED", result.error_message};
        case Completion::completed:
            break;
    }
    return BackendError{
        "OPERATION_FAILED",
        "The isolated media worker returned an invalid result.",
    };
}

MediaSessionSnapshotResult failure(
    const char* message) {
    return MediaSessionSnapshotResult{
        std::nullopt,
        BackendError{"OPERATION_FAILED", message},
    };
}

bool read_string(
    const components::Json& object,
    const std::string_view key,
    std::string& output) {
    const auto* value = object.find(key);
    if (value == nullptr || value->string_value() == nullptr) {
        return false;
    }
    output = *value->string_value();
    return true;
}

bool read_bool(
    const components::Json& object,
    const std::string_view key,
    bool& output) {
    const auto* value = object.find(key);
    if (value == nullptr || value->bool_value() == nullptr) {
        return false;
    }
    output = *value->bool_value();
    return true;
}

bool read_size(
    const components::Json& object,
    const std::string_view key,
    std::size_t& output) {
    const auto* value = object.find(key);
    if (value == nullptr ||
        value->integer_value() == nullptr ||
        *value->integer_value() < 0 ||
        static_cast<std::uint64_t>(*value->integer_value()) >
            std::numeric_limits<std::size_t>::max()) {
        return false;
    }
    output = static_cast<std::size_t>(*value->integer_value());
    return true;
}

bool valid_opaque_media_id(
    const std::string_view value) {
    if (!value.starts_with("s2:m:") || value.size() != 21U) {
        return false;
    }
    return std::all_of(
        value.begin() + 5,
        value.end(),
        [](const unsigned char character) {
            return (character >= '0' && character <= '9') ||
                   (character >= 'a' && character <= 'f');
        });
}

bool valid_playback_status(
    const std::string_view value) {
    constexpr std::string_view values[]{
        "closed",
        "opened",
        "changing",
        "stopped",
        "playing",
        "paused",
        "unknown",
    };
    return std::find(std::begin(values), std::end(values), value) !=
           std::end(values);
}

std::optional<BackendError> parse_envelope(
    const components::WorkerProcessResult& process,
    const std::string_view expected_contract,
    components::Json& response,
    const components::Json*& data) {
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return process_error(process);
    }
    std::string parse_error;
    auto parsed =
        components::Json::parse(process.stdout_text, parse_error);
    if (!parsed.has_value()) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated media worker returned malformed JSON.",
        };
    }
    response = std::move(*parsed);
    const auto* contract = response.find("contractVersion");
    const auto* ok = response.find("ok");
    if (contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != expected_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated media worker violated its envelope.",
        };
    }
    if (!*ok->bool_value()) {
        const auto* error = response.find("error");
        const auto* code =
            error == nullptr ? nullptr : error->find("code");
        const auto* message =
            error == nullptr ? nullptr : error->find("message");
        if (code == nullptr ||
            code->string_value() == nullptr ||
            message == nullptr ||
            message->string_value() == nullptr) {
            return BackendError{
                "OPERATION_FAILED",
                "The isolated media worker returned an invalid error.",
            };
        }
        return BackendError{
            *code->string_value(), *message->string_value()};
    }
    data = response.find("data");
    if (data == nullptr || data->object_items() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated media worker omitted result data.",
        };
    }
    return std::nullopt;
}

bool decode_controls(
    const components::Json& value,
    MediaControls& controls) {
    return value.object_items() != nullptr &&
           value.object_items()->size() == 5U &&
           read_bool(value, "play", controls.play) &&
           read_bool(value, "pause", controls.pause) &&
           read_bool(
               value,
               "togglePlayPause",
               controls.toggle_play_pause) &&
           read_bool(value, "skipNext", controls.skip_next) &&
           read_bool(
               value, "skipPrevious", controls.skip_previous);
}

bool decode_session(
    const components::Json& value,
    MediaSessionRecord& record) {
    const auto* target_kind = value.find("targetKind");
    const auto* controls = value.find("availableControls");
    return value.object_items() != nullptr &&
           value.object_items()->size() == 7U &&
           read_string(value, "sessionId", record.session_id) &&
           valid_opaque_media_id(record.session_id) &&
           target_kind != nullptr &&
           target_kind->string_value() != nullptr &&
           *target_kind->string_value() == "media-session" &&
           read_string(
               value,
               "playbackStatus",
               record.playback_status) &&
           valid_playback_status(record.playback_status) &&
           read_string(value, "title", record.title) &&
           read_string(value, "artist", record.artist) &&
           read_string(
               value, "albumTitle", record.album_title) &&
           controls != nullptr &&
           decode_controls(*controls, record.controls);
}

}  // namespace

MediaSessionSnapshotResult MediaWorkerBackend::read_sessions(
    const std::size_t maximum_items,
    const std::optional<std::string>& session_id,
    const std::uint32_t timeout_ms) const {
    components::Json::Object request_fields{
        {"contractVersion", observation_worker_contract},
        {"operation", "media-sessions-read"},
        {"maximumItems",
         static_cast<std::int64_t>(maximum_items)},
    };
    if (session_id.has_value()) {
        request_fields.emplace_back("sessionId", *session_id);
    }
    const auto process = components::WorkerProcess().run_companion(
        observation_worker_name,
        components::Json(std::move(request_fields)).dump(),
        timeout_ms,
        4U * 1024U * 1024U);
    components::Json response;
    const components::Json* data = nullptr;
    const auto envelope_error =
        parse_envelope(
            process,
            observation_worker_contract,
            response,
            data);
    if (envelope_error.has_value()) {
        return MediaSessionSnapshotResult{
            std::nullopt, envelope_error};
    }

    const auto* session_value = data->find("sessions");
    const auto* sessions =
        session_value == nullptr
            ? nullptr
            : session_value->array_items();
    std::size_t count = 0U;
    std::size_t total = 0U;
    std::size_t ambiguous = 0U;
    bool truncated = false;
    bool foreground_unchanged = false;
    bool metadata_read = false;
    bool write_methods_called = true;
    if (data->object_items()->size() != 8U ||
        sessions == nullptr ||
        !read_size(*data, "count", count) ||
        !read_size(*data, "total", total) ||
        !read_size(
            *data, "ambiguousSourcesSkipped", ambiguous) ||
        !read_bool(*data, "truncated", truncated) ||
        !read_bool(
            *data,
            "foregroundUnchanged",
            foreground_unchanged) ||
        !read_bool(*data, "metadataRead", metadata_read) ||
        !read_bool(
            *data, "writeMethodsCalled", write_methods_called) ||
        count != sessions->size() ||
        count > maximum_items ||
        total < count ||
        truncated != (total > count) ||
        !foreground_unchanged ||
        !metadata_read ||
        write_methods_called) {
        return failure(
            "The isolated media worker violated its safety result.");
    }

    std::vector<MediaSessionRecord> decoded;
    decoded.reserve(sessions->size());
    for (const auto& value : *sessions) {
        MediaSessionRecord record{};
        if (!decode_session(value, record)) {
            return failure(
                "The isolated media worker returned an invalid session.");
        }
        decoded.push_back(std::move(record));
    }
    if (session_id.has_value() &&
        (decoded.size() != 1U ||
         decoded.front().session_id != *session_id)) {
        return failure(
            "The isolated media worker did not resolve the exact target.");
    }
    return MediaSessionSnapshotResult{
        MediaSessionSnapshot{
            std::move(decoded),
            total,
            ambiguous,
            truncated,
            foreground_unchanged,
        },
        std::nullopt,
    };
}

MediaControlResult MediaWorkerBackend::control_session(
    const std::string& session_id,
    const std::string& operation,
    const std::uint32_t timeout_ms) const {
    const components::Json request(components::Json::Object{
        {"contractVersion", control_worker_contract},
        {"operation", "media-control"},
        {"sessionId", session_id},
        {"control", operation},
        {"confirmed", true},
    });
    const auto process = components::WorkerProcess().run_companion(
        control_worker_name,
        request.dump(),
        timeout_ms,
        4U * 1024U * 1024U);
    components::Json response;
    const components::Json* data = nullptr;
    const auto envelope_error =
        parse_envelope(
            process,
            control_worker_contract,
            response,
            data);
    if (envelope_error.has_value()) {
        return MediaControlResult{
            std::nullopt, envelope_error};
    }
    std::string returned_operation;
    bool accepted = false;
    bool confirmed = false;
    bool foreground = false;
    bool write_called = false;
    std::string observation_timing;
    const auto* session_value = data->find("session");
    MediaSessionRecord session{};
    if (data->object_items()->size() != 7U ||
        !read_string(
            *data, "operation", returned_operation) ||
        returned_operation != operation ||
        !read_bool(*data, "accepted", accepted) ||
        !read_bool(
            *data, "confirmationSatisfied", confirmed) ||
        !read_bool(
            *data, "foregroundUnchanged", foreground) ||
        !read_bool(
            *data, "writeMethodsCalled", write_called) ||
        !read_string(
            *data, "sessionObservation", observation_timing) ||
        observation_timing != "before-control" ||
        session_value == nullptr ||
        !decode_session(*session_value, session) ||
        session.session_id != session_id ||
        !accepted || !confirmed || !foreground ||
        !write_called) {
        return MediaControlResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated media worker violated its control result.",
            },
        };
    }
    return MediaControlResult{
        MediaControlEvidence{
            returned_operation,
            true,
            std::move(session),
            true,
            true,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
