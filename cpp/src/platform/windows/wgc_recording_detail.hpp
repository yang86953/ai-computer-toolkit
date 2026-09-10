#pragma once

#include "platform/windows/wgc_capture.hpp"

#include <cstdint>

namespace act::platform::windows::detail {

[[nodiscard]] RecordingStreamResult
capture_recording_frames_impl(
    std::uint32_t frame_count,
    std::uint32_t interval_ms,
    std::uintptr_t native_window,
    const RecordingFrameConsumer& consumer);

}  // namespace act::platform::windows::detail
