#include "components/output_path_policy.hpp"

#include <system_error>

namespace act::components {

OutputPathValidation validate_output_path(
    const std::filesystem::path& path,
    const std::string_view required_extension,
    const bool overwrite) {
    const std::string expected = "." + std::string(required_extension);
    std::error_code error;
    const bool parent_exists =
        !path.parent_path().empty() &&
        std::filesystem::is_directory(path.parent_path(), error);
    if (!path.is_absolute() ||
        path.extension().string() != expected ||
        !parent_exists || error) {
        return OutputPathValidation{
            false,
            "INVALID_ARGUMENT",
            "Output must be an absolute " + expected +
                " path with an existing parent.",
        };
    }
    const bool exists = std::filesystem::exists(path, error);
    if (error) {
        return OutputPathValidation{
            false,
            "OUTPUT_PATH_UNAVAILABLE",
            "The output path could not be inspected.",
        };
    }
    if (exists && !overwrite) {
        return OutputPathValidation{
            false,
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The output exists; explicitly allow overwrite to replace it.",
        };
    }
    return OutputPathValidation{true, {}, {}};
}

bool is_non_empty_regular_file(
    const std::filesystem::path& path) {
    std::error_code error;
    if (!std::filesystem::is_regular_file(path, error) || error) {
        return false;
    }
    return std::filesystem::file_size(path, error) > 0U && !error;
}

}  // namespace act::components
