#pragma once

#include "components/json.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <utility>

namespace act::components {

struct RecordingConfig {
    std::string output_path;
    std::string analysis_directory;
    std::uint32_t duration_ms;
    std::uint32_t fps;
    std::uint32_t maximum_width;
    std::uint32_t crf;
    std::uint32_t maximum_keyframes;
    double change_threshold;
    std::uint32_t frame_timeout_ms;
    std::uint32_t frame_budget;
    bool overwrite;
};

struct RecordingConfigResult {
    std::optional<RecordingConfig> config;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] RecordingConfigResult parse_recording_config(
    const Json& input);

[[nodiscard]] std::optional<
    std::pair<std::uint32_t, std::uint32_t>>
recording_output_dimensions(
    std::uint32_t source_width,
    std::uint32_t source_height,
    std::uint32_t maximum_width);

}  // namespace act::components
