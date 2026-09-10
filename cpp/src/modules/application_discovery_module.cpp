#include "modules/application_discovery_module.hpp"

#include "components/opaque_id.hpp"
// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"

#include <algorithm>
#include <unordered_map>
#include <unordered_set>

namespace act::modules {
namespace {

std::string process_stem(const std::string& process_name) {
    const std::size_t extension = process_name.find_last_of('.');
    return components::normalized_name(
        extension == std::string::npos
            ? process_name
            : process_name.substr(0, extension));
}

using RelationMap =
    std::unordered_map<std::string, std::vector<std::string>>;

struct Relations {
    RelationMap application_to_process;
    RelationMap process_to_application;
};

Relations build_relations(
    const ApplicationInventorySnapshot& snapshot) {
    Relations relations;
    std::unordered_map<std::string, std::vector<std::string>>
        processes_by_hint;
    for (const auto& process : snapshot.processes) {
        const std::string hint = process_stem(process.process_name);
        if (!hint.empty()) {
            processes_by_hint[hint].push_back(process.session_id);
        }
    }
    for (const auto& application : snapshot.applications) {
        std::unordered_set<std::string> matched;
        for (const auto& hint : application.process_match_hints) {
            const auto found = processes_by_hint.find(hint);
            if (found == processes_by_hint.end()) {
                continue;
            }
            for (const auto& process_id : found->second) {
                if (!matched.insert(process_id).second) {
                    continue;
                }
                relations.application_to_process[application.session_id]
                    .push_back(process_id);
                relations.process_to_application[process_id].push_back(
                    application.session_id);
            }
        }
    }
    return relations;
}

components::Json string_array(const std::vector<std::string>& values) {
    components::Json::Array result;
    result.reserve(values.size());
    for (const auto& value : values) {
        result.emplace_back(value);
    }
    return components::Json(std::move(result));
}

const char* metadata_access_name(
    const platform::windows::ProcessMetadataAccess access) {
    using Access = platform::windows::ProcessMetadataAccess;
    switch (access) {
        case Access::available:
            return "available";
        case Access::permission_blocked:
            return "permission-blocked";
        case Access::unavailable:
            return "unavailable";
    }
    return "unavailable";
}

const char* integrity_relation_name(
    const platform::windows::IntegrityRelation relation) {
    using Relation = platform::windows::IntegrityRelation;
    switch (relation) {
        case Relation::lower:
            return "lower";
        case Relation::same:
            return "same";
        case Relation::higher:
            return "higher";
        case Relation::unknown:
            return "unknown";
    }
    return "unknown";
}

components::Json process_observation_json(
    const platform::windows::ProcessRecord& process) {
    return components::object({
        {"sessionId", process.session_id},
        {"targetKind", "running-process"},
        {"processName", process.process_name},
        {"state", "running"},
        {"identityFreshness",
         process.identity_reliable
             ? "process-lifetime"
             : "best-effort-current-snapshot"},
        {"metadataAccess",
         metadata_access_name(process.metadata_access)},
        {"integrityRelation",
         integrity_relation_name(process.integrity_relation)},
        {"hasVisibleWindow", !process.window_session_ids.empty()},
        {"windowVisibility",
         process.window_session_ids.empty()
             ? "no-visible-titled-window"
             : "visible-titled-window"},
        {"foregroundRequiredForObservation", false},
        {"windowSessionIds",
         string_array(process.window_session_ids)},
    });
}

platform::windows::ProcessInventory attach_windows(
    platform::windows::ProcessInventory processes,
    const std::vector<platform::windows::WindowRecord>& windows) {
    std::unordered_map<std::uint32_t, std::size_t> by_native_id;
    for (std::size_t index = 0;
         index < processes.records.size();
         ++index) {
        by_native_id.emplace(
            processes.records[index].native_process_id, index);
    }
    for (const auto& window : windows) {
        const auto process =
            by_native_id.find(window.native_process_id);
        if (process != by_native_id.end()) {
            processes.records[process->second]
                .window_session_ids.push_back(window.session_id);
        }
    }
    return processes;
}

}  // namespace

ApplicationInventorySnapshot ApplicationDiscoveryModule::capture(
    const std::size_t maximum_applications,
    const std::size_t maximum_processes,
    const std::size_t maximum_windows) const {
    const std::string foreground_before =
        window_backend_.foreground_token();
    auto applications =
        installed_backend_.enumerate_applications(maximum_applications);
    auto shell_applications =
        shell_backend_.enumerate_applications(maximum_applications);
    bool combined_applications_complete =
        applications.complete && shell_applications.complete;
    for (auto& shell_application : shell_applications.records) {
        const std::string shell_name =
            components::normalized_name(shell_application.display_name);
        std::vector<std::size_t> matches;
        for (std::size_t index = 0;
             index < applications.records.size();
             ++index) {
            if (components::normalized_name(
                    applications.records[index].display_name) ==
                shell_name) {
                matches.push_back(index);
            }
        }
        if (matches.size() == 1U) {
            auto& existing = applications.records[matches[0]];
            for (const auto& source :
                 shell_application.discovery_sources) {
                if (std::find(
                        existing.discovery_sources.begin(),
                        existing.discovery_sources.end(),
                        source) == existing.discovery_sources.end()) {
                    existing.discovery_sources.push_back(source);
                }
            }
            for (const auto& hint :
                 shell_application.process_match_hints) {
                if (std::find(
                        existing.process_match_hints.begin(),
                        existing.process_match_hints.end(),
                        hint) == existing.process_match_hints.end()) {
                    existing.process_match_hints.push_back(hint);
                }
            }
            if (existing.launch_provider ==
                    platform::windows::ApplicationLaunchProvider::none &&
                shell_application.launch_provider ==
                    platform::windows::ApplicationLaunchProvider::
                        shell_item) {
                existing.launch_provider =
                    shell_application.launch_provider;
                existing.launch_identity =
                    std::move(shell_application.launch_identity);
            }
        } else if (applications.records.size() < maximum_applications) {
            applications.records.push_back(
                std::move(shell_application));
        } else {
            combined_applications_complete = false;
        }
    }
    std::sort(
        applications.records.begin(),
        applications.records.end(),
        [](const auto& left, const auto& right) {
            return left.display_name < right.display_name;
        });
    auto processes =
        process_backend_.enumerate_processes(maximum_processes);
    auto windows = window_backend_.enumerate_windows(maximum_windows);

    std::unordered_map<std::uint32_t, std::size_t> process_by_native_id;
    for (std::size_t index = 0; index < processes.records.size(); ++index) {
        process_by_native_id.emplace(
            processes.records[index].native_process_id, index);
    }
    for (const auto& window : windows) {
        const auto process =
            process_by_native_id.find(window.native_process_id);
        if (process != process_by_native_id.end()) {
            processes.records[process->second].window_session_ids.push_back(
                window.session_id);
        }
    }

    const std::string foreground_after =
        window_backend_.foreground_token();
    return ApplicationInventorySnapshot{
        process_backend_.host_session_id(),
        std::move(applications.records),
        std::move(processes.records),
        std::move(windows),
        applications.registry_source_available,
        shell_applications.source_available,
        combined_applications_complete,
        processes.complete,
        foreground_before == foreground_after,
    };
}

ModuleResult ApplicationDiscoveryModule::discover(
    const std::size_t maximum_applications,
    const std::size_t maximum_processes,
    const std::size_t maximum_windows) const {
    const ApplicationInventorySnapshot snapshot = capture(
        maximum_applications, maximum_processes, maximum_windows);
    if (!snapshot.foreground_unchanged) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during read-only application "
            "inventory discovery.",
            nullptr,
        };
    }

    const Relations relations = build_relations(snapshot);
    components::Json::Array applications;
    applications.reserve(snapshot.applications.size());
    std::size_t running_application_count = 0;
    for (const auto& application : snapshot.applications) {
        const auto related =
            relations.application_to_process.find(application.session_id);
        const std::vector<std::string> empty;
        const auto& process_ids =
            related == relations.application_to_process.end()
                ? empty
                : related->second;
        if (!process_ids.empty()) {
            ++running_application_count;
        }
        applications.push_back(components::object({
            {"sessionId", application.session_id},
            {"targetKind", "installed-application"},
            {"displayName", application.display_name},
            {"version", application.version},
            {"publisher", application.publisher},
            {"state", process_ids.empty() ? "installed" : "running"},
            {"runningProcessSessionIds", string_array(process_ids)},
            {"discoverySources",
             string_array(application.discovery_sources)},
            {"launchCapability",
             application.launch_provider ==
                     platform::windows::ApplicationLaunchProvider::
                         shell_item
                 ? "candidate-not-certified"
                 : "unavailable"},
            {"relationshipEvidence",
             process_ids.empty() ? "none" : "exact-normalized-name"},
        }));
    }

    components::Json::Array processes;
    processes.reserve(snapshot.processes.size());
    std::size_t no_window_process_count = 0;
    std::size_t unassociated_process_count = 0;
    std::size_t permission_blocked_process_count = 0;
    std::size_t higher_integrity_process_count = 0;
    for (const auto& process : snapshot.processes) {
        if (process.window_session_ids.empty()) {
            ++no_window_process_count;
        }
        const auto related =
            relations.process_to_application.find(process.session_id);
        const std::vector<std::string> empty;
        const auto& application_ids =
            related == relations.process_to_application.end()
                ? empty
                : related->second;
        if (application_ids.empty()) {
            ++unassociated_process_count;
        }
        if (process.metadata_access ==
            platform::windows::ProcessMetadataAccess::permission_blocked) {
            ++permission_blocked_process_count;
        }
        if (process.integrity_relation ==
            platform::windows::IntegrityRelation::higher) {
            ++higher_integrity_process_count;
        }
        processes.push_back(components::object({
            {"sessionId", process.session_id},
            {"targetKind", "running-process"},
            {"processName", process.process_name},
            {"state", "running"},
            {"identityFreshness",
             process.identity_reliable
                 ? "process-lifetime"
                 : "best-effort-current-snapshot"},
            {"metadataAccess",
             metadata_access_name(process.metadata_access)},
            {"integrityRelation",
             integrity_relation_name(process.integrity_relation)},
            {"hasVisibleWindow", !process.window_session_ids.empty()},
            {"windowVisibility",
             process.window_session_ids.empty()
                 ? "no-visible-titled-window"
                 : "visible-titled-window"},
            {"foregroundRequiredForObservation", false},
            {"windowSessionIds", string_array(process.window_session_ids)},
            {"relatedApplicationIds", string_array(application_ids)},
            {"relationshipStatus",
             application_ids.empty() ? "unassociated" : "matched"},
        }));
    }

    std::unordered_map<std::uint32_t, std::string> process_session_by_native;
    for (const auto& process : snapshot.processes) {
        process_session_by_native.emplace(
            process.native_process_id, process.session_id);
    }
    components::Json::Array windows;
    windows.reserve(snapshot.windows.size());
    for (const auto& window : snapshot.windows) {
        const auto related =
            process_session_by_native.find(window.native_process_id);
        windows.push_back(components::object({
            {"sessionId", window.session_id},
            {"targetKind", "application-window"},
            {"applicationName", window.application_name},
            {"title", window.title},
            {"visible", window.visible},
            {"visibilityState", "visible"},
            {"foregroundRequiredForObservation", false},
            {"processSessionId",
             related == process_session_by_native.end()
                 ? components::Json(nullptr)
                 : components::Json(related->second)},
        }));
    }

    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"hostTargetId", snapshot.host_session_id},
            {"readOnly", true},
            {"foregroundUnchanged", true},
            {"productPromise",
             "broad-general-control-with-capability-degradation"},
            {"coverage",
             components::object({
                 {"runningProcesses", "available"},
                 {"visibleTitledWindows", "available"},
                 {"traditionalInstalledApplications",
                  snapshot.registry_source_available
                      ? "partial-registry-uninstall"
                      : "unavailable"},
                 {"shellApplications",
                  snapshot.shell_source_available
                      ? "available-shell-apps-folder"
                      : "unavailable"},
                 {"storeAndUwpPackages",
                  snapshot.shell_source_available
                      ? "partial-shell-apps-folder"
                      : "unavailable"},
                 {"noWindowProcessesIncluded", true},
                 {"relationshipPolicy",
                  "conservative-exact-normalized-display-or-icon-name"},
             })},
            {"complete",
             components::object({
                 {"applications", snapshot.applications_complete},
                 {"processes", snapshot.processes_complete},
                 {"windows", snapshot.windows.size() < maximum_windows},
             })},
            {"counts",
             components::object({
                 {"installedApplications",
                  static_cast<std::int64_t>(snapshot.applications.size())},
                 {"runningInstalledApplications",
                  static_cast<std::int64_t>(running_application_count)},
                 {"runningProcesses",
                  static_cast<std::int64_t>(snapshot.processes.size())},
                 {"noWindowProcesses",
                  static_cast<std::int64_t>(no_window_process_count)},
                 {"unassociatedProcesses",
                  static_cast<std::int64_t>(unassociated_process_count)},
                 {"permissionBlockedProcesses",
                  static_cast<std::int64_t>(
                      permission_blocked_process_count)},
                 {"higherIntegrityProcesses",
                  static_cast<std::int64_t>(
                      higher_integrity_process_count)},
                 {"visibleWindows",
                  static_cast<std::int64_t>(snapshot.windows.size())},
             })},
            {"applications", components::Json(std::move(applications))},
            {"processes", components::Json(std::move(processes))},
            {"windows", components::Json(std::move(windows))},
        }),
    };
}

