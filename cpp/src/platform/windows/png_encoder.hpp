#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct PngMemoryImage {
    std::vector<std::uint8_t> bytes;
};

struct PngEncodeResult {
    std::optional<PngMemoryImage> image;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] PngEncodeResult encode_rgba_png(
    std::uint32_t width,
    std::uint32_t height,
    const std::vector<std::uint8_t>& rgba);

}  // namespace act::platform::windows
