#include "platform/windows/text_codec.hpp"

#include <windows.h>

namespace act::platform::windows {

std::string utf8(const std::wstring_view value) {
    if (value.empty()) {
        return {};
    }
    const int required = WideCharToMultiByte(
        CP_UTF8,
        WC_ERR_INVALID_CHARS,
        value.data(),
        static_cast<int>(value.size()),
        nullptr,
        0,
        nullptr,
        nullptr);
    if (required <= 0) {
        return "<unavailable>";
    }
    std::string output(static_cast<std::size_t>(required), '\0');
    const int copied = WideCharToMultiByte(
        CP_UTF8,
        WC_ERR_INVALID_CHARS,
        value.data(),
        static_cast<int>(value.size()),
        output.data(),
        required,
        nullptr,
        nullptr);
    if (copied != required) {
        return "<unavailable>";
    }
    return output;
}

std::wstring wide(const std::string_view value) {
    if (value.empty()) {
        return {};
    }
    const int required = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        value.data(),
        static_cast<int>(value.size()),
        nullptr,
        0);
    if (required <= 0) {
        return {};
    }
    std::wstring output(
        static_cast<std::size_t>(required), L'\0');
    const int copied = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        value.data(),
        static_cast<int>(value.size()),
        output.data(),
        required);
    if (copied != required) {
        return {};
    }
    return output;
}

}  // namespace act::platform::windows
