#pragma once

#include <filesystem>
#include <string>
#include <string_view>

namespace act::components {

struct OutputPathValidation {
    bool ok;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] OutputPathValidation validate_output_path(
    const std::filesystem::path& path,
    std::string_view required_extension,
    bool overwrite);

[[nodiscard]] bool is_non_empty_regular_file(
    const std::filesystem::path& path);

}  // namespace act::components
