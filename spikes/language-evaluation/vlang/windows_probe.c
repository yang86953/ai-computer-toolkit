#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <tlhelp32.h>
#include <uiautomation.h>

#include <stdint.h>

uint32_t act_probe_process_count(void) {
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return 0;
    }
    PROCESSENTRY32W entry = {0};
    entry.dwSize = sizeof(entry);
    uint32_t count = 0;
    if (Process32FirstW(snapshot, &entry)) {
        do {
            ++count;
        } while (Process32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);
    return count;
}

static BOOL CALLBACK act_count_window(HWND window, LPARAM state) {
    (void)window;
    ++(*(uint32_t*)state);
    return TRUE;
}

uint32_t act_probe_window_count(void) {
    uint32_t count = 0;
    return EnumWindows(act_count_window, (LPARAM)&count) ? count : 0;
}

int act_probe_uia_available(void) {
    HRESULT apartment = CoInitializeEx(NULL, COINIT_MULTITHREADED);
    if (FAILED(apartment)) {
        return 0;
    }
    IUIAutomation* automation = NULL;
    HRESULT result = CoCreateInstance(
        &CLSID_CUIAutomation,
        NULL,
        CLSCTX_INPROC_SERVER,
        &IID_IUIAutomation,
        (void**)&automation);
    if (automation != NULL) {
        IUIAutomation_Release(automation);
    }
    CoUninitialize();
    return SUCCEEDED(result) ? 1 : 0;
}

uint64_t act_probe_foreground(void) {
    return (uint64_t)(uintptr_t)GetForegroundWindow();
}
