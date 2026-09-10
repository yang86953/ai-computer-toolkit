#include "modules/environment_capability_module.hpp"

#include "components/companion_file.hpp"
#include "components/json.hpp"
#include "platform/windows/environment_capability_backend.hpp"

namespace act::modules {
namespace {

ModuleResult success(components::Json data) {
    return ModuleResult{true, {}, {}, std::move(data)};
}

components::Json common_status(
    const char* surface,
    const bool runtime_detected,
    const bool foreground_unchanged) {
    return components::object({
        {"surface", surface},
        {"readOnly", true},
        {"runtimeDetected", runtime_detected},
        {"cppExecutionEnabled", false},
        {"cppStatus", "rust-compatibility-only"},
        {"backgroundPolicy", "guaranteed"},
        {"foregroundUnchanged", foreground_unchanged},
        {"nativeIdentifiersExposed", false},
        {"runtimePathExposed", false},
        {"writesEnabled", false},
    });
}

}  // namespace

ModuleResult EnvironmentCapabilityModule::status(
    const std::string& surface) const {
    const platform::windows::EnvironmentCapabilityBackend backend;
    const auto facts = backend.inspect();
    if (surface == "browser") {
        auto value = *common_status(
            "browser",
            facts.chromium_runtime_detected,
            facts.foreground_unchanged).object_items();
        value.emplace_back(
            "isolationRequired", true);
        value.emplace_back(
            "certifiedOperation",
            "isolated-headless-screenshot-rust-compatibility");
        return success(components::Json(std::move(value)));
    }
    if (surface == "win32-control") {
        auto value = *common_status(
            "win32-control",
            true,
            facts.foreground_unchanged).object_items();
        value.emplace_back(
            "certifiedBoundary",
            "system-defined-standard-control-message-only");
        return success(components::Json(std::move(value)));
    }
    if (surface == "notepad") {
        auto value = *common_status(
            "notepad",
            facts.notepad_runtime_detected,
            facts.foreground_unchanged).object_items();
        value.emplace_back(
            "certifiedOperation",
            "owned-utf8-document-create-rust-compatibility");
        return success(components::Json(std::move(value)));
    }
    if (surface == "desktop") {
        return success(components::object({
            {"surface", "desktop"},
            {"readOnly", true},
            {"captureWorkerBundled",
             components::companion_file_exists(
                 "ai-computer-toolkit-capture-worker.exe")},
            {"recordingWorkerBundled",
             components::companion_file_exists(
                 "ai-computer-toolkit-recording-worker.exe")},
            {"ffmpegRuntimeDetected",
             facts.ffmpeg_runtime_detected},
            {"screenshotCppStatus", "available-confirmed"},
            {"recordingCppStatus",
             "available-confirmed-bounded-streaming"},
            {"inputCppStatus", "rust-compatibility-only"},
            {"cppExecutionEnabled", true},
            {"foregroundUnchanged", facts.foreground_unchanged},
            {"nativeIdentifiersExposed", false},
            {"runtimePathExposed", false},
            {"writesEnabled", true},
        }));
    }
    return ModuleResult{
        false,
        "INVALID_ARGUMENT",
        "The requested environment surface is unknown.",
        nullptr,
    };
}

}  // namespace act::modules
