#include "components/pixel_buffer.hpp"

#include <iomanip>
#include <sstream>

namespace act::components {
namespace {

constexpr std::uint32_t maximum_dimension = 4096U;
constexpr std::size_t maximum_pixel_bytes =
    64U * 1024U * 1024U;
constexpr std::size_t channels = 4U;

PixelConversionResult failure(
    const char* code,
    const char* message) {
    return PixelConversionResult{
        std::nullopt,
        code,
        message,
    };
}

}  // namespace

PixelConversionResult convert_bgra_to_rgba(
    const std::uint8_t* source,
    const std::size_t row_pitch,
    const std::uint32_t width,
    const std::uint32_t height) {
    if (source == nullptr || width == 0U || height == 0U) {
        return failure(
            "INVALID_ARGUMENT",
            "Pixel conversion requires non-empty source dimensions.");
    }
    if (width > maximum_dimension || height > maximum_dimension) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "Pixel dimensions exceed the certified 4096 limit.");
    }
    const std::size_t row_bytes =
        static_cast<std::size_t>(width) * channels;
    if (row_pitch < row_bytes) {
        return failure(
            "OPERATION_FAILED",
            "Pixel row pitch is smaller than the active BGRA row.");
    }
    const std::size_t total_bytes =
        row_bytes * static_cast<std::size_t>(height);
    if (total_bytes > maximum_pixel_bytes) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "Pixel image exceeds the certified 64 MiB limit.");
    }

    RgbaImage image{
        width,
        height,
        std::vector<std::uint8_t>(total_bytes),
    };
    for (std::uint32_t row = 0U; row < height; ++row) {
        const auto* input =
            source + static_cast<std::size_t>(row) * row_pitch;
        auto* output =
            image.pixels.data() +
            static_cast<std::size_t>(row) * row_bytes;
        for (std::uint32_t column = 0U;
             column < width;
             ++column) {
            const std::size_t offset =
                static_cast<std::size_t>(column) * channels;
            output[offset] = input[offset + 2U];
            output[offset + 1U] = input[offset + 1U];
            output[offset + 2U] = input[offset];
            output[offset + 3U] = input[offset + 3U];
        }
    }
    return PixelConversionResult{
        std::move(image),
        {},
        {},
    };
}

std::string byte_digest(
    const std::uint8_t* bytes,
    const std::size_t size) {
    std::uint64_t digest = 14695981039346656037ULL;
    for (std::size_t index = 0U; index < size; ++index) {
        digest ^= bytes[index];
        digest *= 1099511628211ULL;
    }
    std::ostringstream output;
    output << std::hex << std::setw(16)
           << std::setfill('0') << digest;
    return output.str();
}

}  // namespace act::components
