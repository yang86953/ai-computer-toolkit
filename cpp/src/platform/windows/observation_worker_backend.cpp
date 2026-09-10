#include "platform/windows/observation_worker_backend.hpp"

#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <limits>
#include <optional>

namespace act::platform::windows {
namespace {

constexpr const char* worker_contract = "act/observation-worker/v1";
constexpr const char* worker_name =
    "ai-computer-toolkit-observation-worker.exe";

BackendError process_error(
    const components::WorkerProcessResult& result) {
    using Completion = components::WorkerCompletion;
    switch (result.completion) {
        case Completion::timed_out:
            return BackendError{"TIMEOUT", result.error_message};
        case Completion::cancelled:
            return BackendError{"CANCELLED", result.error_message};
        case Completion::unavailable:
            return BackendError{
                "ISOLATED_WORKER_UNAVAILABLE", result.error_message};
        case Completion::protocol_failure:
            return BackendError{"OPERATION_FAILED", result.error_message};
        case Completion::completed:
            break;
    }
    return BackendError{
        "OPERATION_FAILED",
        "The isolated observation worker returned an invalid result.",
    };
}

const components::Json* required_field(
    const components::Json& value,
    const std::string_view key) {
    return value.find(key);
}

std::optional<BackendError> parse_envelope(
    const components::WorkerProcessResult& process,
    components::Json& parsed,
    const components::Json*& data) {
    if (process.completion != components::WorkerCompletion::completed) {
        return process_error(process);
    }
    std::string parse_error;
    auto value = components::Json::parse(process.stdout_text, parse_error);
    if (!value.has_value()) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated observation worker returned malformed JSON.",
        };
    }
    parsed = std::move(*value);
    const auto* contract = required_field(parsed, "contractVersion");
    const auto* ok = required_field(parsed, "ok");
    if (contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated observation worker violated its protocol envelope.",
        };
    }
    if (!*ok->bool_value()) {
        const auto* error = required_field(parsed, "error");
        const auto* code =
            error == nullptr ? nullptr : required_field(*error, "code");
        const auto* message =
            error == nullptr ? nullptr : required_field(*error, "message");
        if (code == nullptr ||
            code->string_value() == nullptr ||
            message == nullptr ||
            message->string_value() == nullptr) {
            return BackendError{
                "OPERATION_FAILED",
                "The isolated observation worker returned an invalid error.",
            };
        }
        return BackendError{
            *code->string_value(), *message->string_value()};
    }
    data = required_field(parsed, "data");
    if (data == nullptr || data->object_items() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated observation worker omitted result data.",
        };
    }
    return std::nullopt;
}

bool read_string(
    const components::Json& object,
    const std::string_view key,
    std::string& output) {
    const auto* field = object.find(key);
    if (field == nullptr || field->string_value() == nullptr) {
        return false;
    }
    output = *field->string_value();
    return true;
}

bool read_bool(
    const components::Json& object,
    const std::string_view key,
    bool& output) {
    const auto* field = object.find(key);
    if (field == nullptr || field->bool_value() == nullptr) {
        return false;
    }
    output = *field->bool_value();
    return true;
}

bool read_int(
    const components::Json& object,
    const std::string_view key,
    std::int64_t& output) {
    const auto* field = object.find(key);
    if (field == nullptr || field->integer_value() == nullptr) {
        return false;
    }
    output = *field->integer_value();
    return true;
}

AccessibilityResult invalid_root_result() {
    return AccessibilityResult{
        std::nullopt,
        BackendError{
            "OPERATION_FAILED",
            "The isolated worker returned an invalid accessibility root.",
        },
    };
}

AccessibilityTreeResult invalid_tree_result() {
    return AccessibilityTreeResult{
        std::nullopt,
        BackendError{
            "OPERATION_FAILED",
            "The isolated worker returned an invalid accessibility tree.",
        },
    };
}

components::WorkerProcessResult run_request(
    components::Json request,
    const std::uint32_t timeout_ms,
    const std::size_t maximum_output_bytes) {
    return components::WorkerProcess().run_companion(
        worker_name, request.dump(), timeout_ms, maximum_output_bytes);
}

}  // namespace

