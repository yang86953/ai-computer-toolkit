#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct PngOutputPlan {
    std::string normalized_path;
    bool target_exists;
};

struct PngOutputPlanResult {
    std::optional<PngOutputPlan> plan;
    std::string error_code;
    std::string error_message;
};

struct PngFileOutput {
    std::string normalized_path;
    std::int64_t bytes_written;
    bool replaced_existing;
};

struct PngFileOutputResult {
    std::optional<PngFileOutput> output;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] PngOutputPlanResult validate_png_output_path(
    const std::string& output_path,
    bool overwrite);

[[nodiscard]] PngFileOutputResult write_png_atomically(
    const std::string& output_path,
    const std::vector<std::uint8_t>& png,
    bool overwrite);

}  // namespace act::platform::windows
