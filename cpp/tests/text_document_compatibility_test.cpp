#include "components/json.hpp"
#include "components/utf8.hpp"
#include "modules/text_document_compatibility_module.hpp"

#include <iostream>
#include <string>

int main() {
    using act::components::object;
    using act::modules::ModuleResult;
    using act::modules::TextDocumentCompatibilityModule;

    if (!act::components::valid_utf8("hello") ||
        !act::components::valid_utf8("\xE4\xB8\xAD\xE6\x96\x87") ||
        act::components::valid_utf8("\xC0\xAF") ||
        act::components::valid_utf8("\xED\xA0\x80")) {
        std::cerr << "UTF-8 validation policy failed\n";
        return 1;
    }

    TextDocumentCompatibilityModule compatibility;
    const ModuleResult provider{
        true,
        {},
        {},
        object({
            {"capability", "text.document.create@1"},
            {"path", "C:\\Temp\\fixture.txt"},
            {"text", "hello"},
            {"launcherProcessId", 42},
            {"foregroundUnchanged", true},
            {"encoding", "utf-8"},
            {"mediaType", "text/plain"},
        }),
    };
    const auto legacy = compatibility.map_legacy(provider);
    const auto facade = compatibility.map_app(
        provider, "s2:a:0123456789abcdef");
    if (!legacy.ok ||
        legacy.data.find("launcherProcessId") == nullptr ||
        legacy.data.find("inputMethod") == nullptr ||
        !facade.ok ||
        facade.data.find("launcherProcessId") != nullptr ||
        facade.data.dump().find("notepad") != std::string::npos ||
        facade.data.dump().find("System32") != std::string::npos ||
        facade.data.find("compatibilityShape") == nullptr ||
        *facade.data.find("compatibilityShape")->string_value() !=
            "provider-neutral-text-artifact-v1") {
        std::cerr << "text compatibility mapping policy failed\n";
        return 1;
    }

    const auto incomplete = compatibility.map_app(
        ModuleResult{
            true, {}, {}, object({{"path", "fixture.txt"}})},
        "s2:a:0123456789abcdef");
    if (incomplete.ok ||
        incomplete.error_code != "OPERATION_FAILED") {
        std::cerr << "incomplete provider result was accepted\n";
        return 1;
    }

    std::cout
        << object({
               {"utf8Validation", true},
               {"legacyShape", true},
               {"facadeSanitized", true},
               {"incompleteResultRefused", true},
           })
               .dump()
        << '\n';
    return 0;
}
