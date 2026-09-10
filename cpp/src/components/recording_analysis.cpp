#include "components/recording_analysis.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace act::components {
namespace {

constexpr std::uint32_t thumbnail_width = 64U;
constexpr std::uint32_t thumbnail_height = 36U;
constexpr std::uint32_t storyboard_tile_width = 320U;
constexpr std::uint32_t storyboard_columns = 3U;

RecordingAnalysisResult failure(
    const char* code,
    const char* message) {
    return RecordingAnalysisResult{
        std::nullopt, code, message};
}

std::optional<std::vector<std::uint8_t>> luma_thumbnail(
    const RgbaImage& image) {
    const std::uint64_t expected =
        static_cast<std::uint64_t>(image.width) *
        static_cast<std::uint64_t>(image.height) * 4ULL;
    if (image.width == 0U ||
        image.height == 0U ||
        expected != image.pixels.size()) {
        return std::nullopt;
    }
    std::vector<std::uint8_t> thumbnail;
    thumbnail.reserve(
        thumbnail_width * thumbnail_height);
    for (std::uint32_t y = 0U;
         y < thumbnail_height;
         ++y) {
        const std::uint32_t source_y =
            y * image.height / thumbnail_height;
        for (std::uint32_t x = 0U;
             x < thumbnail_width;
             ++x) {
            const std::uint32_t source_x =
                x * image.width / thumbnail_width;
            const std::size_t index =
                (static_cast<std::size_t>(source_y) *
                     image.width +
                 source_x) *
                4U;
            const std::uint16_t luma =
                (77U * image.pixels[index] +
                 150U * image.pixels[index + 1U] +
                 29U * image.pixels[index + 2U]) >>
                8U;
            thumbnail.push_back(
                static_cast<std::uint8_t>(luma));
        }
    }
    return thumbnail;
}

double difference_score(
    const std::vector<std::uint8_t>& left,
    const std::vector<std::uint8_t>& right) {
    if (left.size() != right.size() || left.empty()) {
        return 1.0;
    }
    std::uint64_t total = 0U;
    for (std::size_t index = 0U;
         index < left.size();
         ++index) {
        total += static_cast<std::uint64_t>(
            left[index] > right[index]
                ? left[index] - right[index]
                : right[index] - left[index]);
    }
    return static_cast<double>(total) /
        (static_cast<double>(left.size()) * 255.0);
}

RgbaImage resize_triangle_impl(
    const RgbaImage& source,
    const std::uint32_t width,
    const std::uint32_t height) {
    RgbaImage output{
        width,
        height,
        std::vector<std::uint8_t>(
            static_cast<std::size_t>(width) * height * 4U),
    };
    for (std::uint32_t y = 0U; y < height; ++y) {
        const double source_y =
            (static_cast<double>(y) + 0.5) *
                source.height / height -
            0.5;
        const auto y0 = static_cast<std::uint32_t>(
            std::clamp(
                std::floor(source_y),
                0.0,
                static_cast<double>(source.height - 1U)));
        const auto y1 =
            std::min(y0 + 1U, source.height - 1U);
        const double fy =
            std::clamp(
                source_y - std::floor(source_y),
                0.0,
                1.0);
        for (std::uint32_t x = 0U; x < width; ++x) {
            const double source_x =
                (static_cast<double>(x) + 0.5) *
                    source.width / width -
                0.5;
            const auto x0 = static_cast<std::uint32_t>(
                std::clamp(
                    std::floor(source_x),
                    0.0,
                    static_cast<double>(
                        source.width - 1U)));
            const auto x1 =
                std::min(x0 + 1U, source.width - 1U);
            const double fx =
                std::clamp(
                    source_x - std::floor(source_x),
                    0.0,
                    1.0);
            for (std::size_t channel = 0U;
                 channel < 4U;
                 ++channel) {
                const auto sample =
                    [&source, channel](
                        const std::uint32_t px,
                        const std::uint32_t py) {
                        return static_cast<double>(
                            source.pixels[
                                (static_cast<std::size_t>(py) *
                                     source.width +
                                 px) *
                                    4U +
                                channel]);
                    };
                const double top =
                    sample(x0, y0) * (1.0 - fx) +
                    sample(x1, y0) * fx;
                const double bottom =
                    sample(x0, y1) * (1.0 - fx) +
                    sample(x1, y1) * fx;
                output.pixels[
                    (static_cast<std::size_t>(y) * width + x) *
                        4U +
                    channel] =
                    static_cast<std::uint8_t>(std::clamp(
                        std::lround(
                            top * (1.0 - fy) + bottom * fy),
                        0L,
                        255L));
            }
        }
    }
    return output;
}

std::optional<RgbaImage> make_storyboard(
    const std::vector<RecordingKeyframe>& keyframes,
    const std::uint32_t width,
    const std::uint32_t height) {
    if (keyframes.empty() ||
        width == 0U ||
        height == 0U) {
        return std::nullopt;
    }
    const std::uint32_t tile_width =
        std::min(width, storyboard_tile_width);
    const std::uint64_t scaled_height =
        (static_cast<std::uint64_t>(height) *
             tile_width +
         width - 1U) /
        width;
    if (scaled_height == 0U ||
        scaled_height >
            std::numeric_limits<std::uint32_t>::max()) {
        return std::nullopt;
    }
    const auto tile_height =
        static_cast<std::uint32_t>(scaled_height);
    const auto columns = std::min(
        storyboard_columns,
        static_cast<std::uint32_t>(keyframes.size()));
    const auto rows =
        (static_cast<std::uint32_t>(keyframes.size()) +
         columns - 1U) /
        columns;
    const std::uint64_t output_width =
        static_cast<std::uint64_t>(columns) * tile_width;
    const std::uint64_t output_height =
        static_cast<std::uint64_t>(rows) * tile_height;
    const std::uint64_t output_bytes =
        output_width * output_height * 4ULL;
    if (output_width > 4096U ||
        output_height > 4096U ||
        output_bytes > 64ULL * 1024ULL * 1024ULL) {
        return std::nullopt;
    }
    RgbaImage storyboard{
        static_cast<std::uint32_t>(output_width),
        static_cast<std::uint32_t>(output_height),
        std::vector<std::uint8_t>(
            static_cast<std::size_t>(output_bytes), 24U),
    };
    for (std::size_t index = 0U;
         index < keyframes.size();
         ++index) {
        auto tile = resize_triangle_impl(
            keyframes[index].image,
            tile_width,
            tile_height);
        const std::uint32_t offset_x =
            static_cast<std::uint32_t>(index) %
                columns *
            tile_width;
        const std::uint32_t offset_y =
            static_cast<std::uint32_t>(index) /
                columns *
            tile_height;
        for (std::uint32_t y = 0U;
             y < tile_height;
             ++y) {
            const std::size_t source =
                static_cast<std::size_t>(y) *
                tile_width * 4U;
            const std::size_t destination =
                (static_cast<std::size_t>(offset_y + y) *
                     storyboard.width +
                 offset_x) *
                4U;
            std::copy_n(
                tile.pixels.begin() +
                    static_cast<std::ptrdiff_t>(source),
                static_cast<std::size_t>(tile_width) * 4U,
                storyboard.pixels.begin() +
                    static_cast<std::ptrdiff_t>(destination));
        }
    }
    return storyboard;
}

}  // namespace

