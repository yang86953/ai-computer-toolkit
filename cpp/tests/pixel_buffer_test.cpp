#include "components/pixel_buffer.hpp"

#include <algorithm>
#include <array>
#include <iostream>

int main() {
    constexpr std::array<std::uint8_t, 16> padded_bgra{
        1U, 2U, 3U, 4U,
        5U, 6U, 7U, 8U,
        91U, 92U, 93U, 94U,
        95U, 96U, 97U, 98U,
    };
    const auto converted = act::components::convert_bgra_to_rgba(
        padded_bgra.data(), 16U, 2U, 1U);
    constexpr std::array<std::uint8_t, 8> expected_rgba{
        3U, 2U, 1U, 4U,
        7U, 6U, 5U, 8U,
    };
    if (!converted.image.has_value() ||
        converted.image->width != 2U ||
        converted.image->height != 1U ||
        !std::equal(
            converted.image->pixels.begin(),
            converted.image->pixels.end(),
            expected_rgba.begin(),
            expected_rgba.end())) {
        std::cerr << "BGRA to RGBA conversion failed\n";
        return 1;
    }

    const auto short_pitch =
        act::components::convert_bgra_to_rgba(
            padded_bgra.data(), 7U, 2U, 1U);
    const auto oversized =
        act::components::convert_bgra_to_rgba(
            padded_bgra.data(), 16U, 4097U, 1U);
    const auto empty =
        act::components::convert_bgra_to_rgba(
            nullptr, 0U, 0U, 0U);
    if (short_pitch.image.has_value() ||
        short_pitch.error_code != "OPERATION_FAILED" ||
        oversized.image.has_value() ||
        oversized.error_code != "RESOURCE_LIMIT_EXCEEDED" ||
        empty.image.has_value() ||
        empty.error_code != "INVALID_ARGUMENT") {
        std::cerr << "pixel conversion bounds failed closed incorrectly\n";
        return 1;
    }

    const std::string digest = act::components::byte_digest(
        expected_rgba.data(), expected_rgba.size());
    if (digest.size() != 16U) {
        std::cerr << "pixel digest is not fixed width\n";
        return 1;
    }
    std::cout << "{\"ok\":true,\"pixelFormat\":\"rgba8\","
                 "\"paddingIgnored\":true,\"boundsFailClosed\":true,"
                 "\"digest\":\""
              << digest << "\"}\n";
    return 0;
}
