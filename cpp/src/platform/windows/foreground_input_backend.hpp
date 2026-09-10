#pragma once

#include "components/key_chord.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <optional>

namespace act::platform::windows {

struct ForegroundKeyEvidence {
    bool target_acquired_before_dispatch;
    bool full_input_dispatched;
    bool target_still_foreground_after;
};

struct ForegroundKeyResult {
    std::optional<ForegroundKeyEvidence> evidence;
    std::optional<BackendError> error;
};

class ForegroundInputBackend final {
public:
    [[nodiscard]] ForegroundKeyResult press_key(
        const WindowRecord& window,
        const components::KeyChord& chord) const;
};

}  // namespace act::platform::windows
