#include "components/json.hpp"
#include "modules/screenshot_compatibility_module.hpp"
#include "modules/screenshot_module.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <filesystem>
#include <iostream>
#include <string>

namespace {

LRESULT CALLBACK window_proc(
    const HWND window,
    const UINT message,
    const WPARAM wparam,
    const LPARAM lparam) {
    return DefWindowProcW(window, message, wparam, lparam);
}

class TestWindow final {
public:
    TestWindow() {
        instance_ = GetModuleHandleW(nullptr);
        class_name_ =
            L"ActScreenshotPolicy-" +
            std::to_wstring(GetCurrentProcessId());
        WNDCLASSW window_class{};
        window_class.lpfnWndProc = window_proc;
        window_class.hInstance = instance_;
        window_class.lpszClassName = class_name_.c_str();
        atom_ = RegisterClassW(&window_class);
        window_ = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name_.c_str(),
            L"ACT screenshot module policy fixture",
            WS_OVERLAPPEDWINDOW,
            32,
            32,
            160,
            100,
            nullptr,
            nullptr,
            instance_,
            nullptr);
        ShowWindow(window_, SW_SHOWNOACTIVATE);
    }

    ~TestWindow() {
        if (window_ != nullptr) {
            DestroyWindow(window_);
        }
        if (atom_ != 0U) {
            UnregisterClassW(class_name_.c_str(), instance_);
        }
    }

    [[nodiscard]] bool valid() const {
        return atom_ != 0U && window_ != nullptr;
    }

private:
    HINSTANCE instance_ = nullptr;
    std::wstring class_name_;
    ATOM atom_ = 0U;
    HWND window_ = nullptr;
};

std::filesystem::path executable_directory() {
    std::array<wchar_t, 32768> path{};
    const DWORD length = GetModuleFileNameW(
        nullptr, path.data(), static_cast<DWORD>(path.size()));
    if (length == 0U || length >= path.size()) {
        return {};
    }
    return std::filesystem::path(
        std::wstring(path.data(), length)).parent_path();
}

bool write_placeholder(const std::filesystem::path& path) {
    const HANDLE file = CreateFileW(
        path.c_str(),
        GENERIC_WRITE,
        0U,
        nullptr,
        CREATE_NEW,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return false;
    }
    constexpr std::array<std::uint8_t, 1> marker{0U};
    DWORD written = 0U;
    const bool ok =
        WriteFile(
            file,
            marker.data(),
            static_cast<DWORD>(marker.size()),
            &written,
            nullptr) != FALSE &&
        written == marker.size();
    CloseHandle(file);
    return ok;
}

}  // namespace

