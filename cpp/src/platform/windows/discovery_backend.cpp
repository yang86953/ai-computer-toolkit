#include "platform/windows/discovery_backend.hpp"

#include "components/opaque_id.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <UIAutomation.h>
#include <oleauto.h>

#include <algorithm>
#include <array>
#include <deque>
#include <sstream>
#include <string_view>

namespace act::platform::windows {
namespace {

std::string bstr_utf8(BSTR value) {
    if (value == nullptr) {
        return {};
    }
    return utf8(
        std::wstring_view(value, static_cast<std::size_t>(SysStringLen(value))));
}

std::string process_name_and_creation(
    const DWORD process_id,
    std::uint64_t& creation_time) {
    creation_time = 0;
    HANDLE process = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (process == nullptr) {
        return "unavailable";
    }

    FILETIME created{};
    FILETIME exited{};
    FILETIME kernel{};
    FILETIME user{};
    if (GetProcessTimes(process, &created, &exited, &kernel, &user) != FALSE) {
        creation_time =
            (static_cast<std::uint64_t>(created.dwHighDateTime) << 32U) |
            static_cast<std::uint64_t>(created.dwLowDateTime);
    }

    std::array<wchar_t, 32768> path{};
    DWORD path_length = static_cast<DWORD>(path.size());
    std::string name = "unavailable";
    if (QueryFullProcessImageNameW(
            process, 0, path.data(), &path_length) != FALSE) {
        std::wstring_view full_path(path.data(), path_length);
        const std::size_t separator = full_path.find_last_of(L"\\/");
        if (separator != std::wstring_view::npos) {
            full_path.remove_prefix(separator + 1U);
        }
        name = utf8(full_path);
    }
    CloseHandle(process);
    return name;
}

std::string session_id(
    const HWND window,
    const DWORD process_id,
    const std::uint64_t creation_time) {
    std::ostringstream identity;
    identity << static_cast<unsigned long>(process_id) << ':'
             << reinterpret_cast<std::uintptr_t>(window) << ':'
             << creation_time;
    return components::opaque_id('w', identity.str());
}

std::string window_title(const HWND window) {
    const int length = GetWindowTextLengthW(window);
    if (length <= 0) {
        return {};
    }
    std::wstring value(static_cast<std::size_t>(length) + 1U, L'\0');
    const int copied =
        GetWindowTextW(window, value.data(), static_cast<int>(value.size()));
    if (copied <= 0) {
        return {};
    }
    value.resize(static_cast<std::size_t>(copied));
    return utf8(value);
}

struct EnumerationContext {
    std::size_t maximum_items;
    std::vector<WindowRecord>* records;
};

BOOL CALLBACK collect_window(const HWND window, const LPARAM data) {
    auto* context = reinterpret_cast<EnumerationContext*>(data);
    if (context->records->size() >= context->maximum_items) {
        return FALSE;
    }
    if (IsWindowVisible(window) == FALSE) {
        return TRUE;
    }
    const std::string title = window_title(window);
    if (title.empty()) {
        return TRUE;
    }

    DWORD process_id = 0;
    GetWindowThreadProcessId(window, &process_id);
    std::uint64_t creation_time = 0;
    std::string application_name =
        process_name_and_creation(process_id, creation_time);

    context->records->push_back(WindowRecord{
        session_id(window, process_id, creation_time),
        std::move(application_name),
        title,
        true,
        reinterpret_cast<std::uintptr_t>(window),
        static_cast<std::uint32_t>(process_id),
    });
    return TRUE;
}

BackendError accessibility_error(
    const char* operation,
    const HRESULT status) {
    std::ostringstream message;
    message << operation << " failed (HRESULT 0x" << std::hex
            << static_cast<unsigned long>(status) << ')';
    const char* code =
        status == E_ACCESSDENIED
            ? "PERMISSION_DENIED"
            : (status ==
                       static_cast<HRESULT>(
                           UIA_E_ELEMENTNOTAVAILABLE)
                   ? "STALE_SESSION"
                   : "ACCESSIBILITY_UNAVAILABLE");
    return BackendError{code, message.str()};
}

struct ComContext {
    IUIAutomation* automation;
    bool should_uninitialize;
};

std::optional<BackendError> create_automation(ComContext& context) {
    const HRESULT initialized =
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    context.should_uninitialize = SUCCEEDED(initialized);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) {
        return accessibility_error("CoInitializeEx", initialized);
    }
    const HRESULT status = CoCreateInstance(
        CLSID_CUIAutomation,
        nullptr,
        CLSCTX_INPROC_SERVER,
        IID_IUIAutomation,
        reinterpret_cast<void**>(&context.automation));
    if (FAILED(status)) {
        if (context.should_uninitialize) {
            CoUninitialize();
            context.should_uninitialize = false;
        }
        return accessibility_error(
            "CoCreateInstance(CUIAutomation)", status);
    }
    return std::nullopt;
}

void release_automation(ComContext& context) {
    if (context.automation != nullptr) {
        context.automation->Release();
        context.automation = nullptr;
    }
    if (context.should_uninitialize) {
        CoUninitialize();
        context.should_uninitialize = false;
    }
}

AccessibilityNode read_node(
    IUIAutomationElement* element,
    const std::size_t depth,
    const std::string& node_identity) {
    BSTR name = nullptr;
    BSTR automation_id = nullptr;
    BSTR class_name = nullptr;
    BSTR framework_id = nullptr;
    CONTROLTYPEID control_type = 0;
    BOOL enabled = FALSE;
    BOOL offscreen = FALSE;
    const bool complete =
        SUCCEEDED(element->get_CurrentName(&name)) &&
        SUCCEEDED(element->get_CurrentAutomationId(&automation_id)) &&
        SUCCEEDED(element->get_CurrentClassName(&class_name)) &&
        SUCCEEDED(element->get_CurrentFrameworkId(&framework_id)) &&
        SUCCEEDED(element->get_CurrentControlType(&control_type)) &&
        SUCCEEDED(element->get_CurrentIsEnabled(&enabled)) &&
        SUCCEEDED(element->get_CurrentIsOffscreen(&offscreen));
    AccessibilityNode node{
        components::opaque_id('e', node_identity),
        depth,
        bstr_utf8(name),
        bstr_utf8(automation_id),
        bstr_utf8(class_name),
        bstr_utf8(framework_id),
        static_cast<int>(control_type),
        enabled != FALSE,
        offscreen != FALSE,
        complete,
    };
    SysFreeString(name);
    SysFreeString(automation_id);
    SysFreeString(class_name);
    SysFreeString(framework_id);
    return node;
}

}  // namespace

