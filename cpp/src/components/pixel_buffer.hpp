#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::components {

struct RgbaImage {
    std::uint32_t width;
    std::uint32_t height;
    std::vector<std::uint8_t> pixels;
};

struct PixelConversionResult {
    std::optional<RgbaImage> image;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] PixelConversionResult convert_bgra_to_rgba(
    const std::uint8_t* source,
    std::size_t row_pitch,
    std::uint32_t width,
    std::uint32_t height);

[[nodiscard]] std::string byte_digest(
    const std::uint8_t* bytes,
    std::size_t size);

}  // namespace act::components
