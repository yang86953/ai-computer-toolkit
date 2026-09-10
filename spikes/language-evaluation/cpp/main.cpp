#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <tlhelp32.h>
#include <uiautomation.h>

#include <cstdint>
#include <iostream>

namespace {

std::uint32_t process_count() {
    const HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return 0;
    }

    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    std::uint32_t count = 0;
    if (Process32FirstW(snapshot, &entry)) {
        do {
            ++count;
        } while (Process32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);
    return count;
}

BOOL CALLBACK count_window(HWND, LPARAM state) {
    auto* count = reinterpret_cast<std::uint32_t*>(state);
    ++(*count);
    return TRUE;
}

std::uint32_t window_count() {
    std::uint32_t count = 0;
    if (!EnumWindows(count_window, reinterpret_cast<LPARAM>(&count))) {
        return 0;
    }
    return count;
}

bool uia_available() {
    IUIAutomation* automation = nullptr;
    const HRESULT result = CoCreateInstance(
        CLSID_CUIAutomation,
        nullptr,
        CLSCTX_INPROC_SERVER,
        IID_IUIAutomation,
        reinterpret_cast<void**>(&automation));
    if (automation != nullptr) {
        automation->Release();
    }
    return SUCCEEDED(result);
}

const char* json_bool(bool value) {
    return value ? "true" : "false";
}

}  // namespace

int main() {
    const HWND foreground_before = GetForegroundWindow();
    const HRESULT apartment = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const bool apartment_owned = SUCCEEDED(apartment);
    const bool uia_initialized = apartment_owned && uia_available();
    if (apartment_owned) {
        CoUninitialize();
    }
    const auto processes = process_count();
    const auto windows = window_count();
    const HWND foreground_after = GetForegroundWindow();
    const bool foreground_unchanged = foreground_before == foreground_after;
    const bool ok = processes > 0 && windows > 0 && uia_initialized && foreground_unchanged;

    std::cout
        << "{\"ok\":" << json_bool(ok)
        << ",\"contractVersion\":\"act/language-probe/v1\""
        << ",\"implementationLanguage\":\"cpp\""
        << ",\"platform\":\"windows\""
        << ",\"observations\":{\"processCount\":" << processes
        << ",\"topLevelWindowCount\":" << windows
        << ",\"uiaClientInitialized\":" << json_bool(uia_initialized)
        << ",\"foregroundUnchanged\":" << json_bool(foreground_unchanged)
        << "},\"safety\":{\"readOnly\":true,\"inputSent\":false,\"windowActivated\":false}}\n";
    return ok ? 0 : 2;
}
