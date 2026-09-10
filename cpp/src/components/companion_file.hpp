#pragma once

#include <optional>
#include <string>

namespace act::components {

[[nodiscard]] std::optional<std::string> read_companion_text(
    const std::string& filename);
[[nodiscard]] bool companion_file_exists(
    const std::string& filename);

}  // namespace act::components
