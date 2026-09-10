#include "act/control_system.hpp"
#include "components/json.hpp"
#include "modules/application_discovery_module.hpp"
#include "modules/capability_assessment_module.hpp"
#include "modules/capability_directory_module.hpp"
#include "modules/discovery_module.hpp"
#include "modules/environment_capability_module.hpp"
#include "modules/media_session_module.hpp"
#include "modules/standard_edit_module.hpp"
#include "systems/application_facade_system.hpp"
#include "systems/browser_command_system.hpp"
#include "systems/media_command_system.hpp"
#include "systems/readonly_diagnostic_system.hpp"
#include "systems/recording_command_system.hpp"
#include "systems/screenshot_command_system.hpp"
#include <algorithm>
#include <cctype>
#include <charconv>
#include <optional>
#include <string_view>
namespace act {
namespace {
CliResult success(components::Json data) {
    return CliResult{
        0,
        components::object({
            {"ok", true},
            {"contractVersion", "act/control/v1"},
            {"implementation", "cpp"},
            {"data", std::move(data)},
        }).dump(),
    };
}
CliResult failure(
    const char* code,
    const std::string& message,
    std::optional<components::Json> details =
        std::nullopt) {
    components::Json::Object error_fields{
        {"code", code},
        {"message", message},
    };
    if (details.has_value()) {
        error_fields.emplace_back(
            "details", std::move(*details));
    }
    return CliResult{
        2,
        components::object({
            {"ok", false},
            {"error", components::Json(
                 std::move(error_fields))},
        }).dump(),
    };
}
CliResult from_module(modules::ModuleResult result) {
    if (result.ok) {
        return success(std::move(result.data));
    }
    return failure(
        result.error_code.c_str(),
        result.error_message,
        std::move(result.error_details));
}
CliResult from_legacy_module(modules::ModuleResult result) {
    if (result.ok) {
        return CliResult{0, result.data.dump()};
    }
    return failure(
        result.error_code.c_str(),
        result.error_message,
        std::move(result.error_details));
}
CliResult from_readonly_module(
    const char* app,
    modules::ModuleResult result) {
    if (!result.ok) {
        return failure(
            result.error_code.c_str(),
            result.error_message,
            std::move(result.error_details));
    }
    const auto* source = result.data.object_items();
    if (source == nullptr) {
        return failure(
            "OPERATION_FAILED",
            "The read-only module returned a non-object result.");
    }
    components::Json original = result.data;
    auto fields = *source;
    if (result.data.find("app") == nullptr) {
        fields.emplace_back("app", app);
    }
    if (result.data.find("ok") == nullptr) {
        fields.emplace_back("ok", true);
    }
    if (result.data.find("readOnly") == nullptr) {
        fields.emplace_back("readOnly", true);
    }
    fields.emplace_back("data", std::move(original));
    return CliResult{
        0, components::Json(std::move(fields)).dump()};
}
bool app_surface(const std::string_view value) {
    return value == "app" || value == "application" || value == "window";
}
bool media_surface(const std::string_view value) {
    return value == "media-session" || value == "media";
}
bool process_surface(const std::string_view value) {
    return value == "process";
}
bool accessibility_surface(const std::string_view value) {
    return value == "uia" || value == "accessibility";
}
bool win32_control_surface(const std::string_view value) {
    return value == "win32-control";
}
std::optional<std::size_t> parse_maximum(
    const std::vector<std::string>& arguments) {
    std::size_t maximum = 100;
    for (std::size_t index = 2; index < arguments.size(); ++index) {
        if (arguments[index] != "--max-items") {
            continue;
        }
        if (index + 1U >= arguments.size()) {
            return std::nullopt;
        }
        const std::string& value = arguments[++index];
        const auto result = std::from_chars(
            value.data(), value.data() + value.size(), maximum);
        if (result.ec != std::errc{} ||
            result.ptr != value.data() + value.size() ||
            maximum == 0U ||
            maximum > 4096U) {
            return std::nullopt;
        }
    }
    return maximum;
}
std::optional<std::size_t> parse_limit(
    const std::vector<std::string>& arguments,
    const std::string_view flag,
    const std::size_t default_value,
    const std::size_t upper_bound = 4096U) {
    std::size_t value = default_value;
    for (std::size_t index = 2; index < arguments.size(); ++index) {
        if (arguments[index] != flag) {
            continue;
        }
        if (index + 1U >= arguments.size()) {
            return std::nullopt;
        }
        const std::string& raw = arguments[++index];
        const auto result =
            std::from_chars(raw.data(), raw.data() + raw.size(), value);
        if (result.ec != std::errc{} ||
            result.ptr != raw.data() + raw.size() ||
            value == 0U ||
            value > upper_bound) {
            return std::nullopt;
        }
    }
    return value;
}
std::optional<std::size_t> parse_nonnegative_limit(
    const std::vector<std::string>& arguments,
    const std::string_view flag,
    const std::size_t default_value,
    const std::size_t upper_bound) {
    std::size_t value = default_value;
    for (std::size_t index = 2; index < arguments.size(); ++index) {
        if (arguments[index] != flag) {
            continue;
        }
        if (index + 1U >= arguments.size()) {
            return std::nullopt;
        }
        const std::string& raw = arguments[++index];
        const auto result =
            std::from_chars(raw.data(), raw.data() + raw.size(), value);
        if (result.ec != std::errc{} ||
            result.ptr != raw.data() + raw.size() ||
            value > upper_bound) {
            return std::nullopt;
        }
    }
    return value;
}
std::optional<std::string> parse_option(
    const std::vector<std::string>& arguments,
    const std::string_view flag) {
    for (std::size_t index = 2; index < arguments.size(); ++index) {
        if (arguments[index] == flag &&
            index + 1U < arguments.size()) {
            return arguments[index + 1U];
        }
    }
    return std::nullopt;
}

bool has_flag(
    const std::vector<std::string>& arguments,
    const std::string_view flag) {
    return std::find(arguments.begin(), arguments.end(), flag) !=
           arguments.end();
}

bool valid_capability_id(const std::string_view value) {
    if (value.empty() ||
        value.front() < 'a' ||
        value.front() > 'z') {
        return false;
    }
    const std::size_t at = value.find('@');
    if (at == std::string_view::npos ||
        at == 0U ||
        at + 1U >= value.size() ||
        value.find('@', at + 1U) != std::string_view::npos) {
        return false;
    }
    for (std::size_t index = 0; index < at; ++index) {
        const unsigned char character =
            static_cast<unsigned char>(value[index]);
        if (!(std::islower(character) != 0 ||
              std::isdigit(character) != 0 ||
              character == '.' ||
              character == '-')) {
            return false;
        }
    }
    for (std::size_t index = at + 1U; index < value.size(); ++index) {
        if (std::isdigit(
                static_cast<unsigned char>(value[index])) == 0) {
            return false;
        }
    }
    return value[at + 1U] != '0';
}
std::optional<std::string> parse_session_id(
    const std::vector<std::string>& arguments) {
    for (std::size_t index = 2; index < arguments.size(); ++index) {
        if (arguments[index] == "--target" &&
            index + 1U < arguments.size()) {
            const std::string& target = arguments[index + 1U];
            constexpr std::string_view prefix = "sessionId=";
            if (target.starts_with(prefix)) {
                return target.substr(prefix.size());
            }
        }
        constexpr std::string_view prefix = "sessionId=";
        if (arguments[index].starts_with(prefix)) {
            return arguments[index].substr(prefix.size());
        }
    }
    return std::nullopt;
}
}  // namespace
class ComputerControlSystem::Impl final {
public:
    modules::DiscoveryModule discovery;
    modules::ApplicationDiscoveryModule application_discovery;
    modules::CapabilityAssessmentModule assessment;
    modules::CapabilityDirectoryModule directory;
    modules::EnvironmentCapabilityModule environment;
    modules::MediaSessionModule media_sessions;
    modules::StandardEditModule standard_edits;
    systems::ApplicationFacadeSystem app_facade;
    systems::BrowserCommandSystem browser;
    systems::ReadOnlyDiagnosticSystem diagnostics;
    systems::RecordingCommandSystem recordings;
    systems::ScreenshotCommandSystem screenshots;
};
ComputerControlSystem::ComputerControlSystem()
    : impl_(std::make_unique<Impl>()) {}
ComputerControlSystem::~ComputerControlSystem() = default;
ComputerControlSystem::ComputerControlSystem(
    ComputerControlSystem&&) noexcept = default;
ComputerControlSystem& ComputerControlSystem::operator=(
    ComputerControlSystem&&) noexcept = default;
CliResult ComputerControlSystem::execute(
    const std::vector<std::string>& arguments) {
    if (arguments.empty()) {
        return failure(
            "INVALID_ARGUMENT",
            "A command is required.");
    }
    const std::string& command = arguments[0];
    if (command == "help") {
        if (arguments.size() != 1U) {
            return failure(
                "INVALID_ARGUMENT",
                "help does not accept positional arguments.");
        }
        return from_legacy_module(impl_->directory.help());
    }
    if (command == "version") {
        if (arguments.size() != 1U) {
            return failure(
                "INVALID_ARGUMENT",
                "version does not accept positional arguments.");
        }
        return from_legacy_module(impl_->directory.version());
    }
    if (command == "build-info") {
        if (arguments.size() != 1U) {
            return failure(
                "INVALID_ARGUMENT",
                "build-info does not accept positional arguments.");
        }
        return from_module(impl_->directory.build_info());
    }
    if (command == "methods") {
        const std::optional<std::string> method_id =
            arguments.size() > 1U
                ? std::optional<std::string>(arguments[1])
                : std::nullopt;
        return from_legacy_module(impl_->directory.methods(method_id));
    }
    if (command == "describe") {
        if (arguments.size() < 2U ||
            arguments.size() > 3U) {
            return failure(
                "INVALID_ARGUMENT",
                "describe requires <app> [operation].");
        }
        const std::optional<std::string> operation =
            arguments.size() == 3U
                ? std::optional<std::string>(arguments[2])
                : std::nullopt;
        return from_legacy_module(
            impl_->directory.describe(arguments[1], operation));
    }
    if (command == "doctor") {
        if (arguments.size() > 2U) {
            return failure(
                "INVALID_ARGUMENT",
                "doctor accepts one known surface.");
        }
        const std::optional<std::string> requested =
            arguments.size() > 1U
                ? std::optional<std::string>(arguments[1])
                : std::nullopt;
        return from_legacy_module(
            impl_->diagnostics.doctor(requested));
    }
    if (command == "catalog") {
        if (arguments.size() > 2U) {
            return failure(
                "INVALID_ARGUMENT",
                "catalog accepts at most one application ID.");
        }
        const std::optional<std::string> app_id =
            arguments.size() == 2U
                ? std::optional<std::string>(arguments[1])
                : std::nullopt;
        return from_legacy_module(impl_->directory.catalog(app_id));
    }
    if (command == "capabilities") {
        if (arguments.size() >= 2U &&
            arguments[1] == "method") {
            if (arguments.size() > 3U) {
                return failure(
                    "INVALID_ARGUMENT",
                    "capabilities method accepts at most one method ID.");
            }
            const std::optional<std::string> method_id =
                arguments.size() == 3U
                    ? std::optional<std::string>(arguments[2])
                    : std::nullopt;
            return from_module(
                impl_->directory.method_capabilities(method_id));
        }
        if (arguments.size() >= 2U &&
            arguments[1] == "descriptor") {
            if (arguments.size() < 3U ||
                arguments.size() > 4U) {
                return failure(
                    "INVALID_ARGUMENT",
                    "capabilities descriptor requires <app> "
                    "[operation].");
            }
            const std::optional<std::string> operation =
                arguments.size() == 4U
                    ? std::optional<std::string>(arguments[3])
                    : std::nullopt;
            return from_module(
                impl_->directory.descriptor_capabilities(
                    arguments[2], operation));
        }
        if (arguments.size() > 2U ||
            (arguments.size() == 2U &&
             !app_surface(arguments[1]) &&
             !media_surface(arguments[1]) &&
             !process_surface(arguments[1]) &&
             !accessibility_surface(arguments[1]))) {
            return failure(
                "INVALID_ARGUMENT",
                "capabilities accepts one migrated C++ surface.");
        }
        return from_module(impl_->directory.capabilities());
    }
    if (command == "status") {
        if (arguments.size() == 2U && arguments[1] == "browser") {
            return from_readonly_module(
                "browser", impl_->browser.status());
        }
        if (arguments.size() == 2U &&
            win32_control_surface(arguments[1])) {
            return from_readonly_module(
                "win32-control",
                impl_->standard_edits.status());
        }
        if (arguments.size() == 2U &&
            (arguments[1] == "notepad" ||
             arguments[1] == "desktop")) {
            return from_readonly_module(
                arguments[1].c_str(),
                impl_->environment.status(arguments[1]));
        }
        if (arguments.size() >= 2U &&
            process_surface(arguments[1])) {
            return from_readonly_module(
                "process",
                impl_->application_discovery.process_status());
        }
        if (arguments.size() == 2U &&
            media_surface(arguments[1])) {
            return from_readonly_module(
                "media-session",
                impl_->media_sessions.status(5000U));
        }
        if (arguments.size() >= 2U &&
            arguments[1] == "window") {
            return from_readonly_module(
                "window", impl_->discovery.window_status());
        }
        if (arguments.size() >= 2U &&
            accessibility_surface(arguments[1])) {
            return from_readonly_module(
                "uia", impl_->discovery.status());
        }
        if (arguments.size() < 2U || !app_surface(arguments[1])) {
            return failure(
                "CAPABILITY_GAP",
                "The requested surface is not in the C++ capability catalog.");
        }
        return from_readonly_module(
            "app", impl_->app_facade.status());
    }
    if (command == "sessions") {
        if (arguments.size() >= 2U &&
            win32_control_surface(arguments[1])) {
            const auto maximum = parse_maximum(arguments);
            if (!maximum.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "--max-items must be an integer from 1 through 4096.");
            }
            return from_readonly_module(
                "win32-control",
                impl_->standard_edits.sessions(*maximum));
        }
        if (arguments.size() >= 2U &&
            process_surface(arguments[1])) {
            const auto maximum = parse_maximum(arguments);
            if (!maximum.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "--max-items must be an integer from 1 through 4096.");
            }
            return from_readonly_module(
                "process",
                impl_->application_discovery.process_sessions(
                    *maximum));
        }
        if (arguments.size() >= 2U &&
            media_surface(arguments[1])) {
            const auto maximum = parse_limit(
                arguments, "--max-items", 100U, 128U);
            const auto timeout_ms = parse_limit(
                arguments, "--timeout-ms", 5000U, 30000U);
            if (!maximum.has_value() ||
                !timeout_ms.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "Media sessions require max-items 1..128 and "
                    "timeout-ms 1..30000.");
            }
            return from_readonly_module(
                "media-session",
                impl_->media_sessions.sessions(
                *maximum,
                static_cast<std::uint32_t>(*timeout_ms)));
        }
        if (arguments.size() >= 2U &&
            accessibility_surface(arguments[1])) {
            const auto maximum = parse_maximum(arguments);
            if (!maximum.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "--max-items must be an integer from 1 through 4096.");
            }
            return from_readonly_module(
                "uia",
                impl_->discovery.sessions(*maximum));
        }
        if (arguments.size() < 2U || !app_surface(arguments[1])) {
            return failure(
                "CAPABILITY_GAP",
                "The requested surface is not in the C++ capability catalog.");
        }
        const auto maximum = parse_maximum(arguments);
        if (!maximum.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "--max-items must be an integer from 1 through 4096.");
        }
        if (arguments[1] == "window") {
            return from_readonly_module(
                "window", impl_->discovery.sessions(*maximum));
        }
        return from_readonly_module(
            "app", impl_->app_facade.sessions(*maximum));
    }
    if (command == "inspect") {
        if (arguments.size() >= 2U &&
            win32_control_surface(arguments[1])) {
            const auto session_id =
                parse_session_id(arguments);
            if (!session_id.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "Standard Edit inspect requires --target "
                    "sessionId=<opaque>.");
            }
            return from_readonly_module(
                "win32-control",
                impl_->standard_edits.inspect(*session_id));
        }
        if (arguments.size() >= 2U &&
            process_surface(arguments[1])) {
            const auto session_id = parse_session_id(arguments);
            if (!session_id.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "Process inspect requires --target "
                    "sessionId=<opaque>.");
            }
            return from_readonly_module(
                "process",
                impl_->application_discovery.inspect_process(
                    *session_id));
        }
        if (arguments.size() >= 2U &&
            arguments[1] == "window") {
            const auto session_id = parse_session_id(arguments);
            if (!session_id.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "Window inspect requires --target "
                    "sessionId=<opaque>.");
            }
            return from_readonly_module(
                "window",
                impl_->discovery.inspect_window(*session_id));
        }
        if (arguments.size() >= 2U &&
            media_surface(arguments[1])) {
            const auto session_id = parse_session_id(arguments);
            const auto timeout_ms = parse_limit(
                arguments, "--timeout-ms", 5000U, 30000U);
            if (!session_id.has_value() ||
                !timeout_ms.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "Media inspect requires --target "
                    "sessionId=<opaque> and timeout-ms 1..30000.");
            }
            return from_readonly_module(
                "media-session",
                impl_->media_sessions.inspect(
                *session_id,
                static_cast<std::uint32_t>(*timeout_ms)));
        }
        if (arguments.size() >= 2U &&
            accessibility_surface(arguments[1])) {
            const auto session_id = parse_session_id(arguments);
            const auto timeout_ms = parse_limit(
                arguments, "--timeout-ms", 5000U, 30000U);
            if (!session_id.has_value() ||
                !timeout_ms.has_value()) {
                return failure(
                    "INVALID_ARGUMENT",
                    "UIA inspect requires --target "
                    "sessionId=<opaque> and timeout-ms 1..30000.");
            }
            return from_readonly_module(
                "uia",
                impl_->discovery.inspect(
                *session_id,
                static_cast<std::uint32_t>(*timeout_ms)));
        }
        if (arguments.size() < 2U || !app_surface(arguments[1])) {
            return failure(
                "CAPABILITY_GAP",
                "The requested surface is not in the C++ capability catalog.");
        }
        const auto session_id = parse_session_id(arguments);
        if (!session_id.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "inspect requires --target sessionId=<opaque>.");
        }
        const auto timeout_ms = parse_limit(
            arguments, "--timeout-ms", 5000U, 30000U);
        if (!timeout_ms.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "--timeout-ms must be an integer from 1 through 30000.");
        }
        return from_readonly_module(
            "app",
            impl_->app_facade.inspect(
                *session_id,
                static_cast<std::uint32_t>(*timeout_ms)));
    }
    if (command == "inspect-tree") {
        if (arguments.size() < 2U ||
            (!app_surface(arguments[1]) &&
             !accessibility_surface(arguments[1]))) {
            return failure(
                "CAPABILITY_GAP",
                "The requested surface is not in the C++ capability catalog.");
        }
        const auto session_id = parse_session_id(arguments);
        const auto maximum_depth = parse_nonnegative_limit(
            arguments, "--max-depth", 3U, 20U);
        const auto maximum_items = parse_limit(
            arguments, "--max-items", 100U);
        const std::string view =
            parse_option(arguments, "--view").value_or("control");
        const auto timeout_ms = parse_limit(
            arguments, "--timeout-ms", 5000U, 30000U);
        if (!session_id.has_value() ||
            !maximum_depth.has_value() ||
            !maximum_items.has_value() ||
            !timeout_ms.has_value() ||
            (view != "control" && view != "raw")) {
            return failure(
                "INVALID_ARGUMENT",
                "inspect-tree requires an exact session, max-depth 0..20, "
                "max-items 1..4096, timeout-ms 1..30000, and control or "
                "raw view.");
        }
        return from_module(impl_->discovery.inspect_tree(
            *session_id,
            *maximum_depth,
            *maximum_items,
            view,
            static_cast<std::uint32_t>(*timeout_ms)));
    }
    if (command == "preflight-capture") {
        if (arguments.size() < 2U || !app_surface(arguments[1])) {
            return failure(
                "CAPABILITY_GAP",
                "Capture preflight is only published for application "
                "windows.");
        }
        const auto session_id = parse_session_id(arguments);
        if (!session_id.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "preflight-capture requires --target sessionId=<opaque>.");
        }
        return from_module(
            impl_->discovery.capture_preflight(*session_id));
    }
    if (command == "probe-capture-frame") {
        if (arguments.size() < 2U || !app_surface(arguments[1])) {
            return failure(
                "CAPABILITY_GAP",
                "Capture frame probe is only published for application "
                "windows.");
        }
        if (!has_flag(arguments, "--confirm")) {
            return failure(
                "CONFIRMATION_REQUIRED",
                "Capture frame probe requires explicit --confirm before "
                "target resolution or frame acquisition.");
        }
        const auto session_id = parse_session_id(arguments);
        const auto timeout_ms = parse_limit(
            arguments, "--timeout-ms", 5000U, 30000U);
        if (!session_id.has_value() || !timeout_ms.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "probe-capture-frame requires --target "
                "sessionId=<opaque>, --confirm, and timeout-ms 1..30000.");
        }
        return from_module(impl_->discovery.capture_frame_probe(
            *session_id,
            static_cast<std::uint32_t>(*timeout_ms)));
    }
    if (command == "discover") {
        if (arguments.size() < 2U ||
            (arguments[1] != "app" &&
             arguments[1] != "application")) {
            return failure(
                "CAPABILITY_GAP",
                "Only the generic application inventory is migrated.");
        }
        const auto maximum_applications = parse_limit(
            arguments, "--max-applications", 512U);
        const auto maximum_processes = parse_limit(
            arguments, "--max-processes", 512U);
        const auto maximum_windows = parse_limit(
            arguments, "--max-windows", 512U);
        if (!maximum_applications.has_value() ||
            !maximum_processes.has_value() ||
            !maximum_windows.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "Discovery limits must be integers from 1 through 4096.");
        }
        return from_module(impl_->application_discovery.discover(
            *maximum_applications,
            *maximum_processes,
            *maximum_windows));
    }
    if (command == "assess") {
        if (arguments.size() < 2U ||
            (arguments[1] != "app" &&
             arguments[1] != "application")) {
            return failure(
                "CAPABILITY_GAP",
                "Only the generic application assessment surface is "
                "migrated.");
        }
        const auto capability =
            parse_option(arguments, "--capability");
        const auto session_id = parse_session_id(arguments);
        if (!capability.has_value() ||
            !valid_capability_id(*capability) ||
            !session_id.has_value()) {
            return failure(
                "INVALID_ARGUMENT",
                "assess requires a versioned --capability and "
                "--target sessionId=<opaque>.");
        }
        if (impl_->app_facade.owns_text_document_session(
                *session_id)) {
            auto result = impl_->assessment.assess(
                *capability,
                *session_id,
                modules::TargetKind::installed_application,
                modules::CapabilityAvailability::available);
            return result.ok
                ? CliResult{0, result.data.dump()}
                : failure(
                      result.error_code.c_str(),
                      result.error_message);
        }
        if (session_id->starts_with("s2:c:")) {
            auto result = impl_->app_facade.assess_standard_edit(
                *capability, *session_id);
            return result.ok ? CliResult{0, result.data.dump()}
                             : failure(result.error_code.c_str(),
                                       result.error_message);
        }
        const auto snapshot = impl_->application_discovery.capture(
            4096U, 4096U, 4096U);
        if (!snapshot.foreground_unchanged) {
            return failure(
                "HOST_INTERFERENCE_DETECTED",
                "The foreground target changed during capability "
                "assessment.");
        }
        // 跨清单解析 opaque 目标并保留多命中状态。
        const auto target_resolution = impl_->application_discovery.resolve_target(
            snapshot, *session_id);
        // 多命中时不得评估任意候选目标。
        if (target_resolution.state == components::OpaqueTargetMatchState::ambiguous) {
            // 返回稳定的跨实现歧义错误。
            return failure("AMBIGUOUS_TARGET",
                "The opaque target resolves to multiple current inventory records.");
        }
        // 零命中保持现有过期会话语义。
        if (target_resolution.state == components::OpaqueTargetMatchState::missing ||
            !target_resolution.target_kind.has_value()) {
            return failure("STALE_SESSION",
                "The opaque target no longer resolves in the current "
                "read-only inventory.");
        }
        // 唯一命中后才读取目标种类。
        const auto target_kind = *target_resolution.target_kind;
        modules::CapabilityAvailability availability = modules::CapabilityAvailability::available;
        if (*capability == "accessibility.tree.read@1" && target_kind == modules::TargetKind::application_window) {
            const auto probe = impl_->discovery.inspect(*session_id);
            if (!probe.ok) {
                if (probe.error_code == "HOST_INTERFERENCE_DETECTED" ||
                    probe.error_code == "STALE_SESSION") {
                    return failure(
                        probe.error_code.c_str(), probe.error_message);
                }
                availability =
                    probe.error_code == "PERMISSION_DENIED"
                        ? modules::CapabilityAvailability::permission_blocked
                        : modules::CapabilityAvailability::unavailable;
            }
        } else if (*capability == "process.metadata.read@1" &&
            target_kind == modules::TargetKind::running_process) {
            // 唯一运行进程解析必须携带同一快照内的记录。
            if (target_resolution.process == nullptr) {
                return failure("STALE_SESSION",
                    "The running process target became stale during "
                    "assessment.");
            }
            // 唯一命中后才读取进程元数据可用性。
            const auto* process = target_resolution.process;
            using Access = platform::windows::ProcessMetadataAccess;
            availability =
                process->metadata_access == Access::available
                    ? modules::CapabilityAvailability::available
                    : (process->metadata_access ==
                               Access::permission_blocked
                           ? modules::CapabilityAvailability::
                                 permission_blocked
                           : modules::CapabilityAvailability::unavailable);
        }
        auto result = impl_->assessment.assess(*capability, *session_id, target_kind,
            availability);
        if (!result.ok) {
            return failure(result.error_code.c_str(), result.error_message);
        }
        return CliResult{0, result.data.dump()};
    }
    if (command == "run" && arguments.size() >= 3U &&
        arguments[1] == "browser" &&
        arguments[2] == "screenshot") {
        return from_legacy_module(impl_->browser.run(arguments));
    }
    if (command == "run" && arguments.size() >= 3U &&
        media_surface(arguments[1])) {
        return from_legacy_module(systems::MediaCommandSystem().run(arguments));
    }
    if (command == "run" &&
        arguments.size() >= 3U &&
        arguments[1] == "app" &&
        arguments[2] == "close") {
        return from_legacy_module(impl_->app_facade.run_app_close(arguments));
    }
    if (command == "run" &&
        arguments.size() >= 3U &&
        arguments[1] == "app" &&
        arguments[2] == "create") {
        return from_legacy_module(
            impl_->app_facade.run_app_create(arguments));
    }
    if (command == "run" &&
        arguments.size() >= 3U &&
        arguments[1] == "notepad" &&
        arguments[2] == "open-and-write-text") {
        return from_legacy_module(
            impl_->app_facade.run_legacy_notepad(arguments));
    }
    if (command == "run" &&
        arguments.size() >= 3U &&
        arguments[1] == "app" &&
        arguments[2] == "apply") {
        return from_legacy_module(
            impl_->app_facade.run_app_apply(arguments));
    }
    if (command == "run" && arguments.size() >= 3U &&
        ((arguments[1] == "win32-control" && arguments[2] == "set-text") ||
         (arguments[1] == "desktop" && arguments[2] == "type-text"))) {
        return from_legacy_module(arguments[1] == "desktop"
            ? impl_->app_facade.run_legacy_type_text(arguments)
            : impl_->app_facade.run_legacy_standard_edit(arguments));
    }
    if (command == "run" && arguments.size() >= 3U &&
        (arguments[1] == "app" || arguments[1] == "desktop") &&
        arguments[2] == "screenshot") {
        return from_legacy_module(
            arguments[1] == "app" ? impl_->screenshots.run_app(arguments)
                : impl_->screenshots.run_desktop(arguments));
    }
    if (command == "run" && arguments.size() >= 3U &&
        (arguments[1] == "app" || arguments[1] == "desktop") &&
        arguments[2] == "record") {
        return from_legacy_module(
            arguments[1] == "app" ? impl_->recordings.run_app(arguments)
                : impl_->recordings.run_desktop(arguments));
    }
    return failure(
        "CAPABILITY_GAP",
        "The requested command is not in the migrated C++ capability catalog.");
}

}  // namespace act
