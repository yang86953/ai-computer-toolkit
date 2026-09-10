#include "platform/windows/window_close_backend.hpp"

#include <windows.h>

namespace act::platform::windows {

WindowCloseResult WindowCloseBackend::close(
    const WindowRecord& target,
    const std::uint32_t timeout_ms) const {
    const HWND window =
        reinterpret_cast<HWND>(target.native_window);
    DWORD process_id = 0U;
    if (IsWindow(window) == FALSE ||
        GetWindowThreadProcessId(window, &process_id) == 0U ||
        process_id != target.native_process_id) {
        return WindowCloseResult{
            false,
            BackendError{
                "STALE_SESSION",
                "The exact window target is no longer available.",
            },
        };
    }
    SetLastError(ERROR_SUCCESS);
    if (PostMessageW(window, WM_CLOSE, 0U, 0) == FALSE) {
        const DWORD error = GetLastError();
        return WindowCloseResult{
            false,
            BackendError{
                error == ERROR_ACCESS_DENIED
                    ? "PERMISSION_DENIED"
                    : "OPERATION_FAILED",
                error == ERROR_ACCESS_DENIED
                    ? "Windows denied the exact window close request."
                    : "The exact window rejected the close request.",
            },
        };
    }
    const ULONGLONG deadline =
        GetTickCount64() + timeout_ms;
    while (IsWindow(window) != FALSE) {
        if (GetTickCount64() >= deadline) {
            return WindowCloseResult{
                false,
                BackendError{
                    "TIMEOUT",
                    "The exact window remained available after the close "
                    "timeout.",
                },
            };
        }
        Sleep(25U);
    }
    return WindowCloseResult{true, std::nullopt};
}

}  // namespace act::platform::windows
