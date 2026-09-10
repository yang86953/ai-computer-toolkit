#pragma once

#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/installed_application_backend.hpp"

#include <optional>

namespace act::platform::windows {

struct ApplicationLaunchEvidence {
    bool launch_dispatched;
    bool process_handle_observed;
};

struct ApplicationLaunchResult {
    std::optional<ApplicationLaunchEvidence> evidence;
    std::optional<BackendError> error;
};

class ApplicationLaunchBackend final {
public:
    [[nodiscard]] ApplicationLaunchResult launch(
        const InstalledApplicationRecord& application) const;
};

}  // namespace act::platform::windows
