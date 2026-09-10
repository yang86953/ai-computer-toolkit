#pragma once

namespace act::platform::windows {

struct EnvironmentCapabilityFacts {
    bool chromium_runtime_detected;
    bool notepad_runtime_detected;
    bool ffmpeg_runtime_detected;
    bool foreground_unchanged;
};

class EnvironmentCapabilityBackend final {
public:
    [[nodiscard]] EnvironmentCapabilityFacts inspect() const;
};

}  // namespace act::platform::windows
