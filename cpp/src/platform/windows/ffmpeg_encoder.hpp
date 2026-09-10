#pragma once

#include "components/pixel_buffer.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <vector>

namespace act::platform::windows {

struct FfmpegEncodeConfig {
    std::uint32_t width;
    std::uint32_t height;
    std::uint32_t fps;
    std::uint32_t crf;
    std::uint32_t timeout_ms;
    std::filesystem::path staging_directory;
};

struct FfmpegEncodeEvidence {
    std::filesystem::path output_path;
    std::uint64_t output_bytes;
    std::uint32_t encoded_frames;
    bool fixed_arguments;
    bool h264_requested;
    bool mp4_signature_valid;
    bool audio_captured;
    bool cursor_captured;
    bool raw_staging_removed;
};

struct FfmpegEncodeResult {
    std::optional<FfmpegEncodeEvidence> evidence;
    std::optional<BackendError> error;
};

using FfmpegFrameSink =
    std::function<bool(const components::RgbaImage&)>;
using FfmpegFrameProducer =
    std::function<std::optional<BackendError>(
        const FfmpegFrameSink&)>;

[[nodiscard]] FfmpegEncodeResult encode_fixture_video(
    const FfmpegEncodeConfig& config,
    const std::vector<components::RgbaImage>& frames);
[[nodiscard]] FfmpegEncodeResult encode_video_stream(
    const FfmpegEncodeConfig& config,
    std::uint32_t expected_frames,
    const FfmpegFrameProducer& producer);

}  // namespace act::platform::windows
