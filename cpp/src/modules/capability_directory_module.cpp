#include "modules/capability_directory_module.hpp"

#include "components/json.hpp"
#include "components/companion_file.hpp"

#include <algorithm>
#include <array>
#include <string_view>

namespace act::modules {
namespace {

struct Method {
    const char* id;
    const char* execution_scope;
    const char* availability;
    const char* summary;
    const char* cpp_status;
    const char* safety_boundary;
};

constexpr std::array<Method, 13> directory_methods{{
    {
        "app-api",
        "background",
        "extension-point",
        "应用公开的 HTTP、SDK 或本地 API；按应用专用适配器注册。",
        "extension-point",
        "named-versioned-adapter-only",
    },
    {
        "command-line",
        "background",
        "extension-point",
        "应用公开 CLI；固定程序与参数模型后注册，不经 shell 拼接。",
        "extension-point",
        "fixed-program-and-argument-schema",
    },
    {
        "com-automation",
        "background",
        "native-provider",
        "已注册在版本化 app capability 后的 attach-only COM；公共输入不接受 "
        "ProgID、COM member 或任意脚本。",
        "rust-compatibility-only",
        "no-progid-member-or-script-in-public-input",
    },
    {
        "ipc",
        "background",
        "extension-point",
        "已公开的命名管道、RPC 或 App Service 协议；禁止猜测消息格式。",
        "extension-point",
        "published-protocol-only",
    },
    {
        "media-session",
        "background",
        "native",
        "Windows GSMTC 系统媒体会话；支持播放、暂停、切歌与状态读取。",
        "available-confirmed",
        "opaque-session-control-requires-confirmation",
    },
    {
        "cdp",
        "background",
        "extension-point",
        "Chrome DevTools Protocol；仅连接显式启用的远程调试端口。",
        "extension-point",
        "explicit-debug-endpoint-only",
    },
    {
        "uia",
        "provider-dependent",
        "read-native",
        "UI Automation 控制模式；仅在应用实际暴露并认证后可写入。",
        "available-read-only",
        "no-value-text-bounds-or-write-patterns",
    },
    {
        "win32-message",
        "background",
        "native",
        "系统定义的标准控件消息；当前认证标准 Edit 的 WM_SETTEXT。",
        "available-confirmed",
        "no-arbitrary-message-or-pointer-protocol",
    },
    {
        "windows-graphics-capture",
        "background",
        "native",
        "Windows Graphics Capture 精确窗口帧；不激活窗口，支持被遮挡窗口，"
        "最小化窗口无新帧。",
        "available-confirmed",
        "confirmation-opaque-target-no-activation",
    },
    {
        "ffmpeg-h264",
        "background",
        "external-runtime",
        "固定参数的 ffmpeg libx264 编码器；默认 2fps、960px、CRF 32，"
        "不接受任意命令参数。",
        "available-confirmed",
        "no-arbitrary-encoder-command",
    },
    {
        "headless-browser",
        "background",
        "native",
        "隔离 profile 的 headless Chromium 操作。",
        "available-confirmed",
        "isolated-profile-fixed-arguments-no-user-browser-attachment",
    },
    {
        "file-automation",
        "background",
        "native",
        "应用支持的文档或配置文件自动化；必须验证对象所有权与回读。",
        "rust-compatibility-only",
        "ownership-validation-and-readback-required",
    },
    {
        "foreground-input",
        "foreground",
        "native-consent-gated",
        "精确窗口的恢复、激活和 SendInput；只在任务授权后作为最后回退。",
        "rust-compatibility-only",
        "confirmation-and-foreground-consent-required",
    },
}};

components::Json method_json(const Method& method) {
    return components::object({
        {"id", method.id},
        {"executionScope", method.execution_scope},
        {"availability", method.availability},
        {"summary", method.summary},
        {"cppStatus", method.cpp_status},
        {"safetyBoundary", method.safety_boundary},
    });
}

components::Json legacy_method_json(const Method& method) {
    return components::object({
        {"availability", method.availability},
        {"executionScope", method.execution_scope},
        {"id", method.id},
        {"summary", method.summary},
    });
}

ModuleResult directory_failure(const char* message) {
    return ModuleResult{
        false, "OPERATION_FAILED", message, nullptr};
}

std::optional<components::Json> load_legacy_catalog(
    std::string& message) {
    const auto text = components::read_companion_text(
        "legacy-public-catalog-v1.json");
    if (!text.has_value()) {
        message =
            "The legacy public catalog companion is unavailable.";
        return std::nullopt;
    }
    std::string parse_error;
    auto catalog = components::Json::parse(*text, parse_error);
    const auto* contract =
        catalog.has_value()
            ? catalog->find("contractVersion")
            : nullptr;
    const auto* policy =
        catalog.has_value() ? catalog->find("policy") : nullptr;
    const auto* apps =
        catalog.has_value() ? catalog->find("apps") : nullptr;
    if (!catalog.has_value() ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() !=
            "act/legacy-public-catalog/v1" ||
        policy == nullptr ||
        policy->string_value() == nullptr ||
        apps == nullptr ||
        apps->array_items() == nullptr) {
        message = "The legacy public catalog companion is invalid.";
        return std::nullopt;
    }
    return catalog;
}

components::Json with_status(
    const components::Json& descriptor,
    const bool operation) {
    auto fields = *descriptor.object_items();
    const auto* id = descriptor.find("id");
    const bool cpp_screenshot =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        (*id->string_value() == "desktop.screenshot" ||
         *id->string_value() == "app.screenshot");
    const bool cpp_text_create =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        (*id->string_value() == "app.create" ||
         *id->string_value() ==
             "notepad.open-and-write-text");
    const bool cpp_standard_edit =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        (*id->string_value() == "app.apply" ||
         *id->string_value() ==
              "win32-control.set-text");
    const bool cpp_desktop_type_text =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        *id->string_value() == "desktop.type-text";
    const bool cpp_window_close =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        *id->string_value() == "app.close";
    const bool cpp_browser =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        *id->string_value() == "browser.screenshot";
    const bool cpp_recording =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        (*id->string_value() == "desktop.record" ||
         *id->string_value() == "app.record");
    const bool cpp_media_control =
        operation &&
        id != nullptr &&
        id->string_value() != nullptr &&
        id->string_value()->starts_with("media-session.");
    const bool cpp_execution =
        cpp_screenshot || cpp_text_create ||
        cpp_standard_edit || cpp_window_close ||
        cpp_browser || cpp_recording ||
        cpp_media_control || cpp_desktop_type_text;
    const char* cpp_status =
        operation
            ? "rust-compatibility-only"
            : "compatibility-descriptor";
    if (cpp_screenshot || cpp_standard_edit ||
        cpp_desktop_type_text ||
        cpp_window_close) {
        cpp_status =
            "available-confirmed-opaque-target";
    } else if (cpp_text_create) {
        cpp_status =
            *id->string_value() == "app.create"
                ? "available-confirmed-opaque-target"
                : "available-confirmed";
    } else if (cpp_browser) {
        cpp_status = "available-confirmed-isolated";
    } else if (cpp_recording) {
        cpp_status =
            "available-confirmed-opaque-target";
    } else if (cpp_media_control) {
        cpp_status =
            "available-confirmed-opaque-target";
    }
    fields.emplace_back(
        operation ? "cppStatus" : "cppDirectoryStatus",
        cpp_status);
    if (operation) {
        fields.emplace_back(
            "cppExecutionEnabled", cpp_execution);
    }
    return components::Json(std::move(fields));
}

}  // namespace

ModuleResult CapabilityDirectoryModule::version() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"name", "ai-computer-toolkit"},
            {"ok", true},
            {"version", "0.1.0"},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::build_info() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"name", "ai-computer-toolkit"},
            {"version", "0.16.0-cpp-background-type-text"},
            {"mainLanguage", "C++23"},
            {"compatibilityVersion", "0.1.0"},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::help() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"name", "ai-computer-toolkit"},
            {"ok", true},
            {"policy",
             "background preferred; foreground input requires explicit "
             "--allow-foreground consent"},
            {"stdout",
             "JSON only; failures are structured JSON with a non-zero "
             "exit code."},
            {"usage",
             components::array({
                 // 保持 Rust/C++ 构建元数据帮助入口一致。
                 "ai-computer-toolkit build-info [--pretty]",
                 // 保持 Rust/C++ capability 元数据帮助入口一致。
                 "ai-computer-toolkit capabilities "
                 "[surface|method|descriptor] [args] [--pretty]",
                 "ai-computer-toolkit catalog [app] [--pretty]",
                 "ai-computer-toolkit methods [method] [--pretty]",
                 "ai-computer-toolkit describe <app> [operation] "
                 "[--pretty]",
                 "ai-computer-toolkit doctor [app] [--pretty]",
                 "ai-computer-toolkit status [app] [--pretty]",
                 "ai-computer-toolkit sessions <app> "
                 "[--target key=value] [--max-items n] [--pretty]",
                 "ai-computer-toolkit inspect <app> "
                 "--target sessionId=<id> [--max-depth n] "
                 "[--max-items n] [--pretty]",
                 "ai-computer-toolkit run <app> <operation> "
                 "--target key=value --arg key=value --confirm "
                 "[--allow-foreground] [--pretty]",
                 "ai-computer-toolkit sequence --input <file|-> "
                 "[--pretty]",
             })},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::catalog(
    const std::optional<std::string>& app_id) const {
    std::string message;
    auto catalog = load_legacy_catalog(message);
    if (!catalog.has_value()) {
        return directory_failure(message.c_str());
    }
    const auto* policy = catalog->find("policy");
    const auto* apps = catalog->find("apps")->array_items();
    components::Json::Array selected;
    if (app_id.has_value()) {
        const auto found = std::find_if(
            apps->begin(),
            apps->end(),
            [&app_id](const auto& value) {
                const auto* id = value.find("id");
                return id != nullptr &&
                       id->string_value() != nullptr &&
                       *id->string_value() == *app_id;
            });
        if (found == apps->end()) {
            return ModuleResult{
                false,
                "INVALID_ARGUMENT",
                "The requested application descriptor is unknown.",
                nullptr,
            };
        }
        selected.push_back(*found);
    } else {
        selected = *apps;
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"apps", components::Json(std::move(selected))},
            {"ok", true},
            {"policy", *policy},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::capabilities() const {
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"surface", "app"},
            {"productPromise",
             "broad-general-control-with-capability-degradation"},
            {"supportLevel", "L2-confirmed-background"},
            {"capabilities",
             components::array({
                 components::object({
                     {"id", "application.discover@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "partial-installed-plus-running-inventory"},
                 }),
                 components::object({
                     {"id", "process.discover@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "running-processes-including-no-window"},
                 }),
                 components::object({
                     {"id", "window.discover@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "visible-titled-top-level-windows"},
                 }),
                 components::object({
                     {"id", "window.metadata.read@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "opaque-id-title-application-and-visibility-only"},
                 }),
                 components::object({
                     {"id", "process.metadata.read@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "no-native-id-path-token-or-sid-exposure"},
                 }),
                 components::object({
                     {"id", "accessibility.tree.read@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "isolated-worker"},
                     {"constraint",
                      "same-session-bounded-tree-without-value-text-or-bounds"},
                 }),
                 components::object({
                     {"id", "window.capture.preflight@1"},
                     {"status", "available"},
                     {"risk", "read"},
                     {"executionDomain", "host-headless"},
                     {"constraint",
                      "metadata-only-no-pixels-no-files-no-activation"},
                 }),
                 components::object({
                     {"id", "window.capture.frame.probe@1"},
                     {"status", "available"},
                     {"risk", "read-sensitive"},
                     {"executionDomain", "isolated-worker"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "frame-metadata-only-no-surface-read-no-file-no-activation"},
                 }),
                 components::object({
                     {"id", "browser.screenshot@1"},
                     {"status", "available-confirmed"},
                     {"risk", "read-sensitive"},
                     {"executionDomain", "isolated-worker"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "temporary-profile-fixed-arguments-no-user-browser"},
                 }),
                 components::object({
                     {"id", "application.open@1"},
                     {"status", "rust-compatibility-only"},
                     {"risk", "mutation"},
                     {"requiresConfirmation", true},
                 }),
                 components::object({
                     {"id", "window.screenshot@1"},
                     {"status", "available-confirmed"},
                     {"risk", "read"},
                     {"executionDomain", "isolated-worker"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "opaque-s2-window-only-legacy-id-via-launcher"},
                 }),
                 components::object({
                     {"id", "ui.text.input@1"},
                     {"status", "available-confirmed"},
                     {"risk", "mutation"},
                     {"executionDomain", "same-session-no-focus"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "opaque-s2-control-only-confirmed-readback"},
                 }),
                 components::object({
                     {"id", "window.close@1"},
                     {"status", "available-confirmed"},
                     {"risk", "mutation"},
                     {"executionDomain", "same-session-no-focus"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "opaque-s2-window-only-refuses-current-foreground"},
                 }),
                 components::object({
                     {"id", "ui.input.key@1"},
                     {"status", "rust-compatibility-only"},
                     {"risk", "mutation"},
                     {"requiresConfirmation", true},
                     {"requiresForegroundConsent", true},
                 }),
                 components::object({
                     {"id", "media.session.discover@1"},
                     {"status", "available"},
                     {"risk", "read-sensitive"},
                     {"executionDomain", "isolated-worker"},
                     {"constraint",
                      "opaque-session-no-source-application-id"},
                 }),
                 components::object({
                     {"id", "media.playback.state.read@1"},
                     {"status", "available"},
                     {"risk", "read-sensitive"},
                     {"executionDomain", "isolated-worker"},
                     {"constraint",
                      "exact-session-metadata-and-control-availability-only"},
                 }),
                 components::object({
                     {"id", "media.playback.control@1"},
                     {"status", "available-confirmed"},
                     {"risk", "mutation"},
                     {"executionDomain", "isolated-worker"},
                     {"requiresConfirmation", true},
                     {"constraint",
                      "opaque-s2-media-target-no-activation-no-native-id"},
                 }),
             })},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::methods(
    const std::optional<std::string>& method_id) const {
    components::Json::Array result;
    if (method_id.has_value()) {
        const auto found = std::find_if(
            directory_methods.begin(),
            directory_methods.end(),
            [&method_id](const auto& method) {
                return method.id == *method_id;
            });
        if (found == directory_methods.end()) {
            return ModuleResult{
                false,
                "INVALID_ARGUMENT",
                "The requested control method is unknown.",
                nullptr,
            };
        }
        result.push_back(legacy_method_json(*found));
    } else {
        result.reserve(directory_methods.size());
        for (const auto& method : directory_methods) {
            result.push_back(legacy_method_json(method));
        }
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"methods", components::Json(std::move(result))},
            {"ok", true},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::method_capabilities(
    const std::optional<std::string>& method_id) const {
    components::Json::Array result;
    if (method_id.has_value()) {
        const auto found = std::find_if(
            directory_methods.begin(),
            directory_methods.end(),
            [&method_id](const auto& method) {
                return method.id == *method_id;
            });
        if (found == directory_methods.end()) {
            return ModuleResult{
                false,
                "INVALID_ARGUMENT",
                "The requested control method is unknown.",
                nullptr,
            };
        }
        result.push_back(method_json(*found));
    } else {
        result.reserve(directory_methods.size());
        for (const auto& method : directory_methods) {
            result.push_back(method_json(method));
        }
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"readOnly", true},
            {"policy", "capability-first-no-silent-fallback"},
            {"methods", components::Json(std::move(result))},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::descriptor_capabilities(
    const std::string& app_id,
    const std::optional<std::string>& operation_id) const {
    std::string message;
    auto catalog = load_legacy_catalog(message);
    if (!catalog.has_value()) {
        return directory_failure(message.c_str());
    }
    const auto* apps_value =
        catalog->find("apps");
    const auto* apps =
        apps_value == nullptr ? nullptr : apps_value->array_items();
    if (apps == nullptr) {
        return directory_failure(
            "The legacy public catalog companion is invalid.");
    }
    const auto app = std::find_if(
        apps->begin(),
        apps->end(),
        [&app_id](const auto& value) {
            const auto* id = value.find("id");
            return id != nullptr &&
                   id->string_value() != nullptr &&
                   *id->string_value() == app_id;
        });
    if (app == apps->end()) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "The requested application descriptor is unknown.",
            nullptr,
        };
    }
    const components::Json* descriptor = &*app;
    if (operation_id.has_value()) {
        const auto* operations_value = app->find("operations");
        const auto* operations =
            operations_value == nullptr
                ? nullptr
                : operations_value->array_items();
        if (operations == nullptr) {
            return directory_failure(
                "The application descriptor omitted operations.");
        }
        const auto operation = std::find_if(
            operations->begin(),
            operations->end(),
            [&operation_id](const auto& value) {
                const auto* id = value.find("operation");
                return id != nullptr &&
                       id->string_value() != nullptr &&
                       *id->string_value() == *operation_id;
            });
        if (operation == operations->end()) {
            return ModuleResult{
                false,
                "INVALID_ARGUMENT",
                "The requested operation descriptor is unknown.",
                nullptr,
            };
        }
        descriptor = &*operation;
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"readOnly", true},
            {"descriptor",
             with_status(
                 *descriptor, operation_id.has_value())},
        }),
    };
}

ModuleResult CapabilityDirectoryModule::describe(
    const std::string& app_id,
    const std::optional<std::string>& operation_id) const {
    std::string message;
    auto catalog = load_legacy_catalog(message);
    if (!catalog.has_value()) {
        return directory_failure(message.c_str());
    }
    const auto* apps = catalog->find("apps")->array_items();
    const auto app = std::find_if(
        apps->begin(),
        apps->end(),
        [&app_id](const auto& value) {
            const auto* id = value.find("id");
            return id != nullptr &&
                   id->string_value() != nullptr &&
                   *id->string_value() == app_id;
        });
    if (app == apps->end()) {
        return ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "The requested application descriptor is unknown.",
            nullptr,
        };
    }
    const components::Json* descriptor = &*app;
    if (operation_id.has_value()) {
        const auto* operations_value = app->find("operations");
        const auto* operations =
            operations_value == nullptr
                ? nullptr
                : operations_value->array_items();
        if (operations == nullptr) {
            return directory_failure(
                "The application descriptor omitted operations.");
        }
        const auto operation = std::find_if(
            operations->begin(),
            operations->end(),
            [&operation_id](const auto& value) {
                const auto* id = value.find("operation");
                return id != nullptr &&
                       id->string_value() != nullptr &&
                       *id->string_value() == *operation_id;
            });
        if (operation == operations->end()) {
            return ModuleResult{
                false,
                "INVALID_ARGUMENT",
                "The requested operation descriptor is unknown.",
                nullptr,
            };
        }
        descriptor = &*operation;
    }
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"descriptor", *descriptor},
            {"ok", true},
        }),
    };
}

}  // namespace act::modules