AccessibilityResult ObservationWorkerBackend::read_accessibility_root(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    auto process = run_request(
        components::object({
            {"contractVersion", worker_contract},
            {"operation", "accessibility-root"},
            {"sessionId", session_id},
        }),
        timeout_ms,
        1024U * 1024U);
    components::Json parsed;
    const components::Json* data = nullptr;
    const auto error = parse_envelope(process, parsed, data);
    if (error.has_value()) {
        return AccessibilityResult{std::nullopt, error};
    }

    AccessibilityRoot root{};
    std::int64_t control_type = 0;
    if (!read_string(*data, "name", root.name) ||
        !read_string(*data, "automationId", root.automation_id) ||
        !read_string(*data, "className", root.class_name) ||
        !read_string(*data, "frameworkId", root.framework_id) ||
        !read_int(*data, "controlType", control_type) ||
        control_type < std::numeric_limits<int>::min() ||
        control_type > std::numeric_limits<int>::max() ||
        !read_bool(*data, "enabled", root.enabled) ||
        !read_bool(*data, "offscreen", root.offscreen)) {
        return invalid_root_result();
    }
    root.control_type = static_cast<int>(control_type);
    return AccessibilityResult{std::move(root), std::nullopt};
}

AccessibilityTreeResult ObservationWorkerBackend::read_accessibility_tree(
    const std::string& session_id,
    const std::size_t maximum_depth,
    const std::size_t maximum_items,
    const std::string& view,
    const std::uint32_t timeout_ms) const {
    auto process = run_request(
        components::object({
            {"contractVersion", worker_contract},
            {"operation", "accessibility-tree"},
            {"sessionId", session_id},
            {"maximumDepth",
             static_cast<std::int64_t>(maximum_depth)},
            {"maximumItems",
             static_cast<std::int64_t>(maximum_items)},
            {"view", view},
        }),
        timeout_ms,
        16U * 1024U * 1024U);
    components::Json parsed;
    const components::Json* data = nullptr;
    const auto error = parse_envelope(process, parsed, data);
    if (error.has_value()) {
        return AccessibilityTreeResult{std::nullopt, error};
    }

    const auto* node_value = data->find("nodes");
    const auto* nodes =
        node_value == nullptr ? nullptr : node_value->array_items();
    std::int64_t visited = 0;
    bool truncated = false;
    if (nodes == nullptr ||
        !read_int(*data, "visited", visited) ||
        visited < 0 ||
        !read_bool(*data, "truncated", truncated)) {
        return invalid_tree_result();
    }
    std::vector<AccessibilityNode> decoded;
    decoded.reserve(nodes->size());
    for (const auto& value : *nodes) {
        AccessibilityNode node{};
        std::int64_t depth = 0;
        std::int64_t control_type = 0;
        if (value.object_items() == nullptr ||
            !read_string(value, "nodeId", node.node_id) ||
            !read_int(value, "depth", depth) ||
            depth < 0 ||
            static_cast<std::uint64_t>(depth) >
                std::numeric_limits<std::size_t>::max() ||
            !read_string(value, "name", node.name) ||
            !read_string(value, "automationId", node.automation_id) ||
            !read_string(value, "className", node.class_name) ||
            !read_string(value, "frameworkId", node.framework_id) ||
            !read_int(value, "controlType", control_type) ||
            control_type < std::numeric_limits<int>::min() ||
            control_type > std::numeric_limits<int>::max() ||
            !read_bool(value, "enabled", node.enabled) ||
            !read_bool(value, "offscreen", node.offscreen) ||
            !read_bool(
                value,
                "propertyReadComplete",
                node.property_read_complete)) {
            return invalid_tree_result();
        }
        node.depth = static_cast<std::size_t>(depth);
        node.control_type = static_cast<int>(control_type);
        decoded.push_back(std::move(node));
    }
    return AccessibilityTreeResult{
        AccessibilityTree{
            std::move(decoded),
            static_cast<std::size_t>(visited),
            truncated,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
