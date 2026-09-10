#include "components/recording_config.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <filesystem>
#include <limits>
#include <string_view>

namespace act::components {
namespace {

constexpr std::uint32_t default_duration_ms = 30000U;
constexpr std::uint32_t default_fps = 2U;
constexpr std::uint32_t default_maximum_width = 960U;
constexpr std::uint32_t default_crf = 32U;
constexpr std::uint32_t default_maximum_keyframes = 8U;
constexpr double default_change_threshold = 0.035;
constexpr std::uint32_t default_frame_timeout_ms = 5000U;

RecordingConfigResult failure(
    const char* code,
    const std::string& message) {
    return RecordingConfigResult{
        std::nullopt, code, message};
}

bool known_field(const std::string_view name) {
    return name == "path" ||
           name == "durationMs" ||
           name == "fps" ||
           name == "maxWidth" ||
           name == "crf" ||
           name == "maxKeyframes" ||
           name == "changeThreshold" ||
           name == "analysisDir" ||
           name == "timeoutMs" ||
           name == "overwrite";
}

std::optional<std::uint32_t> bounded_integer(
    const Json& input,
    const char* name,
    const std::uint32_t fallback,
    const std::uint32_t minimum,
    const std::uint32_t maximum) {
    const auto* value = input.find(name);
    if (value == nullptr) {
        return fallback;
    }
    const auto* integer = value->integer_value();
    if (integer == nullptr ||
        *integer < static_cast<std::int64_t>(minimum) ||
        *integer > static_cast<std::int64_t>(maximum)) {
        return std::nullopt;
    }
    return static_cast<std::uint32_t>(*integer);
}

std::optional<double> bounded_number(
    const Json& input,
    const char* name,
    const double fallback,
    const double minimum,
    const double maximum) {
    const auto* value = input.find(name);
    if (value == nullptr) {
        return fallback;
    }
    double number = 0.0;
    if (const auto* floating = value->double_value();
        floating != nullptr) {
        number = *floating;
    } else if (
        const auto* integer = value->integer_value();
        integer != nullptr) {
        number = static_cast<double>(*integer);
    } else {
        return std::nullopt;
    }
    if (!std::isfinite(number) ||
        number < minimum ||
        number > maximum) {
        return std::nullopt;
    }
    return number;
}

std::optional<std::filesystem::path> normalized_path(
    const std::string& value) {
    if (value.empty()) {
        return std::nullopt;
    }
    std::error_code error;
    const std::u8string utf8_value(
        reinterpret_cast<const char8_t*>(value.data()),
        value.size());
    auto result =
        std::filesystem::absolute(
            std::filesystem::path{utf8_value}, error);
    if (error) {
        return std::nullopt;
    }
    result = result.lexically_normal();
    if (!result.has_filename() ||
        result.native().size() > 32767U) {
        return std::nullopt;
    }
    return result;
}

bool extension_is(
    const std::filesystem::path& path,
    const std::string_view expected) {
    std::string extension = path.extension().string();
    std::transform(
        extension.begin(),
        extension.end(),
        extension.begin(),
        [](const unsigned char value) {
            return static_cast<char>(std::tolower(value));
        });
    return extension == expected;
}

bool directory_exists(
    const std::filesystem::path& path) {
    std::error_code error;
    return std::filesystem::is_directory(path, error) &&
           !error;
}

bool path_exists(
    const std::filesystem::path& path) {
    std::error_code error;
    return std::filesystem::exists(path, error) &&
           !error;
}

bool path_is_symlink(
    const std::filesystem::path& path) {
    std::error_code error;
    const auto status =
        std::filesystem::symlink_status(path, error);
    return !error && std::filesystem::is_symlink(status);
}

std::optional<bool> directory_has_entries(
    const std::filesystem::path& path) {
    std::error_code error;
    const auto begin =
        std::filesystem::directory_iterator(path, error);
    if (error) {
        return std::nullopt;
    }
    return begin != std::filesystem::directory_iterator{};
}

std::string utf8_path(
    const std::filesystem::path& path) {
    const auto value = path.u8string();
    return std::string(
        reinterpret_cast<const char*>(value.data()),
        value.size());
}

}  // namespace

RecordingConfigResult parse_recording_config(
    const Json& input) {
    const auto* fields = input.object_items();
    if (fields == nullptr) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording input must be an object.");
    }
    for (const auto& [name, value] : *fields) {
        static_cast<void>(value);
        if (!known_field(name)) {
            return failure(
                "INVALID_ARGUMENT",
                "Recording input contains an unknown field.");
        }
    }
    const auto* path_value = input.find("path");
    if (path_value == nullptr ||
        path_value->string_value() == nullptr ||
        path_value->string_value()->empty()) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording path is required.");
    }
    const auto output =
        normalized_path(*path_value->string_value());
    if (!output.has_value() ||
        !extension_is(*output, ".mp4")) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording output requires a valid .mp4 path.");
    }
    if (!directory_exists(output->parent_path())) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording output parent directory does not exist.");
    }
    const auto* overwrite_value = input.find("overwrite");
    if (overwrite_value != nullptr &&
        overwrite_value->bool_value() == nullptr) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording overwrite must be boolean.");
    }
    const bool overwrite =
        overwrite_value != nullptr &&
        *overwrite_value->bool_value();
    if (path_exists(*output) &&
        (path_is_symlink(*output) || !overwrite)) {
        return failure(
            path_is_symlink(*output)
                ? "INVALID_ARGUMENT"
                : "OVERWRITE_CONFIRMATION_REQUIRED",
            path_is_symlink(*output)
                ? "Recording output cannot replace a symbolic link."
                : "Recording output exists and overwrite is absent.");
    }

    std::optional<std::filesystem::path> analysis;
    if (const auto* value = input.find("analysisDir");
        value != nullptr) {
        if (value->string_value() == nullptr ||
            value->string_value()->empty()) {
            return failure(
                "INVALID_ARGUMENT",
                "Recording analysisDir must be a non-empty string.");
        }
        analysis = normalized_path(*value->string_value());
    } else {
        analysis = output->parent_path() /
            (output->stem().wstring() + L".analysis");
    }
    if (!analysis.has_value() ||
        *analysis == *output) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording analysis directory is invalid.");
    }
    if (path_exists(*analysis)) {
        if (!directory_exists(*analysis) ||
            path_is_symlink(*analysis)) {
            return failure(
                "INVALID_ARGUMENT",
                "Recording analysisDir must be a real directory.");
        }
        const auto nonempty =
            directory_has_entries(*analysis);
        if (!nonempty.has_value()) {
            return failure(
                "OUTPUT_INSPECTION_FAILED",
                "Recording analysisDir could not be inspected.");
        }
        if (*nonempty && !overwrite) {
            return failure(
                "OVERWRITE_CONFIRMATION_REQUIRED",
                "Recording analysisDir is non-empty and overwrite is "
                "absent.");
        }
    } else if (!directory_exists(analysis->parent_path())) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording analysisDir parent does not exist.");
    }

    const auto duration = bounded_integer(
        input, "durationMs", default_duration_ms,
        1000U, 300000U);
    const auto fps = bounded_integer(
        input, "fps", default_fps, 1U, 10U);
    const auto maximum_width = bounded_integer(
        input, "maxWidth", default_maximum_width,
        320U, 1920U);
    const auto crf = bounded_integer(
        input, "crf", default_crf, 18U, 40U);
    const auto maximum_keyframes = bounded_integer(
        input, "maxKeyframes", default_maximum_keyframes,
        2U, 20U);
    const auto threshold = bounded_number(
        input, "changeThreshold",
        default_change_threshold, 0.005, 0.5);
    const auto frame_timeout = bounded_integer(
        input, "timeoutMs", default_frame_timeout_ms,
        250U, 30000U);
    if (!duration.has_value() ||
        !fps.has_value() ||
        !maximum_width.has_value() ||
        !crf.has_value() ||
        !maximum_keyframes.has_value() ||
        !threshold.has_value() ||
        !frame_timeout.has_value()) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording numeric input violates the bounded contract.");
    }
    const std::uint64_t frame_budget_wide =
        (static_cast<std::uint64_t>(*duration) *
             static_cast<std::uint64_t>(*fps) +
         999U) /
        1000U;
    if (frame_budget_wide == 0U ||
        frame_budget_wide > 3000U) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "Recording frame budget exceeds the certified limit.");
    }
    return RecordingConfigResult{
        RecordingConfig{
            utf8_path(*output),
            utf8_path(*analysis),
            *duration,
            *fps,
            *maximum_width,
            *crf,
            *maximum_keyframes,
            *threshold,
            *frame_timeout,
            static_cast<std::uint32_t>(
                frame_budget_wide),
            overwrite,
        },
        {},
        {},
    };
}

std::optional<std::pair<std::uint32_t, std::uint32_t>>
recording_output_dimensions(
    const std::uint32_t source_width,
    const std::uint32_t source_height,
    const std::uint32_t maximum_width) {
    if (source_width < 2U || source_height < 2U ||
        maximum_width < 2U) {
        return std::nullopt;
    }
    const std::uint32_t width =
        (std::min(source_width, maximum_width) & ~1U);
    const std::uint64_t scaled =
        (static_cast<std::uint64_t>(source_height) *
             width +
         source_width - 1U) /
        source_width;
    if (scaled >
        std::numeric_limits<std::uint32_t>::max()) {
        return std::nullopt;
    }
    const std::uint32_t height =
        (std::max<std::uint32_t>(
             static_cast<std::uint32_t>(scaled), 2U) &
         ~1U);
    return std::pair{
        std::max(width, 2U),
        std::max(height, 2U),
    };
}

}  // namespace act::components
