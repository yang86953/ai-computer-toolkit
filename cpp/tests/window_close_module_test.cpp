#include "components/json.hpp"
#include "modules/window_close_compatibility_module.hpp"
#include "modules/window_close_module.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <windows.h>

#include <future>
#include <iostream>
#include <string>
#include <thread>

namespace {

constexpr UINT fixture_destroy = WM_APP + 51U;

LRESULT CALLBACK close_proc(
    const HWND window,
    const UINT message,
    const WPARAM word,
    const LPARAM value) {
    if (message == WM_CLOSE) {
        DestroyWindow(window);
        return 0;
    }
    if (message == fixture_destroy) {
        DestroyWindow(window);
        return 0;
    }
    if (message == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, word, value);
}

LRESULT CALLBACK ignore_close_proc(
    const HWND window,
    const UINT message,
    const WPARAM word,
    const LPARAM value) {
    if (message == WM_CLOSE) {
        return 0;
    }
    return close_proc(window, message, word, value);
}

HWND create_fixture(
    const wchar_t* class_name,
    const wchar_t* title,
    WNDPROC procedure) {
    WNDCLASSW window_class{};
    window_class.lpfnWndProc = procedure;
    window_class.hInstance = GetModuleHandleW(nullptr);
    window_class.lpszClassName = class_name;
    if (RegisterClassW(&window_class) == 0U &&
        GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        return nullptr;
    }
    const HWND window = CreateWindowExW(
        WS_EX_NOACTIVATE,
        class_name,
        title,
        WS_OVERLAPPEDWINDOW,
        40,
        40,
        320,
        160,
        nullptr,
        nullptr,
        window_class.hInstance,
        nullptr);
    if (window != nullptr) {
        ShowWindow(window, SW_SHOWNOACTIVATE);
    }
    return window;
}

struct Fixture {
    HWND window;
    std::thread thread;
};

Fixture start_fixture(
    const wchar_t* class_name,
    const wchar_t* title,
    WNDPROC procedure) {
    std::promise<HWND> ready;
    auto future = ready.get_future();
    std::thread thread(
        [class_name, title, procedure, &ready] {
            const HWND window =
                create_fixture(class_name, title, procedure);
            ready.set_value(window);
            if (window == nullptr) {
                return;
            }
            MSG message{};
            while (GetMessageW(
                       &message, nullptr, 0U, 0U) > 0) {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        });
    return Fixture{future.get(), std::move(thread)};
}

std::string session_for_title(const std::string& title) {
    const auto windows =
        act::platform::windows::DiscoveryBackend()
            .enumerate_windows(4096U);
    for (const auto& window : windows) {
        if (window.title == title) {
            return window.session_id;
        }
    }
    return {};
}

bool false_field(
    const act::components::Json& value,
    const char* key) {
    const auto* field = value.find(key);
    return field != nullptr &&
           field->bool_value() != nullptr &&
           !*field->bool_value();
}

}  // namespace

int main() {
    const HWND foreground_before = GetForegroundWindow();
    auto close_fixture = start_fixture(
        L"ActWindowCloseFixture",
        L"ACT_WINDOW_CLOSE_MODULE_SUCCESS",
        close_proc);
    if (close_fixture.window == nullptr) {
        close_fixture.thread.join();
        return 2;
    }
    const std::string target =
        session_for_title("ACT_WINDOW_CLOSE_MODULE_SUCCESS");
    act::modules::WindowCloseModule module;
    const auto unconfirmed = module.close(
        "s2:w:0000000000000000", false, 2000U);
    const bool confirmation_first =
        !unconfirmed.ok &&
        unconfirmed.error_code == "CONFIRMATION_REQUIRED" &&
        IsWindow(close_fixture.window) != FALSE;
    const auto stale = module.close(
        "s2:w:0000000000000000", true, 2000U);
    const bool stale_refused =
        !stale.ok &&
        stale.error_code == "STALE_SESSION" &&
        IsWindow(close_fixture.window) != FALSE;
    const auto invalid =
        module.close(target, true, 0U);
    const bool timeout_bounded =
        !invalid.ok &&
        invalid.error_code == "INVALID_ARGUMENT" &&
        IsWindow(close_fixture.window) != FALSE;
    const auto closed =
        module.close(target, true, 2000U);
    close_fixture.thread.join();
    const bool exact_close =
        closed.ok &&
        closed.data.find("closed") != nullptr &&
        closed.data.find("closed")->bool_value() != nullptr &&
        *closed.data.find("closed")->bool_value();

    act::modules::WindowCloseCompatibilityModule mapper;
    const auto mapped = mapper.map_app_result(closed);
    const bool mapped_safely =
        mapped.ok &&
        mapped.data.find("capability") != nullptr &&
        mapped.data.find("targetId") != nullptr &&
        mapped.data.find("compatibilityShape") != nullptr &&
        mapped.data.dump().find("hwnd") == std::string::npos &&
        mapped.data.dump().find("processId") ==
            std::string::npos;

    auto timeout_fixture = start_fixture(
        L"ActWindowCloseTimeoutFixture",
        L"ACT_WINDOW_CLOSE_MODULE_TIMEOUT",
        ignore_close_proc);
    const std::string timeout_target =
        session_for_title("ACT_WINDOW_CLOSE_MODULE_TIMEOUT");
    const auto timed_out =
        module.close(timeout_target, true, 25U);
    const bool timeout_unknown =
        !timed_out.ok &&
        timed_out.error_code == "TIMEOUT" &&
        timed_out.error_details.has_value() &&
        timed_out.error_details->find("outcome") != nullptr &&
        timed_out.error_details->find("outcome")
                ->string_value() != nullptr &&
        *timed_out.error_details->find("outcome")
             ->string_value() == "unknown" &&
        false_field(*timed_out.error_details, "retrySafe") &&
        IsWindow(timeout_fixture.window) != FALSE;
    PostMessageW(
        timeout_fixture.window, fixture_destroy, 0U, 0);
    timeout_fixture.thread.join();

    const bool foreground_unchanged =
        foreground_before == GetForegroundWindow();
    const bool ok =
        !target.empty() &&
        !timeout_target.empty() &&
        confirmation_first &&
        stale_refused &&
        timeout_bounded &&
        exact_close &&
        mapped_safely &&
        timeout_unknown &&
        foreground_unchanged;
    std::cout << act::components::object({
        {"ok", ok},
        {"selfOwnedFixturesOnly", true},
        {"confirmationFirst", confirmation_first},
        {"staleTargetRefused", stale_refused},
        {"timeoutBounded", timeout_bounded},
        {"exactOpaqueTarget", !target.empty()},
        {"closed", exact_close},
        {"timeoutOutcomeUnknown", timeout_unknown},
        {"retrySafe", false},
        {"foregroundUnchanged", foreground_unchanged},
        {"facadeMappedSafely", mapped_safely},
        {"userWindowsClosed", 0},
    }).dump() << '\n';
    return ok ? 0 : 2;
}
