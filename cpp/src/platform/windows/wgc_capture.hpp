#pragma once

#include "components/pixel_buffer.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct CaptureFrameMetadata {
    std::int32_t width;
    std::int32_t height;
    std::string device_driver;
    bool fixture_owned_by_toolkit;
    bool frame_surface_accessed;
    bool pixels_persisted;
    bool file_written;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
    std::int64_t pixel_bytes_read;
    std::int64_t row_pitch;
    std::string pixel_digest;
    std::string pixel_format;
    bool png_encoded;
    std::int64_t png_bytes;
    std::string png_digest;
    bool png_signature_valid;
    std::vector<std::uint8_t> encoded_png;
    std::string output_path;
    bool replaced_existing;
};

struct CaptureFrameResult {
    std::optional<CaptureFrameMetadata> metadata;
    std::optional<BackendError> error;
};

struct RecordingProbeMetadata {
    std::int32_t width;
    std::int32_t height;
    std::uint32_t frames_requested;
    std::uint32_t frames_captured;
    std::uint32_t distinct_frames;
    std::string device_driver;
    std::vector<std::string> pixel_digests;
    bool fixture_owned_by_toolkit;
    bool single_capture_session;
    bool frame_surfaces_accessed;
    bool pixels_persisted;
    bool file_written;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
};

struct RecordingProbeResult {
    std::optional<RecordingProbeMetadata> metadata;
    std::optional<BackendError> error;
};

struct RecordingFrameSequence {
    std::int32_t width;
    std::int32_t height;
    std::string device_driver;
    std::vector<components::RgbaImage> frames;
    bool fixture_owned_by_toolkit;
    bool single_capture_session;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
};

struct RecordingFrameSequenceResult {
    std::optional<RecordingFrameSequence> sequence;
    std::optional<BackendError> error;
};

struct RecordingStreamMetadata {
    std::int32_t width;
    std::int32_t height;
    std::uint32_t frames_captured;
    std::string device_driver;
    bool fixture_owned_by_toolkit;
    bool single_capture_session;
    bool foreground_unchanged;
    bool privacy_indicator_may_have_appeared;
};

struct RecordingStreamResult {
    std::optional<RecordingStreamMetadata> metadata;
    std::optional<BackendError> error;
};

using RecordingFrameConsumer =
    std::function<bool(components::RgbaImage&&)>;

[[nodiscard]] CaptureFrameResult capture_fixture_frame();
[[nodiscard]] CaptureFrameResult capture_fixture_surface_readback();
[[nodiscard]] CaptureFrameResult capture_fixture_memory_png();
[[nodiscard]] RecordingProbeResult capture_fixture_recording_probe(
    std::uint32_t frame_count,
    std::uint32_t interval_ms);
[[nodiscard]] RecordingFrameSequenceResult
capture_fixture_recording_frames(
    std::uint32_t frame_count,
    std::uint32_t interval_ms);
[[nodiscard]] RecordingFrameSequenceResult
capture_window_recording_frames(
    std::uintptr_t native_window,
    std::uint32_t frame_count,
    std::uint32_t interval_ms);
[[nodiscard]] RecordingStreamResult
capture_window_recording_stream(
    std::uintptr_t native_window,
    std::uint32_t frame_count,
    std::uint32_t interval_ms,
    const RecordingFrameConsumer& consumer);
[[nodiscard]] CaptureFrameResult capture_window_frame(
    std::uintptr_t native_window);
[[nodiscard]] CaptureFrameResult capture_window_png_file(
    std::uintptr_t native_window,
    const std::string& output_path,
    bool overwrite);

}  // namespace act::platform::windows