ModuleResult ApplicationDiscoveryModule::process_status() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "process.discover@1"},
            {"readOnly", true},
            {"executionDomain", "host-headless"},
            {"backgroundPolicy", "guaranteed"},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

ModuleResult ApplicationDiscoveryModule::process_sessions(
    const std::size_t maximum_items) const {
    if (maximum_items == 0U || maximum_items > 4096U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Process sessions require max-items 1..4096.",
            nullptr,
        };
    }
    const auto foreground_before =
        window_backend_.foreground_token();
    auto processes = attach_windows(
        process_backend_.enumerate_processes(4096U),
        window_backend_.enumerate_windows(4096U));
    const bool foreground_unchanged =
        foreground_before == window_backend_.foreground_token();
    if (!foreground_unchanged) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during process observation.",
            nullptr,
        };
    }
    const std::size_t total = processes.records.size();
    if (processes.records.size() > maximum_items) {
        processes.records.resize(maximum_items);
    }
    components::Json::Array sessions;
    sessions.reserve(processes.records.size());
    for (const auto& process : processes.records) {
        sessions.push_back(process_observation_json(process));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "process.discover@1"},
            {"readOnly", true},
            {"executionDomain", "host-headless"},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"total", static_cast<std::int64_t>(total)},
            {"truncated", total > sessions.size()},
            {"complete", processes.complete},
            {"foregroundUnchanged", true},
            {"sessions", components::Json(std::move(sessions))},
        }),
    };
}

