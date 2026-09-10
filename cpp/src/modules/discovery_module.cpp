#include "modules/discovery_module.hpp"

// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"

// 提供可选失败结果以复用窗口解析错误映射。
#include <optional>

namespace act::modules {
namespace {

// 把唯一匹配状态映射成窗口解析的稳定公共错误。
std::optional<ModuleResult> opaque_window_resolution_failure(
    const components::OpaqueTargetMatchState state) {
    // 多命中表示 opaque 目标存在碰撞或重复身份。
    if (state == components::OpaqueTargetMatchState::ambiguous) {
        // 不返回任意候选窗口。
        return ModuleResult{
            false,
            "AMBIGUOUS_TARGET",
            "The opaque application-window session resolves to multiple windows.",
            nullptr,
        };
    }
    // 零命中表示目标已从当前清单中消失。
    if (state == components::OpaqueTargetMatchState::missing) {
        // 保持既有过期会话错误。
        return ModuleResult{
            false,
            "STALE_SESSION",
            "The opaque application-window session no longer resolves.",
            nullptr,
        };
    }
    // 唯一命中不产生失败结果。
    return std::nullopt;
// 结束窗口解析错误映射。
}

components::Json capability(
    const char* id,
    const char* execution_domain,
    const char* constraint) {
    return components::object({
        {"id", id},
        {"risk", "read"},
        {"executionDomain", execution_domain},
        {"availability", "available"},
        {"requiresConfirmation", false},
        {"constraint", constraint},
    });
}

components::Json window_json(
    const platform::windows::WindowRecord& window) {
    return components::object({
        {"sessionId", window.session_id},
        {"kind", "window"},
        {"targetKind", "application-window"},
        {"applicationName", window.application_name},
        {"title", window.title},
        {"visible", window.visible},
        {"capabilities",
         components::Json(components::Json::Array{
             capability(
                 "application.discover@1",
                 "host-headless",
                 "running-applications-only"),
             capability(
                 "window.discover@1",
                 "host-headless",
                 "visible-titled-top-level-windows"),
            capability(
                 "accessibility.tree.read@1",
                 "isolated-worker",
                 "same-session-bounded-tree-without-value-text-or-bounds"),
            components::object({
                {"id", "window.capture.frame.probe@1"},
                {"risk", "read-sensitive"},
                {"executionDomain", "isolated-worker"},
                {"availability", "available"},
                {"requiresConfirmation", true},
                {"constraint",
                 "frame-metadata-only-no-surface-read-no-file-no-activation"},
            }),
            components::object({
                {"id", "window.screenshot@1"},
                {"version", 1},
                {"verb", "screenshot"},
                {"risk", "read-sensitive"},
                {"executionDomain", "isolated-worker"},
                {"availability", "available"},
                {"requiresConfirmation", true},
                {"requiresForegroundConsent", false},
                {"inputSchema", "schema://window/screenshot/v1"},
            }),
            components::object({
                {"id", "window.close@1"},
                {"version", 1},
                {"verb", "close"},
                {"risk", "mutation"},
                {"executionDomain", "same-session-no-focus"},
                {"availability", "available"},
                {"requiresConfirmation", true},
                {"requiresForegroundConsent", false},
                {"inputSchema", "schema://window/close/v1"},
            }),
         })},
    });
}

}  // namespace

ModuleResult DiscoveryModule::status() const {
    const auto windows = backend_.enumerate_windows(1);
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"platform", "windows"},
            {"mainImplementation", "cpp"},
            {"compatibilityEntrypoint", "rust-cargo"},
            {"canDiscoverApplications", true},
            {"canReadAccessibilityRoot", true},
            {"accessibilityObservationIsolation", "job-bounded-worker"},
            {"accessibilityObservationTimeoutMs", 5000},
            {"accessibilityObservationCancellable", true},
            {"visibleWindowAvailable", !windows.empty()},
            {"writesEnabled", false},
            {"supportLevel", "L1-observation"},
        }),
    };
}

ModuleResult DiscoveryModule::window_status() const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096U);
    if (foreground_before != backend_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during window status.",
            nullptr,
        };
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.discover@1"},
            {"readOnly", true},
            {"executionDomain", "host-headless"},
            {"backgroundPolicy", "guaranteed"},
            {"visibleTitledWindowCount",
             static_cast<std::int64_t>(windows.size())},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
        }),
    };
}

ModuleResult DiscoveryModule::sessions(
    const std::size_t maximum_items) const {
    const auto foreground_before = backend_.foreground_token();
    auto windows = backend_.enumerate_windows(4096U);
    const auto foreground_after = backend_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during read-only discovery.",
            nullptr,
        };
    }

    const std::size_t total = windows.size();
    if (windows.size() > maximum_items) {
        windows.resize(maximum_items);
    }
    components::Json::Array sessions;
    sessions.reserve(windows.size());
    for (const auto& window : windows) {
        sessions.push_back(window_json(window));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"surface", "app"},
            {"capability", "window.discover@1"},
            {"readOnly", true},
            {"foregroundUnchanged", true},
            {"targetIdentity", "opaque-versioned-session-id"},
            {"count",
             static_cast<std::int64_t>(sessions.size())},
            {"total", static_cast<std::int64_t>(total)},
            {"truncated", total > sessions.size()},
            {"sessions", components::Json(std::move(sessions))},
        }),
    };
}