std::vector<WindowRecord> DiscoveryBackend::enumerate_windows(
    const std::size_t maximum_items) const {
    std::vector<WindowRecord> records;
    records.reserve(std::min<std::size_t>(maximum_items, 64U));
    EnumerationContext context{maximum_items, &records};
    EnumWindows(collect_window, reinterpret_cast<LPARAM>(&context));
    return records;
}

AccessibilityResult DiscoveryBackend::read_accessibility_root(
    const WindowRecord& window) const {
    ComContext context{nullptr, false};
    const auto initialization_error = create_automation(context);
    if (initialization_error.has_value()) {
        return AccessibilityResult{
            std::nullopt,
            initialization_error,
        };
    }

    IUIAutomationElement* element = nullptr;
    const HRESULT status = context.automation->ElementFromHandle(
        reinterpret_cast<HWND>(window.native_window), &element);
    if (FAILED(status) || element == nullptr) {
        release_automation(context);
        return AccessibilityResult{
            std::nullopt,
            accessibility_error("IUIAutomation::ElementFromHandle", status),
        };
    }

    BSTR name = nullptr;
    BSTR automation_id = nullptr;
    BSTR class_name = nullptr;
    BSTR framework_id = nullptr;
    CONTROLTYPEID control_type = 0;
    BOOL enabled = FALSE;
    BOOL offscreen = FALSE;

    const HRESULT name_status = element->get_CurrentName(&name);
    const HRESULT automation_id_status =
        element->get_CurrentAutomationId(&automation_id);
    const HRESULT class_status = element->get_CurrentClassName(&class_name);
    const HRESULT framework_status =
        element->get_CurrentFrameworkId(&framework_id);
    const HRESULT control_type_status =
        element->get_CurrentControlType(&control_type);
    const HRESULT enabled_status = element->get_CurrentIsEnabled(&enabled);
    const HRESULT offscreen_status =
        element->get_CurrentIsOffscreen(&offscreen);

    const bool property_failure =
        FAILED(name_status) || FAILED(automation_id_status) ||
        FAILED(class_status) || FAILED(framework_status) ||
        FAILED(control_type_status) || FAILED(enabled_status) ||
        FAILED(offscreen_status);

    AccessibilityRoot root{
        bstr_utf8(name),
        bstr_utf8(automation_id),
        bstr_utf8(class_name),
        bstr_utf8(framework_id),
        static_cast<int>(control_type),
        enabled != FALSE,
        offscreen != FALSE,
    };

    SysFreeString(name);
    SysFreeString(automation_id);
    SysFreeString(class_name);
    SysFreeString(framework_id);
    element->Release();
    release_automation(context);

    if (property_failure) {
        return AccessibilityResult{
            std::nullopt,
            BackendError{
                "ACCESSIBILITY_UNAVAILABLE",
                "One or more read-only UI Automation root properties could "
                "not be read.",
            },
        };
    }
    return AccessibilityResult{std::move(root), std::nullopt};
}

