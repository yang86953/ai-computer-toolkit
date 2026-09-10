#include "platform/windows/environment_capability_backend.hpp"

#include <windows.h>

#include <array>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {
namespace {

bool command_available(const wchar_t* executable) {
    std::array<wchar_t, 32768> resolved{};
    const DWORD length = SearchPathW(
        nullptr,
        executable,
        nullptr,
        static_cast<DWORD>(resolved.size()),
        resolved.data(),
        nullptr);
    return length > 0U && length < resolved.size();
}

std::optional<std::filesystem::path> environment_path(
    const wchar_t* name) {
    const DWORD required = GetEnvironmentVariableW(
        name, nullptr, 0U);
    if (required == 0U || required > 32768U) {
        return std::nullopt;
    }
    std::vector<wchar_t> value(required);
    const DWORD written = GetEnvironmentVariableW(
        name, value.data(), required);
    if (written == 0U || written >= required) {
        return std::nullopt;
    }
    return std::filesystem::path(
        std::wstring(value.data(), written));
}

bool regular_file(const std::filesystem::path& path) {
    std::error_code error;
    return std::filesystem::is_regular_file(path, error) &&
           !error;
}

bool chromium_runtime_detected() {
    if (command_available(L"chrome.exe") ||
        command_available(L"msedge.exe") ||
        command_available(L"chromium.exe")) {
        return true;
    }
    struct Candidate {
        const wchar_t* environment;
        const wchar_t* relative;
    };
    constexpr Candidate candidates[]{
        {L"PROGRAMFILES",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"PROGRAMFILES(X86)",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"LOCALAPPDATA",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"PROGRAMFILES",
         L"Microsoft\\Edge\\Application\\msedge.exe"},
        {L"PROGRAMFILES(X86)",
         L"Microsoft\\Edge\\Application\\msedge.exe"},
    };
    for (const auto& candidate : candidates) {
        const auto base = environment_path(
            candidate.environment);
        if (base.has_value() &&
            regular_file(*base / candidate.relative)) {
            return true;
        }
    }
    return false;
}

}  // namespace

EnvironmentCapabilityFacts
EnvironmentCapabilityBackend::inspect() const {
    const HWND foreground_before = GetForegroundWindow();
    EnvironmentCapabilityFacts facts{
        chromium_runtime_detected(),
        command_available(L"notepad.exe"),
        command_available(L"ffmpeg.exe"),
        false,
    };
    facts.foreground_unchanged =
        foreground_before == GetForegroundWindow();
    return facts;
}

}  // namespace act::platform::windows
