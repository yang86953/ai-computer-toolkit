#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/structured_image_backend.hpp"
#include "platform/windows/structured_image_observation_backend.hpp"

#include <filesystem>
#include <string>

namespace act::modules {

class StructuredImageModule final {
public:
    [[nodiscard]] ModuleResult status() const;
    [[nodiscard]] ModuleResult sessions() const;
    [[nodiscard]] ModuleResult inspect(
        const std::string& session_id) const;
    [[nodiscard]] ModuleResult save(
        const std::string& session_id,
        const std::filesystem::path& path,
        bool overwrite,
        bool confirmed) const;
    [[nodiscard]] ModuleResult export_png(
        const std::string& session_id,
        const std::filesystem::path& path,
        bool overwrite,
        bool confirmed) const;

private:
    [[nodiscard]] ModuleResult write(
        const std::string& capability,
        const std::string& session_id,
        const std::filesystem::path& path,
        const std::string& extension,
        bool overwrite,
        bool confirmed) const;

    platform::windows::StructuredImageBackend backend_;
    platform::windows::StructuredImageObservationBackend
        observation_;
};

}  // namespace act::modules
