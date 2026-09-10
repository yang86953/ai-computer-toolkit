#include "platform/windows/wgc_capture.hpp"

#include "platform/windows/wgc_recording_detail.hpp"

#include <windows.h>
#include <winrt/base.h>

namespace act::platform::windows {

RecordingFrameSequenceResult capture_fixture_recording_frames(
    const std::uint32_t frame_count,
    const std::uint32_t interval_ms) {
    try {
        std::vector<components::RgbaImage> frames;
        frames.reserve(frame_count);
        auto result = detail::capture_recording_frames_impl(
            frame_count,
            interval_ms,
            0U,
            [&frames](components::RgbaImage&& frame) {
                frames.push_back(std::move(frame));
                return true;
            });
        if (result.error.has_value()) {
            return RecordingFrameSequenceResult{
                std::nullopt, std::move(result.error)};
        }
        const auto& value = *result.metadata;
        return RecordingFrameSequenceResult{
            RecordingFrameSequence{
                value.width,
                value.height,
                value.device_driver,
                std::move(frames),
                value.fixture_owned_by_toolkit,
                value.single_capture_session,
                value.foreground_unchanged,
                value.privacy_indicator_may_have_appeared,
            },
            std::nullopt,
        };
    } catch (const winrt::hresult_error&) {
        return RecordingFrameSequenceResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The recording probe received a WGC or D3D failure.",
            },
        };
    }
}

RecordingFrameSequenceResult capture_window_recording_frames(
    const std::uintptr_t native_window,
    const std::uint32_t frame_count,
    const std::uint32_t interval_ms) {
    try {
        std::vector<components::RgbaImage> frames;
        frames.reserve(frame_count);
        auto result = detail::capture_recording_frames_impl(
            frame_count,
            interval_ms,
            native_window,
            [&frames](components::RgbaImage&& frame) {
                frames.push_back(std::move(frame));
                return true;
            });
        if (result.error.has_value()) {
            return RecordingFrameSequenceResult{
                std::nullopt, std::move(result.error)};
        }
        const auto& value = *result.metadata;
        return RecordingFrameSequenceResult{
            RecordingFrameSequence{
                value.width,
                value.height,
                value.device_driver,
                std::move(frames),
                value.fixture_owned_by_toolkit,
                value.single_capture_session,
                value.foreground_unchanged,
                value.privacy_indicator_may_have_appeared,
            },
            std::nullopt,
        };
    } catch (const winrt::hresult_error& error) {
        return RecordingFrameSequenceResult{
            std::nullopt,
            BackendError{
                error.code() == E_ACCESSDENIED
                    ? "PERMISSION_DENIED"
                    : "OPERATION_FAILED",
                "The exact-window recording capture failed.",
            },
        };
    }
}

RecordingStreamResult capture_window_recording_stream(
    const std::uintptr_t native_window,
    const std::uint32_t frame_count,
    const std::uint32_t interval_ms,
    const RecordingFrameConsumer& consumer) {
    try {
        return detail::capture_recording_frames_impl(
            frame_count,
            interval_ms,
            native_window,
            consumer);
    } catch (const winrt::hresult_error& error) {
        return RecordingStreamResult{
            std::nullopt,
            BackendError{
                error.code() == E_ACCESSDENIED
                    ? "PERMISSION_DENIED"
                    : "OPERATION_FAILED",
                "The exact-window recording stream failed.",
            },
        };
    }
}

}  // namespace act::platform::windows
