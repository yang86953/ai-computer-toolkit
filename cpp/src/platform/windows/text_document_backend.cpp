#include "platform/windows/text_document_backend.hpp"

#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <tlhelp32.h>

#include <algorithm>
#include <chrono>
#include <limits>
#include <vector>

namespace act::platform::windows {
namespace {

std::filesystem::path notepad_path() {
    std::vector<wchar_t> buffer(32768U);
    const DWORD length = GetEnvironmentVariableW(
        L"WINDIR", buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0U || length >= buffer.size()) {
        return {};
    }
    return std::filesystem::path(
        std::wstring(buffer.data(), length)) /
        L"System32" / L"notepad.exe";
}

bool write_all(
    const HANDLE file,
    const std::string_view text) {
    std::size_t offset = 0U;
    while (offset < text.size()) {
        const auto remaining = text.size() - offset;
        const DWORD requested = static_cast<DWORD>(
            std::min<std::size_t>(
                remaining,
                std::numeric_limits<DWORD>::max()));
        DWORD written = 0U;
        if (WriteFile(
                file,
                text.data() + offset,
                requested,
                &written,
                nullptr) == FALSE ||
            written == 0U) {
            return false;
        }
        offset += written;
    }
    return FlushFileBuffers(file) != FALSE;
}

std::string read_all(
    const std::filesystem::path& path,
    bool& ok) {
    ok = false;
    const HANDLE file = CreateFileW(
        path.c_str(),
        GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return {};
    }
    LARGE_INTEGER size{};
    if (GetFileSizeEx(file, &size) == FALSE ||
        size.QuadPart < 0 ||
        size.QuadPart > 1048576) {
        CloseHandle(file);
        return {};
    }
    std::string result(
        static_cast<std::size_t>(size.QuadPart), '\0');
    DWORD read = 0U;
    const BOOL read_ok = result.empty()
        ? TRUE
        : ReadFile(
              file,
              result.data(),
              static_cast<DWORD>(result.size()),
              &read,
              nullptr);
    CloseHandle(file);
    ok = read_ok != FALSE &&
         read == static_cast<DWORD>(result.size());
    return result;
}

}  // namespace

bool TextDocumentBackend::runtime_available() const {
    const auto path = notepad_path();
    return !path.empty() &&
           GetFileAttributesW(path.c_str()) !=
               INVALID_FILE_ATTRIBUTES;
}

bool TextDocumentBackend::existing_notepad_process() const {
    const HANDLE snapshot = CreateToolhelp32Snapshot(
        TH32CS_SNAPPROCESS, 0U);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return true;
    }
    PROCESSENTRY32W process{};
    process.dwSize = sizeof(process);
    bool found = false;
    if (Process32FirstW(snapshot, &process) != FALSE) {
        do {
            if (_wcsicmp(
                    process.szExeFile,
                    L"notepad.exe") == 0) {
                found = true;
                break;
            }
        } while (Process32NextW(snapshot, &process) != FALSE);
    }
    CloseHandle(snapshot);
    return found;
}

TextArtifactResult
TextDocumentBackend::create_temporary_artifact(
    const std::string_view text) const {
    std::error_code error;
    const auto temp = std::filesystem::temp_directory_path(error);
    if (error) {
        return TextArtifactResult{
            false,
            "DOCUMENT_CREATE_FAILED",
            "The temporary document directory is unavailable.",
            {},
            {},
        };
    }
    const auto stamp = std::chrono::steady_clock::now()
                           .time_since_epoch()
                           .count();
    for (std::uint32_t attempt = 0U; attempt < 32U; ++attempt) {
        const auto filename =
            L"ai-computer-toolkit-notepad-" +
            std::to_wstring(GetCurrentProcessId()) + L"-" +
            std::to_wstring(stamp) + L"-" +
            std::to_wstring(attempt) + L".txt";
        const auto path = temp / filename;
        const HANDLE file = CreateFileW(
            path.c_str(),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            nullptr,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            nullptr);
        if (file == INVALID_HANDLE_VALUE) {
            if (GetLastError() == ERROR_FILE_EXISTS ||
                GetLastError() == ERROR_ALREADY_EXISTS) {
                continue;
            }
            return TextArtifactResult{
                false,
                "DOCUMENT_CREATE_FAILED",
                "The new text artifact could not be created atomically.",
                {},
                {},
            };
        }
        const bool written = write_all(file, text);
        CloseHandle(file);
        if (!written) {
            DeleteFileW(path.c_str());
            return TextArtifactResult{
                false,
                "DOCUMENT_CREATE_FAILED",
                "The new text artifact could not be flushed.",
                {},
                {},
            };
        }
        bool read_ok = false;
        auto verified = read_all(path, read_ok);
        if (!read_ok || verified != text) {
            DeleteFileW(path.c_str());
            return TextArtifactResult{
                false,
                "TEXT_VERIFICATION_FAILED",
                "The new text artifact did not match the requested UTF-8.",
                {},
                {},
            };
        }
        return TextArtifactResult{
            true, {}, {}, path, std::move(verified)};
    }
    return TextArtifactResult{
        false,
        "DOCUMENT_CREATE_FAILED",
        "No collision-free temporary artifact name was available.",
        {},
        {},
    };
}

