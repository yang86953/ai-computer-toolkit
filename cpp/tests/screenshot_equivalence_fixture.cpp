#include <windows.h>

#include <array>
#include <iostream>
#include <string>

namespace {

LRESULT CALLBACK fixture_proc(
    const HWND window,
    const UINT message,
    const WPARAM word,
    const LPARAM value) {
    if (message == WM_PAINT) {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        RECT client{};
        GetClientRect(window, &client);
        const int half_x = client.right / 2;
        const int half_y = client.bottom / 2;
        const std::array<COLORREF, 4> colors{
            RGB(24, 72, 160),
            RGB(230, 94, 52),
            RGB(32, 164, 112),
            RGB(244, 202, 60),
        };
        const std::array<RECT, 4> rectangles{{
            {0, 0, half_x, half_y},
            {half_x, 0, client.right, half_y},
            {0, half_y, half_x, client.bottom},
            {half_x, half_y, client.right, client.bottom},
        }};
        for (std::size_t index = 0;
             index < rectangles.size();
             ++index) {
            const HBRUSH brush =
                CreateSolidBrush(colors[index]);
            FillRect(dc, &rectangles[index], brush);
            DeleteObject(brush);
        }
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(255, 255, 255));
        constexpr wchar_t label[] =
            L"ACT RUST C++ WGC EQUIVALENCE";
        TextOutW(
            dc,
            28,
            92,
            label,
            static_cast<int>(std::size(label) - 1U));
        EndPaint(window, &paint);
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

int main(int argc, char** argv) {
    if (argc != 2) {
        return 2;
    }
    const int required = MultiByteToWideChar(
        CP_UTF8, MB_ERR_INVALID_CHARS, argv[1], -1, nullptr, 0);
    if (required <= 1) {
        return 2;
    }
    std::wstring title(
        static_cast<std::size_t>(required), L'\0');
    if (MultiByteToWideChar(
            CP_UTF8,
            MB_ERR_INVALID_CHARS,
            argv[1],
            -1,
            title.data(),
            required) == 0) {
        return 2;
    }
    title.resize(static_cast<std::size_t>(required - 1));

    const HINSTANCE instance = GetModuleHandleW(nullptr);
    constexpr wchar_t class_name[] =
        L"ActScreenshotEquivalenceFixture";
    WNDCLASSW window_class{};
    window_class.lpfnWndProc = fixture_proc;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    window_class.lpszClassName = class_name;
    if (RegisterClassW(&window_class) == 0U &&
        GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        return 3;
    }
    const HWND window = CreateWindowExW(
        WS_EX_NOACTIVATE,
        class_name,
        title.c_str(),
        WS_OVERLAPPEDWINDOW,
        120,
        120,
        420,
        280,
        nullptr,
        nullptr,
        instance,
        nullptr);
    if (window == nullptr) {
        return 3;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    UpdateWindow(window);
    std::cout << "{\"ok\":true}\n" << std::flush;

    MSG message{};
    while (GetMessageW(&message, nullptr, 0U, 0U) > 0) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return 0;
}
