#include "platform/windows/application_launch_backend.hpp"

#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <shellapi.h>
#include <shlobj.h>

namespace act::platform::windows {

ApplicationLaunchResult ApplicationLaunchBackend::launch(
    const InstalledApplicationRecord& application) const {
    if (application.launch_provider !=
            ApplicationLaunchProvider::shell_item ||
        application.launch_identity.empty()) {
        return ApplicationLaunchResult{
            std::nullopt,
            BackendError{
                "CAPABILITY_UNAVAILABLE",
                "The exact installed application has no certified "
                "public Shell launch identity.",
            },
        };
    }
    const HRESULT initialized =
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    const bool should_uninitialize = SUCCEEDED(initialized);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        return ApplicationLaunchResult{
            std::nullopt,
            BackendError{
                "SHELL_PROVIDER_UNAVAILABLE",
                "The Windows Shell apartment could not be initialized.",
            },
        };
    }
    PIDLIST_ABSOLUTE item_id = nullptr;
    const std::wstring identity = wide(application.launch_identity);
    const HRESULT parsed = SHParseDisplayName(
        identity.c_str(), nullptr, &item_id, 0U, nullptr);
    if (FAILED(parsed) || item_id == nullptr) {
        if (should_uninitialize) {
            CoUninitialize();
        }
        return ApplicationLaunchResult{
            std::nullopt,
            BackendError{
                parsed == E_ACCESSDENIED
                    ? "PERMISSION_DENIED"
                    : "STALE_SESSION",
                "The certified Shell application identity no longer "
                "resolves.",
            },
        };
    }
    SHELLEXECUTEINFOW request{};
    request.cbSize = sizeof(request);
    request.fMask =
        SEE_MASK_IDLIST | SEE_MASK_NOCLOSEPROCESS |
        SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
    request.lpIDList = item_id;
    request.nShow = SW_SHOWNORMAL;
    SetLastError(ERROR_SUCCESS);
    const BOOL launched = ShellExecuteExW(&request);
    const DWORD error = GetLastError();
    CoTaskMemFree(item_id);
    const bool process_observed = request.hProcess != nullptr;
    if (request.hProcess != nullptr) {
        CloseHandle(request.hProcess);
    }
    if (should_uninitialize) {
        CoUninitialize();
    }
    if (launched == FALSE) {
        return ApplicationLaunchResult{
            std::nullopt,
            BackendError{
                error == ERROR_ACCESS_DENIED
                    ? "PERMISSION_DENIED"
                    : "APPLICATION_LAUNCH_FAILED",
                "Windows Shell rejected the exact installed application.",
            },
        };
    }
    return ApplicationLaunchResult{
        ApplicationLaunchEvidence{
            true,
            process_observed,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
