#pragma once

#include "modules/capability_assessment_module.hpp"
#include "modules/discovery_module.hpp"
#include "modules/module_result.hpp"
#include "modules/standard_edit_compatibility_module.hpp"
#include "modules/standard_edit_module.hpp"
#include "modules/structured_image_module.hpp"
#include "modules/text_document_compatibility_module.hpp"
#include "modules/text_document_module.hpp"
#include "modules/window_close_compatibility_module.hpp"
#include "modules/window_close_module.hpp"

#include <cstddef>
#include <cstdint>
#include <vector>

namespace act::systems {

class ApplicationFacadeSystem final {
public:
    [[nodiscard]] modules::ModuleResult status() const;
    [[nodiscard]] modules::ModuleResult sessions(
        std::size_t maximum_items) const;
    [[nodiscard]] modules::ModuleResult inspect(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] bool owns_text_document_session(
        const std::string& session_id) const;
    [[nodiscard]] modules::ModuleResult assess_standard_edit(
        const std::string& capability,
        const std::string& session_id) const;
    [[nodiscard]] modules::ModuleResult run_app_create(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_app_apply(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_app_close(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_legacy_notepad(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_legacy_standard_edit(
        const std::vector<std::string>& arguments) const;
    [[nodiscard]] modules::ModuleResult run_legacy_type_text(
        const std::vector<std::string>& arguments) const;

private:
    modules::DiscoveryModule discovery_;
    modules::TextDocumentModule text_documents_;
    modules::TextDocumentCompatibilityModule compatibility_;
    modules::StandardEditModule standard_edits_;
    modules::StandardEditCompatibilityModule
        standard_edit_compatibility_;
    modules::StructuredImageModule structured_images_;
    modules::CapabilityAssessmentModule assessment_;
    modules::WindowCloseModule window_close_;
    modules::WindowCloseCompatibilityModule
        window_close_compatibility_;
};

}  // namespace act::systems
