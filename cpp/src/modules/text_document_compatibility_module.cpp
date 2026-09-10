#include "modules/text_document_compatibility_module.hpp"

namespace act::modules {
namespace {

bool complete(const ModuleResult& result) {
    return result.data.find("path") != nullptr &&
           result.data.find("path")->string_value() != nullptr &&
           result.data.find("text") != nullptr &&
           result.data.find("text")->string_value() != nullptr &&
           result.data.find("launcherProcessId") != nullptr &&
           result.data.find("launcherProcessId")->integer_value() != nullptr &&
           result.data.find("foregroundUnchanged") != nullptr &&
           result.data.find("foregroundUnchanged")->bool_value() != nullptr &&
           *result.data.find("foregroundUnchanged")->bool_value();
}

ModuleResult incomplete_result() {
    return ModuleResult{
        false,
        "OPERATION_FAILED",
        "The text document provider result is incomplete.",
        nullptr,
    };
}

}  // namespace

ModuleResult TextDocumentCompatibilityModule::map_legacy(
    ModuleResult provider_result) const {
    if (!provider_result.ok) {
        return provider_result;
    }
    if (!complete(provider_result)) {
        return incomplete_result();
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "notepad"},
            {"operation", "open-and-write-text"},
            {"path", *provider_result.data.find("path")},
            {"launcherProcessId",
             *provider_result.data.find("launcherProcessId")},
            {"text", *provider_result.data.find("text")},
            {"inputMethod",
             "atomic file write; no keyboard, mouse, clipboard, or focus API"},
            {"foreground",
             components::object({
                 {"unchanged", true},
                 {"nativeIdentifiersExposed", false},
             })},
        }),
    };
}

ModuleResult TextDocumentCompatibilityModule::map_app(
    ModuleResult provider_result,
    const std::string& target_id) const {
    if (!provider_result.ok) {
        return provider_result;
    }
    if (!complete(provider_result)) {
        return incomplete_result();
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "app"},
            {"verb", "create"},
            {"capability", "text.document.create@1"},
            {"data",
             components::object({
                 {"kind", "text-document"},
                 {"state", "created"},
                 {"path", *provider_result.data.find("path")},
                 {"encoding", "utf-8"},
                 {"mediaType", "text/plain"},
                 {"text", *provider_result.data.find("text")},
                 {"foreground",
                  components::object({
                      {"unchanged", true},
                  })},
             })},
            {"meta",
             components::object({
                 {"foreground",
                  components::object({
                      {"unchanged", true},
                  })},
                 {"targeting", "opaque exact session"},
             })},
            {"targetId", target_id},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "provider-neutral-text-artifact-v1"},
        }),
    };
}

}  // namespace act::modules
