#pragma once

#include "platform/windows/discovery_backend.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct StandardEditRecord {
    std::string session_id;
    std::string application_name;
    bool visible;

    std::uintptr_t native_control;
    std::uint32_t native_process_id;
};

struct StandardEditWriteResult {
    bool verified;
    std::optional<BackendError> error;
};

class StandardEditBackend final {
public:
    [[nodiscard]] std::vector<StandardEditRecord> enumerate(
        std::size_t maximum_items) const;
    [[nodiscard]] StandardEditWriteResult set_text(
        const StandardEditRecord& control,
        const std::wstring& text,
        std::uint32_t timeout_ms) const;
};

}  // namespace act::platform::windows
