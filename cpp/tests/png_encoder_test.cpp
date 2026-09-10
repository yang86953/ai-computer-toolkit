#include "platform/windows/png_encoder.hpp"

#include <windows.h>
#include <objbase.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <iostream>
#include <vector>

namespace {

std::uint32_t big_endian_u32(
    const std::vector<std::uint8_t>& bytes,
    const std::size_t offset) {
    return
        (static_cast<std::uint32_t>(bytes[offset]) << 24U) |
        (static_cast<std::uint32_t>(bytes[offset + 1U]) << 16U) |
        (static_cast<std::uint32_t>(bytes[offset + 2U]) << 8U) |
        static_cast<std::uint32_t>(bytes[offset + 3U]);
}

}  // namespace

int main() {
    const HRESULT initialized =
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const bool should_uninitialize = SUCCEEDED(initialized);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        std::cerr << "COM initialization failed\n";
        return 1;
    }

    constexpr std::array<std::uint8_t, 16> rgba{
        255U, 0U, 0U, 255U,
        0U, 255U, 0U, 255U,
        0U, 0U, 255U, 255U,
        255U, 255U, 255U, 255U,
    };
    const auto encoded = act::platform::windows::encode_rgba_png(
        2U,
        2U,
        std::vector<std::uint8_t>(rgba.begin(), rgba.end()));
    constexpr std::array<std::uint8_t, 8> signature{
        0x89U, 0x50U, 0x4eU, 0x47U,
        0x0dU, 0x0aU, 0x1aU, 0x0aU,
    };
    if (!encoded.image.has_value() ||
        encoded.image->bytes.size() < 24U ||
        !std::equal(
            signature.begin(),
            signature.end(),
            encoded.image->bytes.begin(),
            encoded.image->bytes.begin() + 8) ||
        big_endian_u32(encoded.image->bytes, 16U) != 2U ||
        big_endian_u32(encoded.image->bytes, 20U) != 2U) {
        std::cerr << "WIC PNG structure is invalid\n";
        if (should_uninitialize) {
            CoUninitialize();
        }
        return 1;
    }

    const auto mismatch =
        act::platform::windows::encode_rgba_png(
            2U, 2U, std::vector<std::uint8_t>(3U));
    const auto oversized =
        act::platform::windows::encode_rgba_png(
            4097U, 1U, std::vector<std::uint8_t>());
    if (mismatch.image.has_value() ||
        mismatch.error_code != "RESOURCE_LIMIT_EXCEEDED" ||
        oversized.image.has_value() ||
        oversized.error_code != "INVALID_ARGUMENT") {
        std::cerr << "WIC PNG bounds did not fail closed\n";
        if (should_uninitialize) {
            CoUninitialize();
        }
        return 1;
    }

    if (should_uninitialize) {
        CoUninitialize();
    }
    std::cout << "{\"ok\":true,\"pngSignature\":true,"
                 "\"ihdrWidth\":2,\"ihdrHeight\":2,"
                 "\"memoryOnly\":true,\"boundsFailClosed\":true}\n";
    return 0;
}
