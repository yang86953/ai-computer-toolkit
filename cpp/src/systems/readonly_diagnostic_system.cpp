#include "systems/readonly_diagnostic_system.hpp"

#include "components/json.hpp"
#include "modules/application_discovery_module.hpp"
#include "modules/browser_screenshot_module.hpp"
#include "modules/discovery_module.hpp"
#include "modules/environment_capability_module.hpp"
#include "modules/media_session_module.hpp"
#include "modules/standard_edit_module.hpp"

#include <algorithm>
#include <iterator>
#include <string_view>

namespace act::systems {
namespace {

components::Json diagnostic_entry(
    const char* app,
    const char* cpp_status,
    modules::ModuleResult result) {
    if (result.ok) {
        return components::object({
            {"app", app},
            {"ok", true},
            {"readOnly", true},
            {"cppStatus", cpp_status},
            {"diagnostic", std::move(result.data)},
        });
    }
    return components::object({
        {"app", app},
        {"ok", false},
        {"readOnly", true},
        {"cppStatus", cpp_status},
        {"error",
         components::object({
             {"code", result.error_code},
             {"message", result.error_message},
         })},
    });
}

bool selected(
    const std::optional<std::string>& requested,
    const char* app) {
    return !requested.has_value() || *requested == app;
}

}  // namespace

modules::ModuleResult ReadOnlyDiagnosticSystem::doctor(
    const std::optional<std::string>& surface) const {
    constexpr std::string_view known[]{
        "app",
        "uia",
        "window",
        "process",
        "browser",
        "win32-control",
        "notepad",
        "media-session",
        "desktop",
    };
    if (surface.has_value() &&
        std::find(
            std::begin(known),
            std::end(known),
            *surface) == std::end(known)) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "doctor accepts one known surface.",
            nullptr,
        };
    }

    modules::DiscoveryModule discovery;
    modules::ApplicationDiscoveryModule application_discovery;
    modules::BrowserScreenshotModule browser;
    modules::EnvironmentCapabilityModule environment;
    modules::MediaSessionModule media_sessions;
    modules::StandardEditModule standard_edits;
    components::Json::Array results;
    if (selected(surface, "app")) {
        results.push_back(diagnostic_entry(
            "app",
            "available-read-only",
            discovery.status()));
    }
    if (selected(surface, "uia")) {
        results.push_back(diagnostic_entry(
            "uia",
            "available-read-only",
            discovery.status()));
    }
    if (selected(surface, "window")) {
        results.push_back(diagnostic_entry(
            "window",
            "available-read-only",
            discovery.window_status()));
    }
    if (selected(surface, "process")) {
        results.push_back(diagnostic_entry(
            "process",
            "available-read-only",
            application_discovery.process_status()));
    }
    if (selected(surface, "browser")) {
        results.push_back(diagnostic_entry(
            "browser",
            "available-confirmed-isolated",
            browser.status()));
    }
    if (selected(surface, "notepad")) {
        results.push_back(diagnostic_entry(
            "notepad",
            "runtime-observed-execution-rust-compatibility",
            environment.status("notepad")));
    }
    if (selected(surface, "win32-control")) {
        results.push_back(diagnostic_entry(
            "win32-control",
            "available-confirmed-opaque-target",
            standard_edits.status()));
    }
    if (selected(surface, "media-session")) {
        results.push_back(diagnostic_entry(
            "media-session",
            "available-confirmed-opaque-control",
            media_sessions.status(5000U)));
    }
    if (selected(surface, "desktop")) {
        results.push_back(diagnostic_entry(
            "desktop",
            "available-confirmed-capture",
            environment.status("desktop")));
    }
    return modules::ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"policy", "background-preferred"},
            {"cppPolicy", "capability-first-no-silent-fallback"},
            {"readOnly", true},
            {"allCppExecutionAvailable", false},
            {"results", components::Json(std::move(results))},
        }),
    };
}

}  // namespace act::systems
