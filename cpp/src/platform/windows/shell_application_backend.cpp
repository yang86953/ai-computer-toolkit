#include "platform/windows/shell_application_backend.hpp"

#include "components/opaque_id.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <knownfolders.h>
#include <propsys.h>
#include <propkey.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <shlguid.h>

#include <algorithm>
#include <string>

namespace act::platform::windows {

ShellApplicationInventory
ShellApplicationBackend::enumerate_applications(
    const std::size_t maximum_items) const {
    std::vector<InstalledApplicationRecord> records;
    records.reserve(maximum_items);
    const HRESULT initialized =
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const bool should_uninitialize = SUCCEEDED(initialized);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        return ShellApplicationInventory{std::move(records), false, false};
    }

    IShellItem* apps_item = nullptr;
    HRESULT status = SHGetKnownFolderItem(
        FOLDERID_AppsFolder,
        KF_FLAG_DEFAULT,
        nullptr,
        IID_PPV_ARGS(&apps_item));
    if (FAILED(status) || apps_item == nullptr) {
        if (should_uninitialize) {
            CoUninitialize();
        }
        return ShellApplicationInventory{std::move(records), false, false};
    }

    IShellFolder* folder = nullptr;
    status = apps_item->BindToHandler(
        nullptr, BHID_SFObject, IID_PPV_ARGS(&folder));
    apps_item->Release();
    if (FAILED(status) || folder == nullptr) {
        if (should_uninitialize) {
            CoUninitialize();
        }
        return ShellApplicationInventory{std::move(records), false, false};
    }

    IEnumIDList* enumerator = nullptr;
    status = folder->EnumObjects(
        nullptr,
        SHCONTF_FOLDERS | SHCONTF_NONFOLDERS,
        &enumerator);
    if (FAILED(status) || enumerator == nullptr) {
        folder->Release();
        if (should_uninitialize) {
            CoUninitialize();
        }
        return ShellApplicationInventory{std::move(records), false, false};
    }

    bool complete = true;
    while (records.size() < maximum_items) {
        PITEMID_CHILD child = nullptr;
        ULONG fetched = 0;
        status = enumerator->Next(1, &child, &fetched);
        if (status == S_FALSE || fetched == 0U) {
            break;
        }
        if (FAILED(status) || child == nullptr) {
            complete = false;
            break;
        }

        IShellItem2* item = nullptr;
        status = SHCreateItemWithParent(
            nullptr,
            folder,
            child,
            IID_PPV_ARGS(&item));
        CoTaskMemFree(child);
        if (FAILED(status) || item == nullptr) {
            complete = false;
            continue;
        }

        PWSTR display_name = nullptr;
        PWSTR application_model_id = nullptr;
        PWSTR parsing_name = nullptr;
        const HRESULT display_status =
            item->GetDisplayName(SIGDN_NORMALDISPLAY, &display_name);
        const HRESULT id_status =
            item->GetString(
                PKEY_AppUserModel_ID, &application_model_id);
        const HRESULT parsing_status = item->GetDisplayName(
            SIGDN_DESKTOPABSOLUTEPARSING, &parsing_name);
        item->Release();
        if (FAILED(display_status) || display_name == nullptr) {
            CoTaskMemFree(application_model_id);
            CoTaskMemFree(parsing_name);
            continue;
        }

        const std::string display = utf8(display_name);
        const std::string model_id =
            SUCCEEDED(id_status) && application_model_id != nullptr
                ? utf8(application_model_id)
                : std::string{};
        const std::string launch_identity =
            SUCCEEDED(parsing_status) && parsing_name != nullptr
                ? utf8(parsing_name)
                : std::string{};
        CoTaskMemFree(display_name);
        CoTaskMemFree(application_model_id);
        CoTaskMemFree(parsing_name);
        if (display.empty()) {
            continue;
        }

        std::vector<std::string> hints;
        const std::string normalized =
            components::normalized_name(display);
        if (!normalized.empty()) {
            hints.push_back(normalized);
        }
        records.push_back(InstalledApplicationRecord{
            components::opaque_id(
                'a', "shell\n" + display + '\n' + model_id),
            display,
            {},
            {},
            {"shell-apps-folder"},
            std::move(hints),
            launch_identity.empty()
                ? ApplicationLaunchProvider::none
                : ApplicationLaunchProvider::shell_item,
            launch_identity,
        });
    }
    if (records.size() >= maximum_items) {
        complete = false;
    }
    enumerator->Release();
    folder->Release();
    if (should_uninitialize) {
        CoUninitialize();
    }
    std::sort(
        records.begin(),
        records.end(),
        [](const auto& left, const auto& right) {
            return left.display_name < right.display_name;
        });
    return ShellApplicationInventory{
        std::move(records),
        true,
        complete,
    };
}

}  // namespace act::platform::windows
