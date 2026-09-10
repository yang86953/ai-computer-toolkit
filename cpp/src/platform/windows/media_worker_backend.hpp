#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct MediaControls {
    bool play;
    bool pause;
    bool toggle_play_pause;
    bool skip_next;
    bool skip_previous;
};

struct MediaSessionRecord {
    std::string session_id;
    std::string playback_status;
    std::string title;
    std::string artist;
    std::string album_title;
    MediaControls controls;
};

struct MediaSessionSnapshot {
    std::vector<MediaSessionRecord> sessions;
    std::size_t total;
    std::size_t ambiguous_sources_skipped;
    bool truncated;
    bool foreground_unchanged;
};

struct MediaSessionSnapshotResult {
    std::optional<MediaSessionSnapshot> snapshot;
    std::optional<BackendError> error;
};

struct MediaControlEvidence {
    std::string operation;
    bool accepted;
    MediaSessionRecord session;
    bool confirmation_satisfied;
    bool foreground_unchanged;
};

struct MediaControlResult {
    std::optional<MediaControlEvidence> evidence;
    std::optional<BackendError> error;
};

class MediaWorkerBackend final {
public:
    [[nodiscard]] MediaSessionSnapshotResult read_sessions(
        std::size_t maximum_items,
        const std::optional<std::string>& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] MediaControlResult control_session(
        const std::string& session_id,
        const std::string& operation,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
