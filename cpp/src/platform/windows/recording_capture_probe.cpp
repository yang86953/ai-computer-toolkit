#include "platform/windows/wgc_capture.hpp"

#include <set>
#include <string>

namespace act::platform::windows {

RecordingProbeResult capture_fixture_recording_probe(
    const std::uint32_t frame_count,
    const std::uint32_t interval_ms) {
    auto captured = capture_fixture_recording_frames(
        frame_count, interval_ms);
    if (captured.error.has_value()) {
        return RecordingProbeResult{
            std::nullopt, std::move(captured.error)};
    }
    auto& sequence = *captured.sequence;
    std::vector<std::string> digests;
    digests.reserve(sequence.frames.size());
    for (const auto& frame : sequence.frames) {
        digests.push_back(components::byte_digest(
            frame.pixels.data(), frame.pixels.size()));
    }
    const std::set<std::string> distinct(
        digests.begin(), digests.end());
    return RecordingProbeResult{
        RecordingProbeMetadata{
            sequence.width,
            sequence.height,
            frame_count,
            static_cast<std::uint32_t>(digests.size()),
            static_cast<std::uint32_t>(distinct.size()),
            std::move(sequence.device_driver),
            std::move(digests),
            sequence.fixture_owned_by_toolkit,
            sequence.single_capture_session,
            true,
            false,
            false,
            sequence.foreground_unchanged,
            sequence.privacy_indicator_may_have_appeared,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
