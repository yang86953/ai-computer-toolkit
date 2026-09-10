#include "components/json.hpp"
#include "components/static_permission_assessment.hpp"
#include "modules/standard_edit_compatibility_module.hpp"
#include "modules/standard_edit_module.hpp"

#include <windows.h>

#include <array>
#include <future>
#include <iostream>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

namespace {

bool static_assessment_branches() {
    using Access =
        act::components::StaticMetadataAccess;
    using Integrity =
        act::components::StaticIntegrityRelation;
    const auto same =
        act::components::assess_static_background_mutation(
            Access::available, Integrity::same);
    const auto higher =
        act::components::assess_static_background_mutation(
            Access::available, Integrity::higher);
    const auto denied =
        act::components::assess_static_background_mutation(
            Access::permission_blocked, Integrity::unknown);
    const auto unknown =
        act::components::assess_static_background_mutation(
            Access::unavailable, Integrity::unknown);
    return std::string_view(same.decision) ==
               "requires-confirmation" &&
           std::string_view(higher.decision) ==
               "permission-blocked" &&
           std::string_view(denied.decision) ==
               "permission-blocked" &&
           std::string_view(unknown.decision) ==
               "indeterminate" &&
           !same.safe_to_execute_now &&
           same.requires_confirmation &&
           !same.foreground_required &&
           !same.active_write_probe_performed;
}

constexpr UINT fixture_pause_message = WM_APP + 41U;
HANDLE pause_entered = nullptr;
HANDLE pause_release = nullptr;

LRESULT CALLBACK fixture_window_proc(
    const HWND window,
    const UINT message,
    const WPARAM word,
    const LPARAM value) {
    if (message == WM_CLOSE) {
        DestroyWindow(window);
        return 0;
    }
    if (message == fixture_pause_message) {
        SetEvent(pause_entered);
        WaitForSingleObject(pause_release, INFINITE);
        return 0;
    }
    if (message == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, word, value);
}

std::pair<HWND, HWND> create_fixture() {
    constexpr wchar_t class_name[] =
        L"ActSelfOwnedStandardEditFixture";
    WNDCLASSW window_class{};
    window_class.lpfnWndProc = fixture_window_proc;
    window_class.hInstance = GetModuleHandleW(nullptr);
    window_class.lpszClassName = class_name;
    if (RegisterClassW(&window_class) == 0U &&
        GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        return {nullptr, nullptr};
    }
    const HWND parent = CreateWindowExW(
        WS_EX_NOACTIVATE,
        class_name,
        L"ACT_SELF_OWNED_STANDARD_EDIT_FIXTURE",
        WS_OVERLAPPED,
        0,
        0,
        320,
        120,
        nullptr,
        nullptr,
        window_class.hInstance,
        nullptr);
    if (parent == nullptr) {
        return {nullptr, nullptr};
    }
    const HWND edit = CreateWindowExW(
        0U,
        L"EDIT",
        L"initial",
        WS_CHILD | ES_LEFT,
        8,
        8,
        280,
        30,
        parent,
        nullptr,
        window_class.hInstance,
        nullptr);
    if (edit == nullptr) {
        DestroyWindow(parent);
        return {nullptr, nullptr};
    }
    return {parent, edit};
}

std::wstring read_fixture_text(const HWND edit) {
    std::array<wchar_t, 1024> buffer{};
    DWORD_PTR copied = 0U;
    if (SendMessageTimeoutW(
            edit,
            WM_GETTEXT,
            static_cast<WPARAM>(buffer.size()),
            reinterpret_cast<LPARAM>(buffer.data()),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            2000U,
            &copied) == 0) {
        return {};
    }
    return std::wstring(
        buffer.data(), static_cast<std::size_t>(copied));
}

bool json_false(
    const act::components::Json& value,
    const char* key) {
    const auto* field = value.find(key);
    return field != nullptr &&
           field->bool_value() != nullptr &&
           !*field->bool_value();
}

}  // namespace

