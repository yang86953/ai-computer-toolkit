#include "platform/windows/installed_application_backend.hpp"

#include "components/opaque_id.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <cwctype>
#include <string_view>
#include <unordered_set>
#include <vector>

namespace act::platform::windows {
namespace {

constexpr wchar_t uninstall_key[] =
    L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

std::wstring read_string(HKEY key, const wchar_t* name) {
    DWORD byte_count = 0;
    DWORD type = 0;
    const LSTATUS sized = RegGetValueW(
        key,
        nullptr,
        name,
        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
        &type,
        nullptr,
        &byte_count);
    if (sized != ERROR_SUCCESS || byte_count < sizeof(wchar_t)) {
        return {};
    }
    std::vector<wchar_t> buffer(
        static_cast<std::size_t>(byte_count / sizeof(wchar_t)) + 1U,
        L'\0');
    DWORD requested = byte_count;
    const LSTATUS read = RegGetValueW(
        key,
        nullptr,
        name,
        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
        &type,
        buffer.data(),
        &requested);
    if (read != ERROR_SUCCESS) {
        return {};
    }
    const std::size_t length =
        requested >= sizeof(wchar_t)
            ? static_cast<std::size_t>(
                  requested / sizeof(wchar_t) - 1U)
            : 0U;
    return std::wstring(buffer.data(), length);
}

bool read_dword(HKEY key, const wchar_t* name, DWORD& value) {
    DWORD size = sizeof(value);
    DWORD type = 0;
    return RegGetValueW(
               key,
               nullptr,
               name,
               RRF_RT_REG_DWORD,
               &type,
               &value,
               &size) == ERROR_SUCCESS;
}

std::string icon_process_hint(const std::wstring& display_icon) {
    if (display_icon.empty()) {
        return {};
    }
    std::wstring value = display_icon;
    const std::wstring lower = [&value] {
        std::wstring copy = value;
        std::transform(
            copy.begin(), copy.end(), copy.begin(), [](wchar_t character) {
                return static_cast<wchar_t>(std::towlower(character));
            });
        return copy;
    }();
    const std::size_t executable_end = lower.find(L".exe");
    if (executable_end != std::wstring::npos) {
        value.resize(executable_end + 4U);
    }
    const std::size_t separator = value.find_last_of(L"\\/");
    if (separator != std::wstring::npos) {
        value.erase(0, separator + 1U);
    }
    if (!value.empty() && value.front() == L'"') {
        value.erase(value.begin());
    }
    if (!value.empty() && value.back() == L'"') {
        value.pop_back();
    }
    const std::size_t extension = value.find_last_of(L'.');
    if (extension != std::wstring::npos) {
        value.resize(extension);
    }
    return components::normalized_name(utf8(value));
}

struct RegistrySource {
    HKEY hive;
    REGSAM view;
};

bool enumerate_source(
    const RegistrySource& source,
    const std::size_t maximum_items,
    std::unordered_set<std::string>& seen,
    std::vector<InstalledApplicationRecord>& records,
    bool& complete) {
    HKEY uninstall = nullptr;
    const LSTATUS opened = RegOpenKeyExW(
        source.hive,
        uninstall_key,
        0,
        KEY_READ | source.view,
        &uninstall);
    if (opened != ERROR_SUCCESS) {
        return false;
    }

    DWORD index = 0;
    while (records.size() < maximum_items) {
        std::array<wchar_t, 512> subkey_name{};
        DWORD length = static_cast<DWORD>(subkey_name.size());
        const LSTATUS next = RegEnumKeyExW(
            uninstall,
            index++,
            subkey_name.data(),
            &length,
            nullptr,
            nullptr,
            nullptr,
            nullptr);
        if (next == ERROR_NO_MORE_ITEMS) {
            break;
        }
        if (next != ERROR_SUCCESS) {
            complete = false;
            continue;
        }

        HKEY item = nullptr;
        if (RegOpenKeyExW(
                uninstall,
                std::wstring(
                    subkey_name.data(),
                    static_cast<std::size_t>(length))
                    .c_str(),
                0,
                KEY_READ | source.view,
                &item) != ERROR_SUCCESS) {
            complete = false;
            continue;
        }

        DWORD system_component = 0;
        const bool hidden =
            read_dword(item, L"SystemComponent", system_component) &&
            system_component != 0U;
        const std::wstring display_name = read_string(item, L"DisplayName");
        if (hidden || display_name.empty()) {
            RegCloseKey(item);
            continue;
        }
        const std::wstring version = read_string(item, L"DisplayVersion");
        const std::wstring publisher = read_string(item, L"Publisher");
        const std::wstring display_icon = read_string(item, L"DisplayIcon");
        RegCloseKey(item);

        const std::string display_utf8 = utf8(display_name);
        const std::string version_utf8 = utf8(version);
        const std::string publisher_utf8 = utf8(publisher);
        const std::string identity =
            display_utf8 + '\n' + publisher_utf8 + '\n' + version_utf8;
        const std::string application_id =
            components::opaque_id('a', identity);
        if (!seen.insert(application_id).second) {
            continue;
        }

        std::vector<std::string> hints;
        const std::string name_hint =
            components::normalized_name(display_utf8);
        const std::string icon_hint = icon_process_hint(display_icon);
        if (!name_hint.empty()) {
            hints.push_back(name_hint);
        }
        if (!icon_hint.empty() && icon_hint != name_hint) {
            hints.push_back(icon_hint);
        }
        records.push_back(InstalledApplicationRecord{
            application_id,
            display_utf8,
            version_utf8,
            publisher_utf8,
            {"registry-uninstall"},
            std::move(hints),
            ApplicationLaunchProvider::none,
            {},
        });
    }
    if (records.size() >= maximum_items) {
        complete = false;
    }
    RegCloseKey(uninstall);
    return true;
}

}  // namespace

InstalledApplicationInventory
InstalledApplicationBackend::enumerate_applications(
    const std::size_t maximum_items) const {
    std::vector<InstalledApplicationRecord> records;
    records.reserve(maximum_items);
    std::unordered_set<std::string> seen;
    bool source_available = false;
    bool complete = true;
    const std::array<RegistrySource, 4> sources{{
        {HKEY_CURRENT_USER, KEY_WOW64_64KEY},
        {HKEY_CURRENT_USER, KEY_WOW64_32KEY},
        {HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY},
        {HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY},
    }};
    for (const auto& source : sources) {
        if (records.size() >= maximum_items) {
            complete = false;
            break;
        }
        source_available =
            enumerate_source(
                source, maximum_items, seen, records, complete) ||
            source_available;
    }
    std::sort(
        records.begin(),
        records.end(),
        [](const auto& left, const auto& right) {
            return left.display_name < right.display_name;
        });
    return InstalledApplicationInventory{
        std::move(records),
        source_available,
        complete,
    };
}

}  // namespace act::platform::windows
