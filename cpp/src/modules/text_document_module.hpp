#pragma once

#include "modules/module_result.hpp"
#include "platform/windows/text_document_backend.hpp"

#include <string>
#include <string_view>

namespace act::modules {

class TextDocumentModule final {
public:
    [[nodiscard]] std::string session_id() const;
    [[nodiscard]] bool owns_session(std::string_view session_id) const;
    [[nodiscard]] bool available() const;
    [[nodiscard]] components::Json session_descriptor() const;
    [[nodiscard]] ModuleResult inspect(
        const std::string& session_id) const;
    [[nodiscard]] ModuleResult create(
        std::string_view text,
        bool confirmed) const;

private:
    platform::windows::TextDocumentBackend backend_;
};

}  // namespace act::modules
