#include "components/json.hpp"
// 复用主进程相同的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <iostream>
#include <limits>
#include <optional>
#include <regex>
#include <string>

namespace {

using act::components::Json;
using act::platform::windows::BackendError;
using act::platform::windows::DiscoveryBackend;
using act::platform::windows::WindowRecord;

constexpr const char* contract_version = "act/observation-worker/v1";

int write_result(const Json& result, const int exit_code) {
    std::cout << result.dump() << '\n';
    return exit_code;
}

int fail(
    const char* code,
    const std::string& message,
    const int exit_code = 2) {
    return write_result(
        act::components::object({
            {"ok", false},
            {"contractVersion", contract_version},
            {"error",
             act::components::object({
                 {"code", code},
                 {"message", message},
             })},
        }),
        exit_code);
}

const std::string* string_field(
    const Json& object,
    const std::string_view key) {
    const Json* value = object.find(key);
    return value == nullptr ? nullptr : value->string_value();
}

const std::int64_t* integer_field(
    const Json& object,
    const std::string_view key) {
    const Json* value = object.find(key);
    return value == nullptr ? nullptr : value->integer_value();
}

bool valid_session_id(const std::string& value) {
    static const std::regex pattern("^s2:w:[0-9a-f]{16}$");
    return std::regex_match(value, pattern);
}

// 保存跨局部清单生命周期的窗口解析结果。
struct WindowResolution {
    // 保存 missing、unique 或 ambiguous 状态。
    act::components::OpaqueTargetMatchState state;
    // 仅在唯一命中时复制窗口记录。
    std::optional<WindowRecord> window;
// 结束窗口解析结果定义。
};

// 重新枚举并解析唯一 opaque 窗口目标。
WindowResolution resolve_window(
    const DiscoveryBackend& backend,
    const std::string& session_id) {
    auto windows = backend.enumerate_windows(4096U);
    // 完整扫描 worker 当前窗口清单以检测碰撞。
    const auto match = act::components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 非唯一命中时不复制任何候选窗口。
    if (match.state != act::components::OpaqueTargetMatchState::unique) {
        // 保留 missing 或 ambiguous 状态供协议层映射。
        return WindowResolution{match.state, std::nullopt};
    }
    // 唯一命中时复制记录以跨越局部清单生命周期。
    return WindowResolution{match.state, *match.position};
}

int backend_error(const BackendError& error) {
    return fail(error.code.c_str(), error.message);
}

int observe_root(
    const DiscoveryBackend& backend,
    const WindowRecord& window) {
    const auto result = backend.read_accessibility_root(window);
    if (result.error.has_value()) {
        return backend_error(*result.error);
    }
    const auto& root = *result.root;
    return write_result(
        act::components::object({
            {"ok", true},
            {"contractVersion", contract_version},
            {"data",
             act::components::object({
                 {"name", root.name},
                 {"automationId", root.automation_id},
                 {"className", root.class_name},
                 {"frameworkId", root.framework_id},
                 {"controlType", root.control_type},
                 {"enabled", root.enabled},
                 {"offscreen", root.offscreen},
             })},
        }),
        0);
}

int observe_tree(
    const DiscoveryBackend& backend,
    const WindowRecord& window,
    const Json& request) {
    const auto* maximum_depth = integer_field(request, "maximumDepth");
    const auto* maximum_items = integer_field(request, "maximumItems");
    const auto* view = string_field(request, "view");
    if (maximum_depth == nullptr ||
        *maximum_depth < 0 ||
        *maximum_depth > 20 ||
        maximum_items == nullptr ||
        *maximum_items < 1 ||
        *maximum_items > 4096 ||
        view == nullptr ||
        (*view != "control" && *view != "raw")) {
        return fail(
            "INVALID_ARGUMENT",
            "The bounded accessibility-tree request is invalid.");
    }
    const auto result = backend.read_accessibility_tree(
        window,
        static_cast<std::size_t>(*maximum_depth),
        static_cast<std::size_t>(*maximum_items),
        *view);
    if (result.error.has_value()) {
        return backend_error(*result.error);
    }
    Json::Array nodes;
    nodes.reserve(result.tree->nodes.size());
    for (const auto& node : result.tree->nodes) {
        nodes.push_back(act::components::object({
            {"nodeId", node.node_id},
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
    return write_result(
        act::components::object({
            {"ok", true},
            {"contractVersion", contract_version},
            {"data",
             Json(Json::Object{
                 {"visited",
                  static_cast<std::int64_t>(result.tree->visited)},
                 {"truncated", result.tree->truncated},
                 {"nodes", Json(std::move(nodes))},
             })},
        }),
        0);
}

}  // namespace

int main() {
    std::string request_text;
    if (!std::getline(std::cin, request_text)) {
        return fail(
            "INVALID_ARGUMENT",
            "The observation worker requires one JSON request.");
    }
    std::string parse_error;
    auto request = Json::parse(request_text, parse_error);
    if (!request.has_value() || request->object_items() == nullptr) {
        return fail(
            "INVALID_ARGUMENT",
            "The observation worker request is not valid JSON.");
    }
    const auto* contract = string_field(*request, "contractVersion");
    const auto* operation = string_field(*request, "operation");
    const auto* session_id = string_field(*request, "sessionId");
    if (contract == nullptr ||
        *contract != contract_version ||
        operation == nullptr ||
        session_id == nullptr ||
        !valid_session_id(*session_id)) {
        return fail(
            "INVALID_ARGUMENT",
            "The observation worker request violates protocol v1.");
    }

    const DiscoveryBackend backend;
    const auto window = resolve_window(backend, *session_id);
    // 多命中时不允许 worker 观测任意窗口。
    if (window.state == act::components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定的 JSON over stdio 歧义错误。
        return fail(
            "AMBIGUOUS_TARGET",
            "The opaque application-window session resolves to multiple windows.");
    }
    // 零命中保持现有过期会话错误。
    if (window.state == act::components::OpaqueTargetMatchState::missing ||
        !window.window.has_value()) {
        return fail(
            "STALE_SESSION",
            "The opaque application-window session no longer resolves.");
    }
    if (*operation == "accessibility-root") {
        return observe_root(backend, *window.window);
    }
    if (*operation == "accessibility-tree") {
        return observe_tree(backend, *window.window, *request);
    }
    return fail(
        "CAPABILITY_GAP",
        "The isolated observation worker only supports read-only UIA.");
}