int main() {
    std::promise<std::pair<HWND, HWND>> ready;
    auto future = ready.get_future();
    std::thread fixture([&ready] {
        const auto windows = create_fixture();
        ready.set_value(windows);
        if (windows.first == nullptr) {
            return;
        }
        MSG message{};
        while (GetMessageW(
                   &message, nullptr, 0U, 0U) > 0) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    });
    const auto [parent, edit] = future.get();
    if (parent == nullptr || edit == nullptr) {
        if (fixture.joinable()) {
            fixture.join();
        }
        return 2;
    }

    act::modules::StandardEditModule module;
    const auto discovered = module.sessions(4096U);
    std::string session_id;
    if (discovered.ok) {
        const auto* sessions_value =
            discovered.data.find("sessions");
        const auto* sessions =
            sessions_value == nullptr
                ? nullptr
                : sessions_value->array_items();
        if (sessions != nullptr) {
            for (const auto& session : *sessions) {
                const auto* application =
                    session.find("applicationName");
                const auto* id = session.find("sessionId");
                if (application != nullptr &&
                    application->string_value() != nullptr &&
                    *application->string_value() ==
                        "act-standard-edit-module-test.exe" &&
                    id != nullptr &&
                    id->string_value() != nullptr) {
                    session_id = *id->string_value();
                    break;
                }
            }
        }
    }

    const HWND foreground_before = GetForegroundWindow();
    const auto unconfirmed = module.set_text(
        "s2:c:0000000000000000",
        "unconfirmed",
        false,
        2000U);
    const bool confirmation_first =
        !unconfirmed.ok &&
        unconfirmed.error_code ==
            "CONFIRMATION_REQUIRED" &&
        read_fixture_text(edit) == L"initial";

    const auto stale = module.set_text(
        "s2:c:0000000000000000",
        "stale",
        true,
        2000U);
    const bool stale_refused =
        !stale.ok &&
        stale.error_code == "STALE_SESSION" &&
        read_fixture_text(edit) == L"initial";

    const auto invalid_timeout = module.set_text(
        session_id, "invalid", true, 0U);
    const bool timeout_bounded =
        !invalid_timeout.ok &&
        invalid_timeout.error_code ==
            "INVALID_ARGUMENT" &&
        read_fixture_text(edit) == L"initial";

    const std::string replacement =
        "fixture-\xE6\x9B\xB4\xE6\x96\xB0-42";
    const auto changed = module.set_text(
        session_id, replacement, true, 2000U);
    const bool exact_write =
        changed.ok &&
        read_fixture_text(edit) ==
            L"fixture-\u66F4\u65B0-42";
    const bool verified =
        changed.ok &&
        changed.data.find("verifiedByReadback") != nullptr &&
        changed.data.find("verifiedByReadback")
                ->bool_value() != nullptr &&
        *changed.data.find("verifiedByReadback")
             ->bool_value();
    const auto* capability =
        changed.data.find("capability");
    const bool capability_mapped =
        capability != nullptr &&
        capability->string_value() != nullptr &&
        *capability->string_value() ==
            "ui.text.input@1";
    const bool foreground_unchanged =
        foreground_before == GetForegroundWindow();
    const bool native_hidden =
        changed.ok &&
        json_false(
            changed.data, "nativeIdentifiersExposed") &&
        changed.data.dump().find("hwnd") ==
            std::string::npos &&
        changed.data.dump().find("processId") ==
            std::string::npos;

    pause_entered = CreateEventW(
        nullptr, TRUE, FALSE, nullptr);
    pause_release = CreateEventW(
        nullptr, TRUE, FALSE, nullptr);
    PostMessageW(
        parent, fixture_pause_message, 0U, 0);
    const bool fixture_blocked =
        WaitForSingleObject(
            pause_entered, 2000U) == WAIT_OBJECT_0;
    const auto timed_out = module.set_text(
        session_id,
        "timeout-outcome",
        true,
        20U);
    const auto* timeout_details =
        timed_out.error_details.has_value()
            ? &*timed_out.error_details
            : nullptr;
    const bool timeout_outcome_unknown =
        fixture_blocked &&
        !timed_out.ok &&
        timed_out.error_code == "TIMEOUT" &&
        timeout_details != nullptr &&
        timeout_details->find("outcome") != nullptr &&
        timeout_details->find("outcome")
                ->string_value() != nullptr &&
        *timeout_details->find("outcome")
             ->string_value() == "unknown" &&
        json_false(*timeout_details, "retrySafe") &&
        timeout_details->find(
            "targetMayHaveMutated") != nullptr &&
        timeout_details->find(
            "targetMayHaveMutated")->bool_value() !=
            nullptr &&
        *timeout_details->find(
            "targetMayHaveMutated")->bool_value();
    SetEvent(pause_release);
    Sleep(100U);
    const bool late_mutation_observed =
        read_fixture_text(edit) ==
        L"timeout-outcome";
    act::modules::StandardEditCompatibilityModule
        compatibility;
    const auto mapped_success =
        compatibility.map_run_result(changed);
    const auto mapped_timeout =
        compatibility.map_run_result(timed_out);
    const auto mapped_app =
        compatibility.map_app_result(changed, session_id);
    const auto mapped_desktop =
        compatibility.map_desktop_type_text_result(changed);
    const bool success_mapped_safely =
        mapped_success.ok &&
        mapped_success.data.find("app") != nullptr &&
        mapped_success.data.find("sessionId") != nullptr &&
        mapped_success.data.dump().find("hwnd") ==
            std::string::npos &&
        mapped_success.data.dump().find("processId") ==
            std::string::npos;
    const bool timeout_mapped_safely =
        !mapped_timeout.ok &&
        mapped_timeout.error_code ==
            "TARGET_HUNG_OR_UNAVAILABLE" &&
        mapped_timeout.error_details.has_value() &&
        json_false(
            *mapped_timeout.error_details, "retrySafe");
    const bool app_mapped_safely =
        mapped_app.ok &&
        mapped_app.data.find("capability") != nullptr &&
        mapped_app.data.find("targetId") != nullptr &&
        mapped_app.data.find("compatibilityShape") != nullptr &&
        mapped_app.data.dump().find("hwnd") ==
            std::string::npos &&
        mapped_app.data.dump().find("processId") ==
            std::string::npos;
    const bool desktop_mapped_safely =
        mapped_desktop.ok &&
        mapped_desktop.data.find("operation") != nullptr &&
        mapped_desktop.data.find("executionMode") != nullptr &&
        mapped_desktop.data.find("compatibilityShape") != nullptr &&
        mapped_desktop.data.dump().find("hwnd") ==
            std::string::npos &&
        mapped_desktop.data.dump().find("processId") ==
            std::string::npos;

    PostMessageW(parent, WM_CLOSE, 0U, 0);
    fixture.join();
    CloseHandle(pause_entered);
    CloseHandle(pause_release);

    const bool ok =
        discovered.ok &&
        !session_id.empty() &&
        confirmation_first &&
        stale_refused &&
        timeout_bounded &&
        exact_write &&
        verified &&
        capability_mapped &&
        foreground_unchanged &&
        native_hidden &&
        timeout_outcome_unknown &&
        success_mapped_safely &&
        timeout_mapped_safely &&
        app_mapped_safely &&
        desktop_mapped_safely &&
        static_assessment_branches();
    std::cout << act::components::object({
        {"ok", ok},
        {"selfOwnedFixtureOnly", true},
        {"confirmationFirst", confirmation_first},
        {"staleTargetRefused", stale_refused},
        {"timeoutBounded", timeout_bounded},
        {"exactOpaqueTarget", !session_id.empty()},
        {"readbackVerified", verified},
        {"capabilityMapped", capability_mapped},
        {"foregroundUnchanged", foreground_unchanged},
        {"nativeIdentifierLeak", !native_hidden},
        {"timeoutOutcomeUnknown", timeout_outcome_unknown},
        {"retrySafe", false},
        {"lateMutationObserved", late_mutation_observed},
        {"successMappedSafely", success_mapped_safely},
        {"timeoutMappedSafely", timeout_mapped_safely},
        {"appMappedSafely", app_mapped_safely},
        {"desktopTypeTextMappedSafely",
         desktop_mapped_safely},
        {"staticAssessmentBranches",
         static_assessment_branches()},
        {"publicRunOpened", true},
        {"userApplicationsWritten", 0},
    }).dump() << '\n';
    return ok ? 0 : 2;
}
