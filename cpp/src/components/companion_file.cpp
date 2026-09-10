#include "components/companion_file.hpp"

#include <windows.h>

#include <array>
#include <filesystem>
#include <fstream>
#include <iterator>

namespace act::components {
namespace {

std::optional<std::filesystem::path> companion_path(
    const std::string& filename) {
    if (filename.empty() ||
        filename.find_first_of("\\/") != std::string::npos) {
        return std::nullopt;
    }
    std::array<wchar_t, 32768> module{};
    const DWORD length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    if (length == 0U || length >= module.size()) {
        return std::nullopt;
    }
    return std::filesystem::path(
            std::wstring(module.data(), length))
            .parent_path() /
        filename;
}

}  // namespace

std::optional<std::string> read_companion_text(
    const std::string& filename) {
    const auto path = companion_path(filename);
    if (!path.has_value()) {
        return std::nullopt;
    }
    std::ifstream stream(*path, std::ios::binary);
    if (!stream) {
        return std::nullopt;
    }
    return std::string(
        std::istreambuf_iterator<char>(stream),
        std::istreambuf_iterator<char>());
}

bool companion_file_exists(const std::string& filename) {
    const auto path = companion_path(filename);
    if (!path.has_value()) {
        return false;
    }
    std::error_code error;
    return std::filesystem::is_regular_file(*path, error) &&
           !error;
}

}  // namespace act::components
