#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace act::platform::windows {

enum class ProcessMetadataAccess {
    available,
    permission_blocked,
    unavailable,
};

enum class IntegrityRelation {
    lower,
    same,
    higher,
    unknown,
};

struct ProcessRecord {
    std::string session_id;
    std::string process_name;
    bool identity_reliable;
    ProcessMetadataAccess metadata_access;
    IntegrityRelation integrity_relation;
    std::vector<std::string> window_session_ids;

    // Private platform identity; never serialized.
    std::uint32_t native_process_id;
    // 私有进程创建 FILETIME，仅用于抵抗 PID 复用。
    std::uint64_t native_creation_time;
};

struct ProcessInventory {
    std::vector<ProcessRecord> records;
    bool complete;
};

class ProcessBackend final {
public:
    [[nodiscard]] ProcessInventory enumerate_processes(
        std::size_t maximum_items) const;
    [[nodiscard]] std::string host_session_id() const;
};

}  // namespace act::platform::windows
