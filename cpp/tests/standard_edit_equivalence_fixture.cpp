#include <windows.h>

#include <array>
#include <fstream>
#include <string>
#include <string_view>

namespace {

constexpr wchar_t fixture_class[] =
    L"ActStandardEditEquivalenceFixture";
constexpr wchar_t initial_text[] = L"initial";
std::string output_path;
HWND edit_control = nullptr;

std::string utf8(const std::wstring_view text) {
    if (text.empty()) {
        return {};
    }
    const int length = WideCharToMultiByte(
        CP_UTF8,
        WC_ERR_INVALID_CHARS,
        text.data(),
        static_cast<int>(text.size()),
        nullptr,
        0,
        nullptr,
        nullptr);
    if (length <= 0) {
        return {};
    }
    std::string result(static_cast<std::size_t>(length), '\0');
    if (WideCharToMultiByte(
            CP_UTF8,
            WC_ERR_INVALID_CHARS,
            text.data(),
            static_cast<int>(text.size()),
            result.data(),
            length,
            nullptr,
            nullptr) != length) {
        return {};
    }
    return result;
}

LRESULT CALLBACK window_proc(
    const HWND window,
    const UINT message,
    const WPARAM word,
    const LPARAM value) {
    if (message == WM_TIMER) {
        std::array<wchar_t, 65537> buffer{};
        const int length = GetWindowTextW(
            edit_control,
            buffer.data(),
            static_cast<int>(buffer.size()));
        const std::wstring_view text(
            buffer.data(), static_cast<std::size_t>(length));
        if (text != initial_text) {
            const std::string encoded = utf8(text);
            std::ofstream output(
                output_path, std::ios::binary | std::ios::trunc);
            output.write(
                encoded.data(),
                static_cast<std::streamsize>(encoded.size()));
            output.close();
            DestroyWindow(window);
        }
        return 0;
    }
    if (message == WM_CLOSE) {
        DestroyWindow(window);
        return 0;
    }
    if (message == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, word, value);
}

}  // namespace

int main(const int count, char** arguments) {
    if (count != 2) {
        return 2;
    }
    output_path = arguments[1];
    const HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSW window_class{};
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.lpszClassName = fixture_class;
    if (RegisterClassW(&window_class) == 0U) {
        return 3;
    }
    const HWND parent = CreateWindowExW(
        WS_EX_NOACTIVATE,
        fixture_class,
        L"ACT_STANDARD_EDIT_EQUIVALENCE_FIXTURE",
        WS_OVERLAPPED,
        0,
        0,
        360,
        120,
        nullptr,
        nullptr,
        instance,
        nullptr);
    if (parent == nullptr) {
        return 4;
    }
    edit_control = CreateWindowExW(
        0U,
        L"EDIT",
        initial_text,
        WS_CHILD | WS_VISIBLE | ES_LEFT,
        8,
        8,
        320,
        30,
        parent,
        nullptr,
        instance,
        nullptr);
    if (edit_control == nullptr) {
        DestroyWindow(parent);
        return 5;
    }
    ShowWindow(parent, SW_SHOWNOACTIVATE);
    if (SetTimer(parent, 1U, 50U, nullptr) == 0U) {
        DestroyWindow(parent);
        return 6;
    }
    MSG message{};
    while (GetMessageW(&message, nullptr, 0U, 0U) > 0) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return 0;
}
