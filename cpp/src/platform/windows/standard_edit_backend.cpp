#include "platform/windows/standard_edit_backend.hpp"

#include "components/opaque_id.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <array>
#include <cwchar>
#include <sstream>
#include <string_view>

namespace act::platform::windows {
namespace {

std::string process_name_and_creation(
    const DWORD process_id,
    std::uint64_t& creation_time) {
    creation_time = 0U;
    HANDLE process = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (process == nullptr) {
        return "unavailable";
    }
    FILETIME created{};
    FILETIME exited{};
    FILETIME kernel{};
    FILETIME user{};
    if (GetProcessTimes(
            process, &created, &exited, &kernel, &user) != FALSE) {
        creation_time =
            (static_cast<std::uint64_t>(
                 created.dwHighDateTime)
             << 32U) |
            static_cast<std::uint64_t>(
                created.dwLowDateTime);
    }
    std::array<wchar_t, 32768> path{};
    DWORD length = static_cast<DWORD>(path.size());
    std::string name = "unavailable";
    if (QueryFullProcessImageNameW(
            process, 0U, path.data(), &length) != FALSE) {
        std::wstring_view full(path.data(), length);
        const std::size_t separator =
            full.find_last_of(L"\\/");
        if (separator != std::wstring_view::npos) {
            full.remove_prefix(separator + 1U);
        }
        name = utf8(full);
    }
    CloseHandle(process);
    return name;
}

std::string control_session_id(
    const HWND control,
    const DWORD process_id,
    const std::uint64_t creation_time) {
    std::ostringstream identity;
    identity << static_cast<unsigned long>(process_id)
             << ':'
             << reinterpret_cast<std::uintptr_t>(control)
             << ':'
             << creation_time;
    return components::opaque_id('c', identity.str());
}

bool standard_edit(const HWND control) {
    std::array<wchar_t, 256> class_name{};
    const int length = GetClassNameW(
        control,
        class_name.data(),
        static_cast<int>(class_name.size()));
    return length == 4 &&
           _wcsicmp(class_name.data(), L"Edit") == 0;
}

struct EnumerationContext {
    std::size_t maximum_items;
    std::vector<StandardEditRecord>* records;
};

BOOL CALLBACK collect_control(
    const HWND control,
    const LPARAM data) {
    auto* context =
        reinterpret_cast<EnumerationContext*>(data);
    if (context->records->size() >=
        context->maximum_items) {
        return FALSE;
    }
    if (!standard_edit(control)) {
        return TRUE;
    }
    DWORD process_id = 0U;
    GetWindowThreadProcessId(control, &process_id);
    std::uint64_t creation_time = 0U;
    std::string application_name =
        process_name_and_creation(
            process_id, creation_time);
    context->records->push_back(StandardEditRecord{
        control_session_id(
            control, process_id, creation_time),
        std::move(application_name),
        IsWindowVisible(control) != FALSE,
        reinterpret_cast<std::uintptr_t>(control),
        static_cast<std::uint32_t>(process_id),
    });
    return TRUE;
}

BOOL CALLBACK collect_top_level(
    const HWND window,
    const LPARAM data) {
    auto* context =
        reinterpret_cast<EnumerationContext*>(data);
    if (context->records->size() >=
        context->maximum_items) {
        return FALSE;
    }
    EnumChildWindows(window, collect_control, data);
    return context->records->size() <
        context->maximum_items;
}

std::optional<BackendError> send_timeout(
    const HWND control,
    const UINT message,
    const WPARAM word,
    const LPARAM value,
    const std::uint32_t timeout_ms,
    DWORD_PTR& result) {
    SetLastError(ERROR_SUCCESS);
    if (SendMessageTimeoutW(
            control,
            message,
            word,
            value,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            timeout_ms,
            &result) != 0) {
        return std::nullopt;
    }
    const DWORD error = GetLastError();
    if (error == ERROR_TIMEOUT) {
        return BackendError{
            "TIMEOUT",
            "The standard Edit control did not respond before timeout.",
        };
    }
    if (error == ERROR_ACCESS_DENIED) {
        return BackendError{
            "PERMISSION_DENIED",
            "Windows denied the standard Edit message.",
        };
    }
    return BackendError{
        "BACKGROUND_OPERATION_UNAVAILABLE",
        "The standard Edit control rejected the certified message.",
    };
}

}  // namespace

std::vector<StandardEditRecord>
StandardEditBackend::enumerate(
    const std::size_t maximum_items) const {
    std::vector<StandardEditRecord> records;
    records.reserve(maximum_items);
    EnumerationContext context{
        maximum_items, &records};
    EnumWindows(
        collect_top_level,
        reinterpret_cast<LPARAM>(&context));
    return records;
}

StandardEditWriteResult StandardEditBackend::set_text(
    const StandardEditRecord& control,
    const std::wstring& text,
    const std::uint32_t timeout_ms) const {
    const HWND window =
        reinterpret_cast<HWND>(control.native_control);
    if (IsWindow(window) == FALSE ||
        !standard_edit(window)) {
        return StandardEditWriteResult{
            false,
            BackendError{
                "STALE_SESSION",
                "The exact standard Edit target is no longer available.",
            },
        };
    }
    DWORD process_id = 0U;
    GetWindowThreadProcessId(window, &process_id);
    if (process_id != control.native_process_id) {
        return StandardEditWriteResult{
            false,
            BackendError{
                "STALE_SESSION",
                "The standard Edit target identity changed.",
            },
        };
    }

    DWORD_PTR ignored = 0U;
    auto error = send_timeout(
        window,
        WM_SETTEXT,
        0U,
        reinterpret_cast<LPARAM>(text.c_str()),
        timeout_ms,
        ignored);
    if (error.has_value()) {
        return StandardEditWriteResult{
            false, std::move(error)};
    }

    DWORD_PTR length_result = 0U;
    error = send_timeout(
        window,
        WM_GETTEXTLENGTH,
        0U,
        0,
        timeout_ms,
        length_result);
    if (error.has_value() ||
        length_result > 65536U) {
        return StandardEditWriteResult{
            false,
            error.has_value()
                ? std::move(error)
                : std::optional<BackendError>(
                      BackendError{
                          "OPERATION_FAILED",
                          "Edit readback exceeded the bounded length.",
                      })};
    }
    std::wstring readback(
        static_cast<std::size_t>(length_result) + 1U,
        L'\0');
    DWORD_PTR copied = 0U;
    error = send_timeout(
        window,
        WM_GETTEXT,
        static_cast<WPARAM>(readback.size()),
        reinterpret_cast<LPARAM>(readback.data()),
        timeout_ms,
        copied);
    if (error.has_value()) {
        return StandardEditWriteResult{
            false, std::move(error)};
    }
    readback.resize(static_cast<std::size_t>(copied));
    if (readback != text) {
        return StandardEditWriteResult{
            false,
            BackendError{
                "OPERATION_FAILED",
                "Standard Edit write readback did not match.",
            },
        };
    }
    return StandardEditWriteResult{
        true, std::nullopt};
}

}  // namespace act::platform::windows
