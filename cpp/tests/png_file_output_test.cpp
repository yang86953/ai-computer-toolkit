#include "platform/windows/png_encoder.hpp"
#include "platform/windows/png_file_output.hpp"

#include <windows.h>
#include <objbase.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

namespace {

std::vector<std::uint8_t> read_all(
    const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary);
    return std::vector<std::uint8_t>(
        std::istreambuf_iterator<char>(input),
        std::istreambuf_iterator<char>());
}

}  // namespace

int main(const int argc, char** argv) {
    if (argc != 2) {
        std::cerr << "test requires a dedicated output directory\n";
        return 1;
    }
    const std::filesystem::path directory =
        std::filesystem::path(argv[1]);
    std::error_code filesystem_error;
    if (!std::filesystem::create_directory(
            directory, filesystem_error) ||
        filesystem_error) {
        std::cerr << "dedicated output directory could not be created\n";
        return 1;
    }
    const std::filesystem::path output =
        directory / "result.png";

    const HRESULT initialized =
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const bool should_uninitialize = SUCCEEDED(initialized);
    const auto finish = [&]() {
        if (should_uninitialize) {
            CoUninitialize();
        }
        std::filesystem::remove(output, filesystem_error);
        std::filesystem::remove(directory, filesystem_error);
    };
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        finish();
        return 1;
    }

    const std::vector<std::uint8_t> red{
        255U, 0U, 0U, 255U,
    };
    const std::vector<std::uint8_t> blue{
        0U, 0U, 255U, 255U,
    };
    const auto first_png =
        act::platform::windows::encode_rgba_png(1U, 1U, red);
    const auto second_png =
        act::platform::windows::encode_rgba_png(1U, 1U, blue);
    if (!first_png.image.has_value() ||
        !second_png.image.has_value()) {
        finish();
        return 1;
    }

    const std::string output_utf8 = output.string();
    const auto first =
        act::platform::windows::write_png_atomically(
            output_utf8, first_png.image->bytes, false);
    if (!first.output.has_value() ||
        first.output->replaced_existing ||
        read_all(output) != first_png.image->bytes) {
        std::cerr << "initial atomic PNG write failed\n";
        finish();
        return 1;
    }

    const auto refused =
        act::platform::windows::write_png_atomically(
            output_utf8, second_png.image->bytes, false);
    if (refused.output.has_value() ||
        refused.error_code !=
            "OVERWRITE_CONFIRMATION_REQUIRED" ||
        read_all(output) != first_png.image->bytes) {
        std::cerr << "unconfirmed overwrite was not refused\n";
        finish();
        return 1;
    }

    const auto replaced =
        act::platform::windows::write_png_atomically(
            output_utf8, second_png.image->bytes, true);
    if (!replaced.output.has_value() ||
        !replaced.output->replaced_existing ||
        read_all(output) != second_png.image->bytes) {
        std::cerr << "confirmed atomic PNG replacement failed\n";
        finish();
        return 1;
    }

    std::size_t temporary_files = 0U;
    for (const auto& entry :
         std::filesystem::directory_iterator(directory)) {
        const std::string name =
            entry.path().filename().string();
        if (name.find(".part.png") != std::string::npos) {
            ++temporary_files;
        }
    }
    const auto invalid_extension =
        act::platform::windows::write_png_atomically(
            (directory / "invalid.jpg").string(),
            first_png.image->bytes,
            false);
    const auto invalid_bytes =
        act::platform::windows::write_png_atomically(
            (directory / "invalid.png").string(),
            std::vector<std::uint8_t>{1U, 2U, 3U},
            false);
    if (temporary_files != 0U ||
        invalid_extension.error_code != "INVALID_ARGUMENT" ||
        invalid_bytes.error_code != "INVALID_ARGUMENT") {
        std::cerr << "atomic PNG validation or cleanup failed\n";
        finish();
        return 1;
    }

    std::cout << "{\"ok\":true,\"initialWrite\":true,"
                 "\"unconfirmedOverwriteRefused\":true,"
                 "\"confirmedReplace\":true,"
                 "\"temporaryFilesRemaining\":0,"
                 "\"normalizedPath\":true}\n";
    finish();
    return 0;
}