ApplicationLaunchResult TextDocumentBackend::open_in_notepad(
    const std::filesystem::path& path) const {
    const auto executable = notepad_path();
    if (executable.empty() ||
        GetFileAttributesW(executable.c_str()) ==
            INVALID_FILE_ATTRIBUTES) {
        return ApplicationLaunchResult{
            false,
            "NOTEPAD_UNAVAILABLE",
            "The system Notepad runtime is unavailable.",
            0U,
            true,
        };
    }
    std::wstring command =
        L"\"" + executable.native() + L"\" \"" +
        path.native() + L"\"";
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESHOWWINDOW;
    startup.wShowWindow = SW_SHOWNOACTIVATE;
    PROCESS_INFORMATION process{};
    const HWND foreground_before = GetForegroundWindow();
    const HANDLE job = CreateJobObjectW(nullptr, nullptr);
    if (job == nullptr) {
        return ApplicationLaunchResult{
            false,
            "NOTEPAD_START_FAILED",
            "A rollback boundary for the Notepad launch could not be created.",
            0U,
            GetForegroundWindow() == foreground_before,
        };
    }
    if (CreateProcessW(
            executable.c_str(),
            command.data(),
            nullptr,
            nullptr,
            FALSE,
            CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            nullptr,
            nullptr,
            &startup,
            &process) == FALSE) {
        CloseHandle(job);
        return ApplicationLaunchResult{
            false,
            "NOTEPAD_START_FAILED",
            "The system Notepad process could not be started.",
            0U,
            GetForegroundWindow() == foreground_before,
        };
    }
    if (AssignProcessToJobObject(job, process.hProcess) == FALSE ||
        ResumeThread(process.hThread) ==
            static_cast<DWORD>(-1)) {
        TerminateJobObject(job, 1U);
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
        CloseHandle(job);
        return ApplicationLaunchResult{
            false,
            "NOTEPAD_START_FAILED",
            "The Notepad launch could not enter its rollback boundary.",
            process.dwProcessId,
            GetForegroundWindow() == foreground_before,
        };
    }
    CloseHandle(process.hThread);
    WaitForInputIdle(process.hProcess, 1500U);
    Sleep(250U);
    const bool unchanged =
        GetForegroundWindow() == foreground_before;
    if (!unchanged) {
        TerminateJobObject(job, 1U);
        WaitForSingleObject(process.hProcess, 1000U);
    }
    CloseHandle(process.hProcess);
    CloseHandle(job);
    if (!unchanged) {
        return ApplicationLaunchResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Notepad changed the foreground target; the launch was rolled back.",
            process.dwProcessId,
            false,
        };
    }
    return ApplicationLaunchResult{
        true, {}, {}, process.dwProcessId, true};
}

void TextDocumentBackend::remove_owned_artifact(
    const std::filesystem::path& path) const {
    const std::wstring name = path.filename().native();
    if (name.starts_with(L"ai-computer-toolkit-notepad-") &&
        path.extension() == L".txt") {
        DeleteFileW(path.c_str());
    }
}

}  // namespace act::platform::windows
