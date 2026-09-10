#pragma once

#include "components/pixel_buffer.hpp"

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::components {

struct RecordingKeyframe {
    std::uint64_t frame_index;
    std::uint64_t timestamp_ms;
    double change_score;
    RgbaImage image;
};

struct RecordingAnalysis {
    std::vector<RecordingKeyframe> keyframes;
    RgbaImage storyboard;
};

struct RecordingAnalysisResult {
    std::optional<RecordingAnalysis> analysis;
    std::string error_code;
    std::string error_message;
};

class RecordingAnalysisAccumulator final {
public:
    RecordingAnalysisAccumulator(
        std::uint32_t fps,
        std::uint32_t maximum_keyframes,
        double change_threshold);

    [[nodiscard]] bool accept(const RgbaImage& frame);
    [[nodiscard]] RecordingAnalysisResult finalize();
    [[nodiscard]] const std::string& error_code() const;
    [[nodiscard]] const std::string& error_message() const;
    [[nodiscard]] std::uint32_t accepted_frames() const;

private:
    std::uint32_t fps_;
    std::uint32_t maximum_keyframes_;
    double change_threshold_;
    std::uint32_t width_ = 0U;
    std::uint32_t height_ = 0U;
    std::uint32_t accepted_frames_ = 0U;
    double last_score_ = 0.0;
    std::vector<std::uint8_t> previous_;
    std::optional<RecordingKeyframe> first_;
    std::optional<RecordingKeyframe> last_;
    std::vector<RecordingKeyframe> changes_;
    std::string error_code_;
    std::string error_message_;
    bool finalized_ = false;
};

[[nodiscard]] RecordingAnalysisResult analyze_recording_frames(
    const std::vector<RgbaImage>& frames,
    std::uint32_t fps,
    std::uint32_t maximum_keyframes,
    double change_threshold);
[[nodiscard]] std::optional<RgbaImage> resize_rgba_triangle(
    const RgbaImage& source,
    std::uint32_t width,
    std::uint32_t height);

}  // namespace act::components