ModuleResult ApplicationDiscoveryModule::inspect_process(
    const std::string& session_id) const {
    const auto foreground_before =
        window_backend_.foreground_token();
    auto processes = attach_windows(
        process_backend_.enumerate_processes(4096U),
        window_backend_.enumerate_windows(4096U));
    if (foreground_before != window_backend_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "Foreground changed during exact process observation.",
            nullptr,
        };
    }
    // 完整扫描进程清单以检测 sessionId 碰撞。
    const auto found = components::match_opaque_target(
        processes.records.begin(),
        processes.records.end(),
        [&session_id](const auto& process) {
            return process.session_id == session_id;
        });
    // 多命中时不返回任意进程的元数据。
    if (found.state == components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定歧义错误且不暴露候选记录。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The opaque running-process session resolves to multiple processes.",
            nullptr,
        };
    }
    // 零命中保持现有过期会话错误。
    if (found.state == components::OpaqueTargetMatchState::missing) {
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The opaque running-process session no longer resolves.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "process.metadata.read@1"},
            {"readOnly", true},
            {"executionDomain", "host-headless"},
            {"foregroundUnchanged", true},
            {"process", process_observation_json(*found.position)},
        }),
    };
}

TargetKindResolution ApplicationDiscoveryModule::resolve_target(
    const ApplicationInventorySnapshot& snapshot,
    const std::string& target_id) const {
    // 默认表示当前清单中没有匹配目标。
    TargetKindResolution resolution{
        components::OpaqueTargetMatchState::missing,
        std::nullopt,
        nullptr,
    };
    // 合并一个目标种类的匹配状态并拒绝跨种类重复。
    const auto observe = [&resolution](
                             const components::OpaqueTargetMatchState state,
                             const TargetKind target_kind) {
        // 已经歧义时保持关闭失败状态。
        if (resolution.state ==
            components::OpaqueTargetMatchState::ambiguous) {
            // 后续记录不能恢复为唯一命中。
            return;
        }
        // 单个种类内部多命中直接形成歧义。
        if (state == components::OpaqueTargetMatchState::ambiguous) {
            // 清除任何先前候选。
            resolution = TargetKindResolution{
                components::OpaqueTargetMatchState::ambiguous,
                std::nullopt,
                nullptr,
            };
            // 结束当前种类合并。
            return;
        }
        // 零命中不会改变已有结果。
        if (state == components::OpaqueTargetMatchState::missing) {
            // 继续等待其他种类的唯一命中。
            return;
        }
        // 第二个种类的命中必须形成歧义。
        if (resolution.state == components::OpaqueTargetMatchState::unique) {
            // 清除跨种类重复候选。
            resolution = TargetKindResolution{
                components::OpaqueTargetMatchState::ambiguous,
                std::nullopt,
                nullptr,
            };
            // 结束当前种类合并。
            return;
        }
        // 保存首个唯一命中的目标种类。
        resolution = TargetKindResolution{
            components::OpaqueTargetMatchState::unique,
            target_kind,
            nullptr,
        };
    };
    // 固定 host 身份也参与跨种类碰撞检查。
    if (target_id == snapshot.host_session_id) {
        // 将 host 作为一个唯一候选合并。
        observe(
            components::OpaqueTargetMatchState::unique,
            TargetKind::host);
    }
    // 完整扫描已安装应用清单。
    const auto application = components::match_opaque_target(
        snapshot.applications.begin(),
        snapshot.applications.end(),
        [&target_id](const auto& candidate) {
            return candidate.session_id == target_id;
        });
    // 合并已安装应用的匹配状态。
    observe(application.state, TargetKind::installed_application);
    // 完整扫描运行进程清单。
    const auto process = components::match_opaque_target(
        snapshot.processes.begin(),
        snapshot.processes.end(),
        [&target_id](const auto& candidate) {
            return candidate.session_id == target_id;
        });
    // 合并运行进程的匹配状态。
    observe(process.state, TargetKind::running_process);
    // 完整扫描应用窗口清单。
    const auto window = components::match_opaque_target(
        snapshot.windows.begin(),
        snapshot.windows.end(),
        [&target_id](const auto& candidate) {
            return candidate.session_id == target_id;
        });
    // 合并应用窗口的匹配状态。
    observe(window.state, TargetKind::application_window);
    // 唯一运行进程命中时保留同一快照内的记录地址。
    if (resolution.state == components::OpaqueTargetMatchState::unique &&
        resolution.target_kind == TargetKind::running_process &&
        process.state == components::OpaqueTargetMatchState::unique) {
        // 记录地址的生命周期受调用方持有的快照约束。
        resolution.process = &*process.position;
    }
    // 返回保留歧义信息的跨种类结果。
    return resolution;
}

}  // namespace act::modules
