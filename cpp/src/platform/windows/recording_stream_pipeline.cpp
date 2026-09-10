#include "platform/windows/recording_stream_pipeline.hpp"

#include "components/recording_config.hpp"
#include "platform/windows/wgc_capture.hpp"

namespace act::platform::windows {
namespace {

RecordingStreamPipelineResult failure(
    const BackendError& error) {
    return RecordingStreamPipelineResult{
        std::nullopt, error};
}

}  // namespace

RecordingStreamPipelineResult run_recording_stream_pipeline(
    const std::uintptr_t native_window,
    const std::int32_t source_width,
    const std::int32_t source_height,
    const std::filesystem::path& staging_directory,
    const RecordingStreamPipelineConfig& config) {
    if (source_width < 2 || source_height < 2) {
        return failure(BackendError{
            "CAPTURE_TARGET_FAILED",
            "The preflight recording dimensions are invalid."});
    }
    const auto dimensions =
        components::recording_output_dimensions(
            static_cast<std::uint32_t>(source_width),
            static_cast<std::uint32_t>(source_height),
            config.maximum_width);
    if (!dimensions.has_value()) {
        return failure(BackendError{
            "CAPTURE_TARGET_FAILED",
            "The recording output dimensions are invalid."});
    }
    components::RecordingAnalysisAccumulator analysis(
        config.fps,
        config.maximum_keyframes,
        config.change_threshold);
    std::optional<RecordingStreamMetadata> capture_metadata;
    std::optional<BackendError> producer_error;
    auto encoded = encode_video_stream(
        FfmpegEncodeConfig{
            dimensions->first,
            dimensions->second,
            config.fps,
            config.crf,
            30000U,
            staging_directory,
        },
        config.frame_count,
        [&](const FfmpegFrameSink& sink)
            -> std::optional<BackendError> {
            auto captured = capture_window_recording_stream(
                native_window,
                config.frame_count,
                config.interval_ms,
                [&](components::RgbaImage&& frame) {
                    if (frame.width != dimensions->first ||
                        frame.height != dimensions->second) {
                        auto resized =
                            components::resize_rgba_triangle(
                                frame,
                                dimensions->first,
                                dimensions->second);
                        if (!resized.has_value()) {
                            producer_error = BackendError{
                                "CAPTURE_READBACK_FAILED",
                                "A streamed frame could not be resized."};
                            return false;
                        }
                        frame = std::move(*resized);
                    }
                    if (!analysis.accept(frame)) {
                        producer_error = BackendError{
                            analysis.error_code(),
                            analysis.error_message()};
                        return false;
                    }
                    if (!sink(frame)) {
                        producer_error = BackendError{
                            "VIDEO_ENCODER_WRITE_FAILED",
                            "The fixed encoder rejected a streamed frame."};
                        return false;
                    }
                    return true;
                });
            if (producer_error.has_value()) {
                return producer_error;
            }
            if (captured.error.has_value()) {
                return captured.error;
            }
            capture_metadata = std::move(captured.metadata);
            return std::nullopt;
        });
    if (encoded.error.has_value()) {
        return failure(*encoded.error);
    }
    if (!capture_metadata.has_value() ||
        capture_metadata->frames_captured == 0U ||
        capture_metadata->frames_captured >
            config.frame_count ||
        !capture_metadata->single_capture_session ||
        !capture_metadata->foreground_unchanged) {
        std::error_code error;
        std::filesystem::remove(
            encoded.evidence->output_path, error);
        return failure(BackendError{
            "HOST_INTERFERENCE_DETECTED",
            "The streamed capture violated its session boundary."});
    }
    auto analyzed = analysis.finalize();
    if (!analyzed.analysis.has_value()) {
        std::error_code error;
        std::filesystem::remove(
            encoded.evidence->output_path, error);
        return failure(BackendError{
            analyzed.error_code,
            analyzed.error_message});
    }
    return RecordingStreamPipelineResult{
        RecordingStreamPipelineEvidence{
            std::move(*encoded.evidence),
            std::move(*analyzed.analysis),
            static_cast<std::int32_t>(dimensions->first),
            static_cast<std::int32_t>(dimensions->second),
            capture_metadata->frames_captured,
            capture_metadata->device_driver,
            true,
            true,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