ModuleResult DiscoveryModule::inspect_window(
    const std::string& session_id) const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    if (foreground_before != backend_.foreground_token()) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during window inspection.",
            nullptr,
        };
    }
    // 前台稳定性检查后再返回目标解析结果。
    if (const auto failure = opaque_window_resolution_failure(match.state);
        failure.has_value()) {
        // 返回 fail-closed 的稳定错误。
        return *failure;
    }
    // 唯一命中后才读取窗口观测数据。
    const auto& window = *match.position;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.metadata.read@1"},
            {"readOnly", true},
            {"executionDomain", "host-headless"},
            {"foregroundUnchanged", true},
            {"window", window_json(window)},
        }),
    };
}

ModuleResult DiscoveryModule::inspect(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 在启动观测 worker 前拒绝零命中或多命中。
    if (const auto failure = opaque_window_resolution_failure(match.state);
        failure.has_value()) {
        // 返回 fail-closed 的稳定错误。
        return *failure;
    }
    // 唯一命中后才把会话交给观测 worker。
    const auto& window = *match.position;

    const auto accessibility =
        observation_worker_.read_accessibility_root(
            window.session_id, timeout_ms);
    const auto foreground_after = backend_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during read-only inspection.",
            nullptr,
        };
    }
    if (accessibility.error.has_value()) {
        const std::string public_code =
            accessibility.error->code == "ACCESSIBILITY_UNAVAILABLE"
                ? "BACKGROUND_OPERATION_UNAVAILABLE"
                : accessibility.error->code;
        return ModuleResult{
            false,
            public_code,
            accessibility.error->message,
            nullptr,
        };
    }

    const auto& root = *accessibility.root;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"session", window_json(window)},
            {"readOnly", true},
            {"foregroundUnchanged", true},
            {"accessibility",
             components::object({
                 {"scope", "root-only"},
                 {"name", root.name},
                 {"automationId", root.automation_id},
                 {"className", root.class_name},
                 {"frameworkId", root.framework_id},
                 {"controlType", root.control_type},
                 {"enabled", root.enabled},
                 {"offscreen", root.offscreen},
             })},
        }),
    };
}

ModuleResult DiscoveryModule::inspect_tree(
    const std::string& session_id,
    const std::size_t maximum_depth,
    const std::size_t maximum_items,
    const std::string& view,
    const std::uint32_t timeout_ms) const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 在启动树观测 worker 前拒绝零命中或多命中。
    if (const auto failure = opaque_window_resolution_failure(match.state);
        failure.has_value()) {
        // 返回 fail-closed 的稳定错误。
        return *failure;
    }
    // 唯一命中后才把会话交给树观测 worker。
    const auto& window = *match.position;

    const auto accessibility =
        observation_worker_.read_accessibility_tree(
            window.session_id,
            maximum_depth,
            maximum_items,
            view,
            timeout_ms);
    const auto foreground_after = backend_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during bounded read-only UIA "
            "inspection.",
            nullptr,
        };
    }
    if (accessibility.error.has_value()) {
        const std::string public_code =
            accessibility.error->code == "ACCESSIBILITY_UNAVAILABLE"
                ? "BACKGROUND_OPERATION_UNAVAILABLE"
                : accessibility.error->code;
        return ModuleResult{
            false,
            public_code,
            accessibility.error->message,
            nullptr,
        };
    }

    components::Json::Array nodes;
    nodes.reserve(accessibility.tree->nodes.size());
    for (const auto& node : accessibility.tree->nodes) {
        nodes.push_back(components::object({
            {"nodeId", node.node_id},
            {"identityFreshness", "inspection-snapshot"},
            {"depth", static_cast<std::int64_t>(node.depth)},
            {"name", node.name},
            {"automationId", node.automation_id},
            {"className", node.class_name},
            {"frameworkId", node.framework_id},
            {"controlType", node.control_type},
            {"enabled", node.enabled},
            {"offscreen", node.offscreen},
            {"propertyReadComplete", node.property_read_complete},
        }));
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"session", window_json(window)},
            {"scope", "bounded-tree"},
            {"view", view},
            {"readOnly", true},
            {"foregroundUnchanged", true},
            {"maximumDepth", static_cast<std::int64_t>(maximum_depth)},
            {"maximumItems", static_cast<std::int64_t>(maximum_items)},
            {"visited",
             static_cast<std::int64_t>(accessibility.tree->visited)},
            {"truncated", accessibility.tree->truncated},
            {"nodes", components::Json(std::move(nodes))},
            {"safety",
             components::object({
                 {"valueContentRead", false},
                 {"textContentRead", false},
                 {"writePatternsQueried", false},
                 {"boundsExposed", false},
                {"providerTimeoutIsolation", "job-bounded-worker"},
                {"workerTimeoutMs",
                 static_cast<std::int64_t>(timeout_ms)},
                {"workerCancellable", true},
             })},
        }),
    };
}

