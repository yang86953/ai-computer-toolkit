#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <string>

namespace act::platform::windows {

class ObservationWorkerBackend final {
public:
    [[nodiscard]] AccessibilityResult read_accessibility_root(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] AccessibilityTreeResult read_accessibility_tree(
        const std::string& session_id,
        std::size_t maximum_depth,
        std::size_t maximum_items,
        const std::string& view,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
