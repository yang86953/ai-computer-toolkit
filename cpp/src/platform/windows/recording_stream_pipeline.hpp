#pragma once

#include "components/recording_analysis.hpp"
#include "platform/windows/ffmpeg_encoder.hpp"

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>

namespace act::platform::windows {

struct RecordingStreamPipelineConfig {
    std::uint32_t frame_count;
    std::uint32_t interval_ms;
    std::uint32_t fps;
    std::uint32_t maximum_width;
    std::uint32_t crf;
    std::uint32_t maximum_keyframes;
    double change_threshold;
};

struct RecordingStreamPipelineEvidence {
    FfmpegEncodeEvidence encode;
    components::RecordingAnalysis analysis;
    std::int32_t width;
    std::int32_t height;
    std::uint32_t captured_frames;
    std::string device_driver;
    bool single_capture_session;
    bool foreground_unchanged;
};

struct RecordingStreamPipelineResult {
    std::optional<RecordingStreamPipelineEvidence> evidence;
    std::optional<BackendError> error;
};

[[nodiscard]] RecordingStreamPipelineResult
run_recording_stream_pipeline(
    std::uintptr_t native_window,
    std::int32_t source_width,
    std::int32_t source_height,
    const std::filesystem::path& staging_directory,
    const RecordingStreamPipelineConfig& config);

}  // namespace act::platform::windows
