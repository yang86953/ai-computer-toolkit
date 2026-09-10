#include "modules/capability_assessment_module.hpp"

#include <array>
#include <string_view>

namespace act::modules {
namespace {

const char* target_kind_name(const TargetKind kind) {
    switch (kind) {
        case TargetKind::host:
            return "host-session";
        case TargetKind::installed_application:
            return "installed-application";
        case TargetKind::running_process:
            return "running-process";
        case TargetKind::application_window:
            return "application-window";
        case TargetKind::standard_edit_control:
            return "standard-edit-control";
    }
    return "unknown";
}

struct CapabilityRule {
    std::string_view id;
    TargetKind target_kind;
    const char* execution_realm;
    bool cpp_available;
    bool requires_confirmation;
    bool requires_foreground_consent;
    const char* scope;
};

constexpr std::array<CapabilityRule, 14> rules{{
    {
        "application.discover@1",
        TargetKind::host,
        "host-headless",
        true,
        false,
        false,
        "installed-and-running-application-inventory",
    },
    {
        "process.discover@1",
        TargetKind::host,
        "host-headless",
        true,
        false,
        false,
        "running-process-inventory",
    },
    {
        "window.discover@1",
        TargetKind::host,
        "host-headless",
        true,
        false,
        false,
        "visible-titled-top-level-windows",
    },
    {
        "process.metadata.read@1",
        TargetKind::running_process,
        "host-headless",
        true,
        false,
        false,
        "public-name-state-relations-and-relative-integrity",
    },
    {
        "accessibility.tree.read@1",
        TargetKind::application_window,
        "isolated-worker",
        true,
        false,
        false,
        "bounded-tree-without-value-text-or-bounds",
    },
    {
        "window.capture.preflight@1",
        TargetKind::application_window,
        "host-headless",
        true,
        false,
        false,
        "metadata-only-no-pixels-no-files-no-activation",
    },
    {
        "window.capture.frame.probe@1",
        TargetKind::application_window,
        "isolated-worker",
        true,
        true,
        false,
        "frame-metadata-only-no-surface-read-no-file-no-activation",
    },
    {
        "application.open@1",
        TargetKind::installed_application,
        "none",
        false,
        true,
        false,
        "rust-compatibility-only",
    },
    {
        "window.screenshot@1",
        TargetKind::application_window,
        "isolated-worker",
        true,
        true,
        false,
        "confirmed-opaque-target",
    },
    {
        "window.record@1",
        TargetKind::application_window,
        "isolated-worker",
        true,
        true,
        false,
        "confirmed-opaque-target-bounded-streaming-artifacts",
    },
    {
        "text.document.create@1",
        TargetKind::installed_application,
        "host-background",
        true,
        true,
        false,
        "atomic-new-utf8-artifact-and-system-notepad-launch",
    },
    {
        "ui.text.input@1",
        TargetKind::standard_edit_control,
        "same-session-no-focus",
        true,
        true,
        false,
        "exact-standard-edit-wm-settext-with-readback",
    },
    {
        "window.close@1",
        TargetKind::application_window,
        "same-session-no-focus",
        true,
        true,
        false,
        "exact-background-window-close-no-foreground-target",
    },
    {
        "ui.input.key@1",
        TargetKind::application_window,
        "none",
        false,
        true,
        true,
        "rust-compatibility-only",
    },
}};

components::Json reasons(std::initializer_list<const char*> values) {
    components::Json::Array result;
    result.reserve(values.size());
    for (const char* value : values) {
        result.emplace_back(value);
    }
    return components::Json(std::move(result));
}

}  // namespace

ModuleResult CapabilityAssessmentModule::assess(
    const std::string& capability,
    const std::string& target_id,
    const TargetKind target_kind,
    const CapabilityAvailability availability) const {
    const CapabilityRule* matching_rule = nullptr;
    for (const auto& rule : rules) {
        if (rule.id == capability) {
            matching_rule = &rule;
            break;
        }
    }

    const char* decision = "capability-gap";
    const char* execution_realm = "none";
    bool requires_confirmation = false;
    bool requires_foreground_consent = false;
    components::Json reason_list =
        reasons({"capability-not-published-in-catalog"});
    const char* scope = "none";
    const char* implementation_state = "not-published";

    if (matching_rule != nullptr) {
        requires_confirmation =
            matching_rule->requires_confirmation;
        requires_foreground_consent =
            matching_rule->requires_foreground_consent;
        scope = matching_rule->scope;
        if (matching_rule->target_kind != target_kind) {
            decision = "unsupported";
            reason_list = reasons(
                {"capability-not-published-for-exact-target-kind"});
            implementation_state = "target-kind-unsupported";
        } else if (
            availability == CapabilityAvailability::permission_blocked) {
            decision = "permission-blocked";
            reason_list =
                reasons({"target-permission-denied-for-read-only-probe"});
            implementation_state = "permission-blocked";
        } else if (
            availability == CapabilityAvailability::unavailable) {
            decision = "unavailable";
            reason_list =
                reasons({"target-read-only-provider-currently-unavailable"});
            implementation_state = "target-unavailable";
        } else if (!matching_rule->cpp_available) {
            decision = "unavailable";
            reason_list =
                reasons({"exact-target-has-no-certified-cpp-route",
                         "legacy-capability-retained-separately"});
            implementation_state =
                "compatibility-mapping-not-yet-certified";
        } else if (matching_rule->requires_confirmation) {
            decision = "confirmation-required";
            execution_realm = matching_rule->execution_realm;
            reason_list = reasons({
                matching_rule->id == "text.document.create@1"
                    ? "explicit-confirmation-required-for-new-artifact"
                    : ((matching_rule->id == "ui.text.input@1" ||
                        matching_rule->id == "window.close@1")
                           ? "explicit-confirmation-required-for-background-mutation"
                           : "explicit-confirmation-required-for-sensitive-read"),
            });
            implementation_state =
                "cpp-available-awaiting-confirmation";
        } else {
            decision = "executable-background";
            execution_realm = matching_rule->execution_realm;
            reason_list = reasons(
                {"read-only-certified-implementation-available"});
            implementation_state = "cpp-available";
        }
    }

    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"ok", true},
            {"contractVersion", "act/control/v1"},
            {"capability", capability},
            {"targetId", target_id},
            {"decision", decision},
            {"executionRealm", execution_realm},
            {"requiresConfirmation", requires_confirmation},
            {"requiresForegroundConsent",
             requires_foreground_consent},
            {"reasons", std::move(reason_list)},
            {"constraints",
             components::object({
                 {"scope", scope},
                 {"readOnly",
                  matching_rule != nullptr &&
                      matching_rule->cpp_available &&
                      matching_rule->id != "window.screenshot@1" &&
                      matching_rule->id != "text.document.create@1" &&
                      matching_rule->id != "ui.text.input@1" &&
                      matching_rule->id != "window.close@1"},
                 {"noFallback", true},
             })},
            {"evidence",
             components::object({
                 {"targetKind", target_kind_name(target_kind)},
                 {"implementationState", implementation_state},
                 {"foregroundActivationAllowed", false},
                 {"inputAllowed", false},
             })},
        }),
    };
}

}  // namespace act::modules