std::optional<RgbaImage> resize_rgba_triangle(
    const RgbaImage& source,
    const std::uint32_t width,
    const std::uint32_t height) {
    const std::uint64_t source_bytes =
        static_cast<std::uint64_t>(source.width) *
        source.height * 4ULL;
    const std::uint64_t output_bytes =
        static_cast<std::uint64_t>(width) *
        height * 4ULL;
    if (source.width == 0U ||
        source.height == 0U ||
        source_bytes != source.pixels.size() ||
        width == 0U ||
        height == 0U ||
        width > 4096U ||
        height > 4096U ||
        output_bytes > 64ULL * 1024ULL * 1024ULL) {
        return std::nullopt;
    }
    return resize_triangle_impl(source, width, height);
}

RecordingAnalysisResult analyze_recording_frames(
    const std::vector<RgbaImage>& frames,
    const std::uint32_t fps,
    const std::uint32_t maximum_keyframes,
    const double change_threshold) {
    RecordingAnalysisAccumulator accumulator(
        fps, maximum_keyframes, change_threshold);
    for (const auto& frame : frames) {
        if (!accumulator.accept(frame)) {
            return failure(
                accumulator.error_code().c_str(),
                accumulator.error_message().c_str());
        }
    }
    return accumulator.finalize();
}

