#pragma once

#include "modules/module_result.hpp"

#include <optional>
#include <string>

namespace act::modules {

class CapabilityDirectoryModule final {
public:
    [[nodiscard]] ModuleResult version() const;
    [[nodiscard]] ModuleResult build_info() const;
    [[nodiscard]] ModuleResult help() const;
    [[nodiscard]] ModuleResult catalog(
        const std::optional<std::string>& app_id) const;
    [[nodiscard]] ModuleResult capabilities() const;
    [[nodiscard]] ModuleResult methods(
        const std::optional<std::string>& method_id) const;
    [[nodiscard]] ModuleResult method_capabilities(
        const std::optional<std::string>& method_id) const;
    [[nodiscard]] ModuleResult describe(
        const std::string& app_id,
        const std::optional<std::string>& operation_id) const;
    [[nodiscard]] ModuleResult descriptor_capabilities(
        const std::string& app_id,
        const std::optional<std::string>& operation_id) const;
};

}  // namespace act::modules
