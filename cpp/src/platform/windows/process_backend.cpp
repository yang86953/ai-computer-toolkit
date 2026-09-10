#include "platform/windows/process_backend.hpp"

#include "components/opaque_id.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>
#include <tlhelp32.h>

#include <array>
#include <cstddef>
#include <optional>
#include <sstream>
#include <vector>

namespace act::platform::windows {
namespace {

std::optional<DWORD> integrity_rid(HANDLE process) {
    HANDLE token = nullptr;
    if (OpenProcessToken(process, TOKEN_QUERY, &token) == FALSE) {
        return std::nullopt;
    }
    DWORD required = 0;
    (void)GetTokenInformation(
        token, TokenIntegrityLevel, nullptr, 0, &required);
    if (required == 0U) {
        CloseHandle(token);
        return std::nullopt;
    }
    std::vector<std::byte> buffer(required);
    if (GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.data(),
            required,
            &required) == FALSE) {
        CloseHandle(token);
        return std::nullopt;
    }
    CloseHandle(token);
    const auto* label =
        reinterpret_cast<const TOKEN_MANDATORY_LABEL*>(buffer.data());
    const UCHAR count = *GetSidSubAuthorityCount(label->Label.Sid);
    if (count == 0U) {
        return std::nullopt;
    }
    return *GetSidSubAuthority(label->Label.Sid, count - 1U);
}

struct ProcessObservation {
    std::uint64_t creation_time;
    bool identity_reliable;
    ProcessMetadataAccess metadata_access;
    IntegrityRelation integrity_relation;
};

ProcessObservation observe_process(
    const DWORD process_id,
    const std::optional<DWORD> current_integrity) {
    HANDLE process = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (process == nullptr) {
        return ProcessObservation{
            0,
            false,
            GetLastError() == ERROR_ACCESS_DENIED
                ? ProcessMetadataAccess::permission_blocked
                : ProcessMetadataAccess::unavailable,
            IntegrityRelation::unknown,
        };
    }
    FILETIME created{};
    FILETIME exited{};
    FILETIME kernel{};
    FILETIME user{};
    const BOOL succeeded =
        GetProcessTimes(process, &created, &exited, &kernel, &user);
    const auto target_integrity = integrity_rid(process);
    CloseHandle(process);
    if (succeeded == FALSE) {
        return ProcessObservation{
            0,
            false,
            ProcessMetadataAccess::unavailable,
            IntegrityRelation::unknown,
        };
    }
    IntegrityRelation relation = IntegrityRelation::unknown;
    if (current_integrity.has_value() &&
        target_integrity.has_value()) {
        relation = *target_integrity < *current_integrity
                       ? IntegrityRelation::lower
                       : (*target_integrity > *current_integrity
                              ? IntegrityRelation::higher
                              : IntegrityRelation::same);
    }
    return ProcessObservation{
        (static_cast<std::uint64_t>(created.dwHighDateTime) << 32U) |
            static_cast<std::uint64_t>(created.dwLowDateTime),
        true,
        ProcessMetadataAccess::available,
        relation,
    };
}

}  // namespace

ProcessInventory ProcessBackend::enumerate_processes(
    const std::size_t maximum_items) const {
    std::vector<ProcessRecord> records;
    records.reserve(maximum_items);
    HANDLE snapshot =
        CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return ProcessInventory{std::move(records), false};
    }

    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    const std::optional<DWORD> current_integrity =
        integrity_rid(GetCurrentProcess());
    BOOL has_entry = Process32FirstW(snapshot, &entry);
    while (has_entry != FALSE && records.size() < maximum_items) {
        const std::string name = utf8(entry.szExeFile);
        const ProcessObservation observation =
            observe_process(entry.th32ProcessID, current_integrity);
        std::ostringstream identity;
        identity << static_cast<unsigned long>(entry.th32ProcessID) << ':'
                 << observation.creation_time << ':' << name;
        records.push_back(ProcessRecord{
            components::opaque_id('p', identity.str()),
            name,
            observation.identity_reliable,
            observation.metadata_access,
            observation.integrity_relation,
            {},
            static_cast<std::uint32_t>(entry.th32ProcessID),
            // 保存只参与重新发现身份的进程创建 FILETIME。
            observation.creation_time,
        });
        has_entry = Process32NextW(snapshot, &entry);
    }
    const bool complete = has_entry == FALSE;
    CloseHandle(snapshot);
    return ProcessInventory{std::move(records), complete};
}

std::string ProcessBackend::host_session_id() const {
    DWORD session_id = 0;
    (void)ProcessIdToSessionId(GetCurrentProcessId(), &session_id);
    std::array<wchar_t, 257> user{};
    DWORD user_length = static_cast<DWORD>(user.size());
    std::wstring_view user_view;
    if (GetUserNameW(user.data(), &user_length) != FALSE &&
        user_length > 0U) {
        user_view = std::wstring_view(
            user.data(), static_cast<std::size_t>(user_length - 1U));
    }
    std::ostringstream identity;
    identity << static_cast<unsigned long>(session_id) << ':'
             << utf8(user_view);
    return components::opaque_id('h', identity.str());
}

}  // namespace act::platform::windows
