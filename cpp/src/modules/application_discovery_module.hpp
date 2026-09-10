#pragma once

// 暴露无平台类型的 opaque 唯一匹配状态。
#include "components/opaque_target_match.hpp"
#include "modules/module_result.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/installed_application_backend.hpp"
#include "platform/windows/process_backend.hpp"
#include "platform/windows/shell_application_backend.hpp"

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

namespace act::modules {

enum class TargetKind {
    host,
    installed_application,
    running_process,
    application_window,
    standard_edit_control,
};

// 保存跨目标种类解析后的唯一匹配状态。
struct TargetKindResolution {
    // 保存 missing、unique 或 ambiguous 状态。
    components::OpaqueTargetMatchState state;
    // 仅在唯一命中时保存目标种类。
    std::optional<TargetKind> target_kind;
    // 仅为唯一运行进程命中保存当前快照内的记录地址。
    const platform::windows::ProcessRecord* process;
// 结束跨目标种类解析结果定义。
};

struct ApplicationInventorySnapshot {
    std::string host_session_id;
    std::vector<platform::windows::InstalledApplicationRecord> applications;
    std::vector<platform::windows::ProcessRecord> processes;
    std::vector<platform::windows::WindowRecord> windows;
    bool registry_source_available;
    bool shell_source_available;
    bool applications_complete;
    bool processes_complete;
    bool foreground_unchanged;
};

class ApplicationDiscoveryModule final {
public:
    [[nodiscard]] ApplicationInventorySnapshot capture(
        std::size_t maximum_applications,
        std::size_t maximum_processes,
        std::size_t maximum_windows) const;
    [[nodiscard]] ModuleResult discover(
        std::size_t maximum_applications,
        std::size_t maximum_processes,
        std::size_t maximum_windows) const;
    [[nodiscard]] ModuleResult process_status() const;
    [[nodiscard]] ModuleResult process_sessions(
        std::size_t maximum_items) const;
    [[nodiscard]] ModuleResult inspect_process(
        const std::string& session_id) const;
    // 跨所有当前清单解析目标并拒绝重复身份。
    [[nodiscard]] TargetKindResolution resolve_target(
        const ApplicationInventorySnapshot& snapshot,
        const std::string& target_id) const;
private:
    platform::windows::InstalledApplicationBackend installed_backend_;
    platform::windows::ProcessBackend process_backend_;
    platform::windows::ShellApplicationBackend shell_backend_;
    platform::windows::DiscoveryBackend window_backend_;
};

}  // namespace act::modules