RecordingAnalysisAccumulator::RecordingAnalysisAccumulator(
    const std::uint32_t fps,
    const std::uint32_t maximum_keyframes,
    const double change_threshold)
    : fps_(fps),
      maximum_keyframes_(maximum_keyframes),
      change_threshold_(change_threshold) {
    if (fps_ < 1U || fps_ > 10U ||
        maximum_keyframes_ < 2U ||
        maximum_keyframes_ > 20U ||
        !std::isfinite(change_threshold_) ||
        change_threshold_ < 0.005 ||
        change_threshold_ > 0.5) {
        error_code_ = "INVALID_ARGUMENT";
        error_message_ =
            "Recording analysis input violates certified bounds.";
    }
}

bool RecordingAnalysisAccumulator::accept(
    const RgbaImage& frame) {
    if (!error_code_.empty() || finalized_ ||
        accepted_frames_ >= 3000U) {
        if (error_code_.empty()) {
            error_code_ = "RESOURCE_LIMIT_EXCEEDED";
            error_message_ =
                "Recording analysis frame budget was exceeded.";
        }
        return false;
    }
    if (accepted_frames_ == 0U) {
        width_ = frame.width;
        height_ = frame.height;
    } else if (
        frame.width != width_ || frame.height != height_) {
        error_code_ = "CAPTURE_SIZE_CHANGED";
        error_message_ =
            "Recording analysis refuses mixed frame dimensions.";
        return false;
    }
    const auto thumbnail = luma_thumbnail(frame);
    if (!thumbnail.has_value()) {
        error_code_ = "VIDEO_ANALYSIS_FAILED";
        error_message_ =
            "Recording analysis received invalid RGBA pixels.";
        return false;
    }
    const double score = previous_.empty()
        ? 0.0
        : difference_score(previous_, *thumbnail);
    previous_ = *thumbnail;
    RecordingKeyframe candidate{
        accepted_frames_,
        static_cast<std::uint64_t>(accepted_frames_) *
            1000ULL / fps_,
        score,
        frame,
    };
    last_score_ = score;
    last_ = candidate;
    if (!first_.has_value()) {
        first_ = candidate;
    } else if (score >= change_threshold_) {
        const std::size_t capacity =
            maximum_keyframes_ - 2U;
        if (changes_.size() < capacity) {
            changes_.push_back(std::move(candidate));
        } else if (!changes_.empty()) {
            const auto weakest = std::min_element(
                changes_.begin(),
                changes_.end(),
                [](const auto& left, const auto& right) {
                    return left.change_score <
                        right.change_score;
                });
            if (candidate.change_score >
                weakest->change_score) {
                *weakest = std::move(candidate);
            }
        }
    }
    ++accepted_frames_;
    return true;
}

RecordingAnalysisResult
RecordingAnalysisAccumulator::finalize() {
    finalized_ = true;
    if (!error_code_.empty() ||
        !first_.has_value() ||
        !last_.has_value()) {
        return failure(
            error_code_.empty()
                ? "INVALID_ARGUMENT"
                : error_code_.c_str(),
            error_message_.empty()
                ? "Recording analysis requires at least one frame."
                : error_message_.c_str());
    }
    std::vector<RecordingKeyframe> selected;
    selected.push_back(std::move(*first_));
    for (auto& change : changes_) {
        selected.push_back(std::move(change));
    }
    const auto last_index = accepted_frames_ - 1U;
    const bool visually_changed =
        last_score_ >= change_threshold_ ||
        selected.size() > 1U;
    if (selected.front().frame_index != last_index &&
        visually_changed) {
        std::erase_if(
            selected,
            [last_index](const auto& frame) {
                return frame.frame_index == last_index;
            });
        selected.push_back(RecordingKeyframe{
            last_index,
            static_cast<std::uint64_t>(last_index) *
                1000ULL / fps_,
            last_score_,
            std::move(last_->image),
        });
    }
    std::sort(
        selected.begin(),
        selected.end(),
        [](const auto& left, const auto& right) {
            return left.frame_index < right.frame_index;
        });
    if (selected.size() > maximum_keyframes_) {
        selected.resize(maximum_keyframes_);
    }
    auto storyboard =
        make_storyboard(selected, width_, height_);
    if (!storyboard.has_value()) {
        return failure(
            "VIDEO_ANALYSIS_FAILED",
            "Recording storyboard violates certified bounds.");
    }
    return RecordingAnalysisResult{
        RecordingAnalysis{
            std::move(selected),
            std::move(*storyboard),
        },
        {},
        {},
    };
}

const std::string&
RecordingAnalysisAccumulator::error_code() const {
    return error_code_;
}

const std::string&
RecordingAnalysisAccumulator::error_message() const {
    return error_message_;
}

std::uint32_t
RecordingAnalysisAccumulator::accepted_frames() const {
    return accepted_frames_;
}

}  // namespace act::components