AccessibilityTreeResult DiscoveryBackend::read_accessibility_tree(
    const WindowRecord& window,
    const std::size_t maximum_depth,
    const std::size_t maximum_items,
    const std::string& view) const {
    ComContext context{nullptr, false};
    const auto initialization_error = create_automation(context);
    if (initialization_error.has_value()) {
        return AccessibilityTreeResult{
            std::nullopt,
            initialization_error,
        };
    }

    IUIAutomationElement* root = nullptr;
    HRESULT status = context.automation->ElementFromHandle(
        reinterpret_cast<HWND>(window.native_window), &root);
    if (FAILED(status) || root == nullptr) {
        release_automation(context);
        return AccessibilityTreeResult{
            std::nullopt,
            accessibility_error("IUIAutomation::ElementFromHandle", status),
        };
    }

    IUIAutomationTreeWalker* walker = nullptr;
    status = view == "raw"
                 ? context.automation->get_RawViewWalker(&walker)
                 : context.automation->get_ControlViewWalker(&walker);
    if (FAILED(status) || walker == nullptr) {
        root->Release();
        release_automation(context);
        return AccessibilityTreeResult{
            std::nullopt,
            accessibility_error("IUIAutomation::TreeWalker", status),
        };
    }

    struct PendingNode {
        IUIAutomationElement* element;
        std::size_t depth;
        std::string path;
    };
    std::deque<PendingNode> pending;
    pending.push_back(PendingNode{root, 0U, "0"});
    std::vector<AccessibilityNode> nodes;
    nodes.reserve(maximum_items);
    std::size_t visited = 0;
    bool traversal_complete = true;

    while (!pending.empty() && nodes.size() < maximum_items) {
        PendingNode current = std::move(pending.front());
        pending.pop_front();
        ++visited;
        nodes.push_back(read_node(
            current.element,
            current.depth,
            window.session_id + ':' + current.path));

        if (current.depth < maximum_depth) {
            IUIAutomationElement* child = nullptr;
            HRESULT child_status = walker->GetFirstChildElement(
                current.element, &child);
            if (FAILED(child_status)) {
                traversal_complete = false;
            }
            std::size_t child_index = 0;
            while (SUCCEEDED(child_status) && child != nullptr) {
                IUIAutomationElement* next = nullptr;
                const HRESULT next_status =
                    walker->GetNextSiblingElement(child, &next);
                pending.push_back(PendingNode{
                    child,
                    current.depth + 1U,
                    current.path + '.' +
                        std::to_string(child_index++),
                });
                child = next;
                child_status = next_status;
                if (FAILED(child_status)) {
                    traversal_complete = false;
                }
            }
        }
        current.element->Release();
    }

    const bool truncated = !pending.empty() || !traversal_complete;
    for (auto& remaining : pending) {
        remaining.element->Release();
    }
    walker->Release();
    release_automation(context);
    return AccessibilityTreeResult{
        AccessibilityTree{
            std::move(nodes),
            visited,
            truncated,
        },
        std::nullopt,
    };
}

std::string DiscoveryBackend::foreground_token() const {
    const HWND foreground = GetForegroundWindow();
    if (foreground == nullptr) {
        return "none";
    }
    DWORD process_id = 0;
    GetWindowThreadProcessId(foreground, &process_id);
    std::uint64_t creation_time = 0;
    (void)process_name_and_creation(process_id, creation_time);
    return session_id(foreground, process_id, creation_time);
}

}  // namespace act::platform::windows
