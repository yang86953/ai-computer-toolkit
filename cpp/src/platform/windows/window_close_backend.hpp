#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <optional>

namespace act::platform::windows {

struct WindowCloseResult {
    bool closed;
    std::optional<BackendError> error;
};

class WindowCloseBackend final {
public:
    [[nodiscard]] WindowCloseResult close(
        const WindowRecord& target,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
