#include "components/recording_analysis.hpp"

#include <iostream>
#include <vector>

namespace {

act::components::RgbaImage solid(
    const std::uint8_t value) {
    constexpr std::uint32_t width = 128U;
    constexpr std::uint32_t height = 96U;
    act::components::RgbaImage image{
        width,
        height,
        std::vector<std::uint8_t>(
            static_cast<std::size_t>(width) * height * 4U),
    };
    for (std::size_t offset = 0U;
         offset < image.pixels.size();
         offset += 4U) {
        image.pixels[offset] = value;
        image.pixels[offset + 1U] = value;
        image.pixels[offset + 2U] = value;
        image.pixels[offset + 3U] = 255U;
    }
    return image;
}

}  // namespace

int main() {
    const std::vector<act::components::RgbaImage> dynamic{
        solid(16U),
        solid(64U),
        solid(128U),
        solid(224U),
    };
    const auto all =
        act::components::analyze_recording_frames(
            dynamic, 2U, 4U, 0.035);
    const auto bounded =
        act::components::analyze_recording_frames(
            dynamic, 2U, 2U, 0.035);
    const std::vector<act::components::RgbaImage> static_frames{
        solid(32U),
        solid(32U),
        solid(32U),
    };
    const auto unchanged =
        act::components::analyze_recording_frames(
            static_frames, 2U, 8U, 0.035);
    auto mixed = dynamic;
    mixed.back().width = 126U;
    const auto refused =
        act::components::analyze_recording_frames(
            mixed, 2U, 4U, 0.035);
    act::components::RecordingAnalysisAccumulator maximum(
        10U, 2U, 0.035);
    const auto small = solid(48U);
    bool maximum_accepted = true;
    for (std::uint32_t index = 0U;
         index < 3000U;
         ++index) {
        maximum_accepted =
            maximum_accepted && maximum.accept(small);
    }
    const auto maximum_result = maximum.finalize();
    act::components::RecordingAnalysisAccumulator overflow(
        10U, 2U, 0.035);
    bool overflow_refused = false;
    for (std::uint32_t index = 0U;
         index <= 3000U;
         ++index) {
        if (!overflow.accept(small)) {
            overflow_refused =
                index == 3000U &&
                overflow.error_code() ==
                    "RESOURCE_LIMIT_EXCEEDED";
            break;
        }
    }
    if (!all.analysis.has_value() ||
        all.analysis->keyframes.size() != 3U ||
        all.analysis->storyboard.width != 384U ||
        all.analysis->storyboard.height != 96U ||
        !bounded.analysis.has_value() ||
        bounded.analysis->keyframes.size() != 2U ||
        bounded.analysis->keyframes.front().frame_index != 0U ||
        bounded.analysis->keyframes.back().frame_index != 3U ||
        !unchanged.analysis.has_value() ||
        unchanged.analysis->keyframes.size() != 1U ||
        refused.analysis.has_value() ||
        refused.error_code != "CAPTURE_SIZE_CHANGED" ||
        !maximum_accepted ||
        maximum.accepted_frames() != 3000U ||
        !maximum_result.analysis.has_value() ||
        maximum_result.analysis->keyframes.size() != 1U ||
        !overflow_refused) {
        std::cerr
            << "Recording analysis differs from its bounded policy.\n";
        return 1;
    }
    return 0;
}