ModuleResult DiscoveryModule::capture_preflight(
    const std::string& session_id) const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 在调用捕获预检前拒绝零命中或多命中。
    if (const auto failure = opaque_window_resolution_failure(match.state);
        failure.has_value()) {
        // 返回 fail-closed 的稳定错误。
        return *failure;
    }
    // 唯一命中后才把窗口交给预检后端。
    const auto& window = *match.position;

    const auto result = capture_preflight_.inspect(window);
    const auto foreground_after = backend_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during capture preflight.",
            nullptr,
        };
    }
    if (result.error.has_value()) {
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
        };
    }

    const auto& evidence = *result.preflight;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.capture.preflight@1"},
            {"targetId", session_id},
            {"targetKind", "application-window"},
            {"readOnly", true},
            {"foregroundUnchanged", true},
            {"foregroundRequired", false},
            {"confirmationRequired", false},
            {"captureExecutionMigrated", false},
            {"captureCompatibilityEntrypoint", "rust-cargo"},
            {"privacyIndicatorMayAppearOnCapture", true},
            {"evidence",
             components::object({
                 {"visible", evidence.visible},
                 {"minimized", evidence.minimized},
                 {"cloaked", evidence.cloaked},
                 {"nonzeroExtent", evidence.nonzero_extent},
                 {"desktopCompositionEnabled",
                  evidence.desktop_composition_enabled},
                 {"contentProtection",
                  evidence.content_protection},
                 {"wgcRuntime", evidence.wgc_runtime},
                 {"wgcItemInterop", evidence.wgc_item_interop},
                 {"captureItemNonzeroSize",
                  evidence.capture_item_nonzero_size},
                 {"eligibility", evidence.eligibility},
             })},
            {"safety",
             components::object({
                 {"pixelsRead", false},
                 {"captureSessionStarted", false},
                 {"framePoolCreated", false},
                 {"fileWritten", false},
                 {"windowActivated", false},
                 {"inputSent", false},
                 {"nativeTargetExposed", false},
             })},
        }),
    };
}

ModuleResult DiscoveryModule::capture_frame_probe(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    const auto foreground_before = backend_.foreground_token();
    const auto windows = backend_.enumerate_windows(4096U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 在调用捕获探针前拒绝零命中或多命中。
    if (const auto failure = opaque_window_resolution_failure(match.state);
        failure.has_value()) {
        // 返回 fail-closed 的稳定错误。
        return *failure;
    }
    // 唯一命中后才把窗口交给预检后端。
    const auto& window = *match.position;

    const auto preflight = capture_preflight_.inspect(window);
    if (preflight.error.has_value()) {
        return ModuleResult{
            false,
            preflight.error->code,
            preflight.error->message,
            nullptr,
        };
    }
    if (preflight.preflight->eligibility !=
        "eligible-for-certified-capture-route") {
        return ModuleResult{
            false,
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact target is not eligible for background capture: " +
                preflight.preflight->eligibility + ".",
            nullptr,
        };
    }

    const auto result =
        capture_worker_.probe_frame(session_id, timeout_ms);
    const auto foreground_after = backend_.foreground_token();
    if (foreground_before != foreground_after) {
        return ModuleResult{
            false,
            "HOST_INTERFERENCE_DETECTED",
            "The foreground target changed during the capture frame probe.",
            nullptr,
        };
    }
    if (result.error.has_value()) {
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
        };
    }

    const auto& frame = *result.frame;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.capture.frame.probe@1"},
            {"targetId", session_id},
            {"targetKind", "application-window"},
            {"readOnly", true},
            {"confirmationRequired", true},
            {"confirmationSatisfied", true},
            {"executionDomain", "isolated-worker"},
            {"foregroundRequired", false},
            {"foregroundUnchanged", true},
            {"frame",
             components::object({
                 {"width", frame.width},
                 {"height", frame.height},
                 {"deviceDriver", frame.device_driver},
             })},
            {"safety",
             components::object({
                 {"frameAcquired", true},
                 {"frameSurfaceAccessed",
                  frame.frame_surface_accessed},
                 {"pixelsPersisted", frame.pixels_persisted},
                 {"fileWritten", frame.file_written},
                 {"windowActivated", false},
                 {"inputSent", false},
                 {"nativeTargetExposed", false},
                 {"privacyIndicatorMayHaveAppeared",
                  frame.privacy_indicator_may_have_appeared},
                 {"workerTimeoutMs",
                  static_cast<std::int64_t>(timeout_ms)},
                 {"workerCancellable", true},
             })},
        }),
    };
}

}  // namespace act::modules
