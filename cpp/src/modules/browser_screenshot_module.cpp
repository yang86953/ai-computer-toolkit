#include "modules/browser_screenshot_module.hpp"

#include "components/json.hpp"
#include "platform/windows/png_file_output.hpp"

namespace act::modules {

ModuleResult BrowserScreenshotModule::status() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"surface", "browser"},
            {"readOnly", true},
            {"runtimeDetected", backend_.runtime_available()},
            {"cppExecutionEnabled", true},
            {"cppStatus", "available-confirmed-isolated"},
            {"backgroundPolicy", "guaranteed"},
            {"foregroundUnchanged", true},
            {"nativeIdentifiersExposed", false},
            {"runtimePathExposed", false},
            {"writesEnabled", true},
            {"isolationRequired", true},
            {"certifiedOperation",
             "isolated-headless-screenshot-cpp"},
        }),
    };
}

ModuleResult BrowserScreenshotModule::capture(
    const std::string& url,
    const std::string& output_path,
    const bool confirmed,
    const bool overwrite,
    const std::uint32_t width,
    const std::uint32_t height,
    const std::uint32_t timeout_ms) const {
    if (!confirmed) {
        return ModuleResult{
            false,
            "CONFIRMATION_REQUIRED",
            "Browser screenshot requires explicit confirmation.",
            nullptr,
        };
    }
    if (url.size() > 8192U ||
        !(url.starts_with("https://") ||
          url.starts_with("http://") ||
          url.starts_with("file://"))) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Browser screenshot accepts a bounded http, https, or file URL.",
            nullptr,
        };
    }
    if (width == 0U || height == 0U ||
        width > 10000U || height > 10000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Browser screenshot width and height must be 1 through 10000.",
            nullptr,
        };
    }
    if (timeout_ms < 1000U || timeout_ms > 300000U) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "Browser screenshot timeoutMs must be 1000 through 300000.",
            nullptr,
        };
    }
    const auto output_plan =
        platform::windows::validate_png_output_path(
            output_path, overwrite);
    if (!output_plan.plan.has_value()) {
        return ModuleResult{
            false,
            output_plan.error_code,
            output_plan.error_message,
            nullptr,
        };
    }
    const auto result = backend_.capture(
        url,
        output_plan.plan->normalized_path,
        overwrite,
        width,
        height,
        timeout_ms);
    if (result.error.has_value()) {
        return ModuleResult{
            false,
            result.error->code,
            result.error->message,
            nullptr,
        };
    }
    const auto& screenshot = *result.screenshot;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"app", "browser"},
            {"operation", "screenshot"},
            {"capability", "browser.screenshot@1"},
            {"path", screenshot.output_path},
            {"bytes", screenshot.bytes},
            {"width", screenshot.width},
            {"height", screenshot.height},
            {"isolatedProfile", true},
            {"atomicOutput", true},
            {"overwriteConfirmed", overwrite},
            {"foreground",
             components::object({
                 {"unchanged", true},
                 {"nativeIdentifiersExposed", false},
             })},
            {"runtimePathExposed", false},
            {"nativeIdentifiersExposed", false},
            {"compatibilityShape",
             "isolated-headless-browser-v1"},
        }),
    };
}

}  // namespace act::modules
