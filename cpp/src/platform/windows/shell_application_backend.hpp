#pragma once

#include "platform/windows/installed_application_backend.hpp"

#include <cstddef>
#include <vector>

namespace act::platform::windows {

struct ShellApplicationInventory {
    std::vector<InstalledApplicationRecord> records;
    bool source_available;
    bool complete;
};

class ShellApplicationBackend final {
public:
    [[nodiscard]] ShellApplicationInventory enumerate_applications(
        std::size_t maximum_items) const;
};

}  // namespace act::platform::windows
