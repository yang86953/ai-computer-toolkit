#include <windows.h>

#include <filesystem>
#include <fstream>

int WINAPI wWinMain(
    HINSTANCE,
    HINSTANCE,
    PWSTR,
    int) {
    std::wstring path(32768U, L'\0');
    const DWORD length = GetModuleFileNameW(
        nullptr,
        path.data(),
        static_cast<DWORD>(path.size()));
    if (length == 0U || length >= path.size()) {
        return 2;
    }
    path.resize(length);
    const auto ready =
        std::filesystem::path(path).parent_path() /
        L"act-application-launch-fixture.ready";
    std::ofstream output(
        ready, std::ios::binary | std::ios::trunc);
    output << "launched";
    return output.good() ? 0 : 3;
}
