#pragma once

#include <cstddef>
#include <string>
#include <vector>

namespace act::platform::windows {

enum class ApplicationLaunchProvider {
    none,
    shell_item,
};

struct InstalledApplicationRecord {
    std::string session_id;
    std::string display_name;
    std::string version;
    std::string publisher;
    std::vector<std::string> discovery_sources;

    // Normalized internal hints used only for conservative exact matching.
    std::vector<std::string> process_match_hints;

    // Backend-only public-Shell identity. Never exposed in JSON or accepted
    // from a caller.
    ApplicationLaunchProvider launch_provider;
    std::string launch_identity;
};

struct InstalledApplicationInventory {
    std::vector<InstalledApplicationRecord> records;
    bool registry_source_available;
    bool complete;
};

class InstalledApplicationBackend final {
public:
    [[nodiscard]] InstalledApplicationInventory enumerate_applications(
        std::size_t maximum_items) const;
};

}  // namespace act::platform::windows