int main() {
    const auto output_directory =
        executable_directory() / "screenshot-module-fixtures";
    std::error_code filesystem_error;
    std::filesystem::create_directory(
        output_directory, filesystem_error);
    if (filesystem_error) {
        std::cerr << "fixture directory could not be created\n";
        return 1;
    }
    const auto absent_path = output_directory / "absent.png";
    const auto existing_path = output_directory / "existing.png";
    const auto invalid_path = output_directory / "invalid.jpg";
    std::filesystem::remove(absent_path, filesystem_error);
    std::filesystem::remove(existing_path, filesystem_error);

    const act::modules::ScreenshotModule module;
    const auto unconfirmed = module.capture(
        "not-an-opaque-target",
        act::platform::windows::utf8(absent_path.native()),
        false,
        false,
        100U);
    if (unconfirmed.ok ||
        unconfirmed.error_code != "CONFIRMATION_REQUIRED" ||
        std::filesystem::exists(absent_path)) {
        std::cerr << "confirmation was not the first gate\n";
        return 1;
    }

    const auto stale = module.capture(
        "s2:w:0000000000000000",
        act::platform::windows::utf8(absent_path.native()),
        true,
        false,
        100U);
    if (stale.ok ||
        stale.error_code != "STALE_SESSION" ||
        std::filesystem::exists(absent_path)) {
        std::cerr << "stale exact target was not rejected\n";
        return 1;
    }

    TestWindow fixture;
    if (!fixture.valid()) {
        std::cerr << "test window could not be created\n";
        return 1;
    }
    const act::platform::windows::DiscoveryBackend discovery;
    const auto windows = discovery.enumerate_windows(4096U);
    const auto match = std::find_if(
        windows.begin(),
        windows.end(),
        [](const auto& window) {
            return window.title ==
                "ACT screenshot module policy fixture";
        });
    if (match == windows.end()) {
        std::cerr << "test window did not receive an opaque target\n";
        return 1;
    }

    const auto invalid_timeout = module.capture(
        match->session_id,
        act::platform::windows::utf8(absent_path.native()),
        true,
        false,
        100U);
    if (invalid_timeout.ok ||
        invalid_timeout.error_code != "INVALID_ARGUMENT" ||
        std::filesystem::exists(absent_path)) {
        std::cerr << "invalid timeout reached output or capture\n";
        return 1;
    }

    const auto invalid = module.capture(
        match->session_id,
        act::platform::windows::utf8(invalid_path.native()),
        true,
        false,
        1000U);
    if (invalid.ok ||
        invalid.error_code != "INVALID_ARGUMENT" ||
        std::filesystem::exists(invalid_path)) {
        std::cerr << "invalid output path reached capture\n";
        return 1;
    }

    if (!write_placeholder(existing_path)) {
        std::cerr << "overwrite fixture could not be created\n";
        return 1;
    }
    const auto overwrite = module.capture(
        match->session_id,
        act::platform::windows::utf8(existing_path.native()),
        true,
        false,
        1000U);
    if (overwrite.ok ||
        overwrite.error_code !=
            "OVERWRITE_CONFIRMATION_REQUIRED" ||
        std::filesystem::file_size(existing_path) != 1U) {
        std::cerr << "overwrite gate reached capture or changed file\n";
        return 1;
    }

    std::filesystem::remove(existing_path, filesystem_error);
    std::filesystem::remove(output_directory, filesystem_error);
    if (std::filesystem::exists(absent_path) ||
        std::filesystem::exists(existing_path) ||
        std::filesystem::exists(output_directory)) {
        std::cerr << "policy fixtures were not cleaned\n";
        return 1;
    }

    const act::modules::ScreenshotCompatibilityModule compatibility;
    const auto mapped = compatibility.map_desktop_result(
        act::modules::ModuleResult{
            true,
            {},
            {},
            act::components::object({
                {"capability", "window.screenshot@1"},
                {"targetId", "s2:w:0123456789abcdef"},
                {"path", "C:\\fixture\\capture.png"},
                {"bytes", 1234},
                {"width", 320},
                {"height", 200},
                {"deviceDriver", "hardware"},
                {"foregroundUnchanged", true},
            }),
        });
    const std::string mapped_json = mapped.data.dump();
    const bool success_mapped =
        mapped.ok &&
        mapped.data.find("app") != nullptr &&
        *mapped.data.find("app")->string_value() == "desktop" &&
        mapped_json.find("s2:w:0123456789abcdef") !=
            std::string::npos &&
        mapped_json.find("hwnd") == std::string::npos &&
        mapped_json.find("processId") == std::string::npos;
    const auto app_mapped =
        compatibility.map_app_facade_result(
            act::modules::ModuleResult{
                true,
                {},
                {},
                act::components::object({
                    {"capability", "window.screenshot@1"},
                    {"targetId", "s2:w:0123456789abcdef"},
                    {"path", "C:\\fixture\\capture.png"},
                    {"bytes", 1234},
                    {"width", 320},
                    {"height", 200},
                    {"deviceDriver", "hardware"},
                    {"foregroundUnchanged", true},
                }),
            });
    const std::string app_json = app_mapped.data.dump();
    const bool app_mapped_safely =
        app_mapped.ok &&
        app_mapped.data.find("app") != nullptr &&
        *app_mapped.data.find("app")->string_value() == "app" &&
        app_json.find("window.screenshot@1") !=
            std::string::npos &&
        app_json.find("s2:w:0123456789abcdef") !=
            std::string::npos &&
        app_json.find("captureMethod") == std::string::npos &&
        app_json.find("deviceDriver") == std::string::npos &&
        app_json.find("hwnd") == std::string::npos;
    const auto mapped_error = [&compatibility](
                                  const char* code,
                                  const char* expected,
                                  act::components::Json details =
                                      nullptr) {
        auto result = compatibility.map_desktop_result(
            act::modules::ModuleResult{
                false,
                code,
                "fixture",
                nullptr,
                std::move(details),
            });
        return !result.ok &&
               result.error_code == expected;
    };
    const bool errors_mapped =
        mapped_error("STALE_SESSION", "TARGET_NOT_FOUND") &&
        mapped_error("TIMEOUT", "CAPTURE_TIMEOUT") &&
        mapped_error(
            "RESOURCE_LIMIT_EXCEEDED",
            "CAPTURE_READBACK_FAILED") &&
        mapped_error(
            "HOST_INTERFERENCE_DETECTED",
            "FOREGROUND_CHANGED") &&
        mapped_error(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "CAPTURE_TARGET_HIDDEN",
            act::components::object({
                {"targetState", "hidden"},
            })) &&
        mapped_error(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "CAPTURE_TARGET_MINIMIZED",
            act::components::object({
                {"targetState", "minimized"},
            }));
    if (!success_mapped ||
        !app_mapped_safely ||
        !errors_mapped) {
        std::cerr << "screenshot compatibility mapping failed\n";
        return 1;
    }

    std::cout
        << "{\"ok\":true,\"confirmationFirst\":true,"
           "\"staleTargetRefused\":true,"
           "\"invalidTimeoutRefused\":true,"
           "\"invalidPathRefused\":true,"
           "\"overwriteRefused\":true,"
           "\"successMappedSafely\":true,"
           "\"appFacadeMappedSafely\":true,"
           "\"errorsMappedSafely\":true,"
           "\"routeDecisionDelegatedToSystem\":true,"
           "\"realCaptureAttempted\":false,"
           "\"userFilesTouched\":0}\n";
    return 0;
}
