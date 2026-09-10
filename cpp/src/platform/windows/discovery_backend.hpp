#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

struct WindowRecord {
    std::string session_id;
    std::string application_name;
    std::string title;
    bool visible;

    // Backend-only identity data. This header is private to the executable and
    // never crosses the public C++ or JSON boundary.
    std::uintptr_t native_window;
    std::uint32_t native_process_id;
};

struct AccessibilityRoot {
    std::string name;
    std::string automation_id;
    std::string class_name;
    std::string framework_id;
    int control_type;
    bool enabled;
    bool offscreen;
};

struct BackendError {
    std::string code;
    std::string message;
};

struct AccessibilityResult {
    std::optional<AccessibilityRoot> root;
    std::optional<BackendError> error;
};

struct AccessibilityNode {
    std::string node_id;
    std::size_t depth;
    std::string name;
    std::string automation_id;
    std::string class_name;
    std::string framework_id;
    int control_type;
    bool enabled;
    bool offscreen;
    bool property_read_complete;
};

struct AccessibilityTree {
    std::vector<AccessibilityNode> nodes;
    std::size_t visited;
    bool truncated;
};

struct AccessibilityTreeResult {
    std::optional<AccessibilityTree> tree;
    std::optional<BackendError> error;
};

class DiscoveryBackend final {
public:
    [[nodiscard]] std::vector<WindowRecord> enumerate_windows(
        std::size_t maximum_items) const;
    [[nodiscard]] AccessibilityResult read_accessibility_root(
        const WindowRecord& window) const;
    [[nodiscard]] AccessibilityTreeResult read_accessibility_tree(
        const WindowRecord& window,
        std::size_t maximum_depth,
        std::size_t maximum_items,
        const std::string& view) const;
    [[nodiscard]] std::string foreground_token() const;
};

}  // namespace act::platform::windows
