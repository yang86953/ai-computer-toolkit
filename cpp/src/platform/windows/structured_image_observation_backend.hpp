#pragma once

#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/structured_image_backend.hpp"

#include <cstdint>
#include <optional>

namespace act::platform::windows {

struct StructuredImageStatusObservation {
    std::optional<StructuredImageStatus> status;
    bool foreground_unchanged;
    std::optional<BackendError> error;
};

struct StructuredImageInventoryObservation {
    std::optional<StructuredImageInventory> inventory;
    bool foreground_unchanged;
    std::optional<BackendError> error;
};

class StructuredImageObservationBackend final {
public:
    [[nodiscard]] StructuredImageStatusObservation status(
        std::uint32_t timeout_ms) const;
    [[nodiscard]] StructuredImageInventoryObservation inventory(
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
