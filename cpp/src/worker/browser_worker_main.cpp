#include "components/json.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <filesystem>
#include <iostream>
#include <optional>
#include <string>
// 导入内部协议字段白名单使用的只读字符串视图。
#include <string_view>
#include <vector>

namespace {

constexpr const char* worker_contract = "act/browser-worker/v1";

int failure(const char* code, const char* message) {
    std::cout << act::components::object({
        {"ok", false},
        {"contractVersion", worker_contract},
        {"error",
         act::components::object({
             {"code", code},
             {"message", message},
         })},
    }).dump() << '\n';
    return 2;
}

std::wstring quote_argument(const std::wstring& value) {
    std::wstring quoted = L"\"";
    std::size_t slashes = 0U;
    for (const wchar_t character : value) {
        if (character == L'\\') {
            ++slashes;
            continue;
        }
        if (character == L'"') {
            quoted.append(slashes * 2U + 1U, L'\\');
            quoted.push_back(L'"');
        } else {
            quoted.append(slashes, L'\\');
            quoted.push_back(character);
        }
        slashes = 0U;
    }
    quoted.append(slashes * 2U, L'\\');
    quoted.push_back(L'"');
    return quoted;
}

bool string_field(
    const act::components::Json& value,
    const char* name,
    std::string& output) {
    const auto* field = value.find(name);
    if (field == nullptr ||
        field->string_value() == nullptr ||
        field->string_value()->empty()) {
        return false;
    }
    output = *field->string_value();
    return true;
}

bool valid_url(const std::string& url) {
    return url.size() <= 8192U &&
           (url.starts_with("https://") ||
            url.starts_with("http://") ||
            url.starts_with("file://"));
}

// 判断 worker JSON 字段是否属于版本化内部协议。
bool known_request_field(const std::string_view name) {
    // 只允许固定协议字段和认证动态槽。
    return name == "contractVersion" ||
           // 固定 operation。
           name == "operation" ||
           // 纵深确认标记。
           name == "confirmed" ||
           // 私下发现的 Chromium 路径。
           name == "browserPath" ||
           // 本次调用专属 profile。
           name == "profilePath" ||
           // profile 内固定 staging PNG。
           name == "stagingPath" ||
           // 有界 URL。
           name == "url" ||
           // 有界 viewport 宽度。
           name == "width" ||
           // 有界 viewport 高度。
           name == "height";
}

// 拒绝 facade 未认证的任意 worker 消息字段。
bool request_fields_are_certified(
    // 接收解析后的内部 JSON request。
    const act::components::Json& request) {
    // worker request 必须是对象。
    const auto* fields = request.object_items();
    // 非对象消息失败闭合。
    if (fields == nullptr) {
        // 调用方不能用其他 JSON 类型启动 Chromium。
        return false;
    }
    // 逐字段检查稳定协议白名单。
    for (const auto& [name, value] : *fields) {
        // 当前只检查字段身份，值由后续强类型边界验证。
        static_cast<void>(value);
        // 任意 flag、argv、script 或 profile 覆盖字段均拒绝。
        if (!known_request_field(name)) {
            // 未知字段失败闭合。
            return false;
        }
    }
    // 全部字段均属于版本化 worker 协议。
    return true;
}

bool is_child_path(
    const std::filesystem::path& parent,
    const std::filesystem::path& child) {
    std::error_code error;
    const auto normalized_parent =
        std::filesystem::weakly_canonical(parent, error);
    if (error) {
        return false;
    }
    const auto normalized_child =
        std::filesystem::weakly_canonical(child, error);
    if (error) {
        return false;
    }
    auto parent_it = normalized_parent.begin();
    auto child_it = normalized_child.begin();
    while (parent_it != normalized_parent.end()) {
        if (child_it == normalized_child.end() ||
            *parent_it != *child_it) {
            return false;
        }
        ++parent_it;
        ++child_it;
    }
    return child_it != normalized_child.end();
}

class Handle final {
public:
    explicit Handle(HANDLE value = nullptr) : value_(value) {}
    ~Handle() {
        if (value_ != nullptr &&
            value_ != INVALID_HANDLE_VALUE) {
            CloseHandle(value_);
        }
    }
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    [[nodiscard]] HANDLE get() const {
        return value_;
    }
    [[nodiscard]] bool valid() const {
        return value_ != nullptr &&
               value_ != INVALID_HANDLE_VALUE;
    }

private:
    HANDLE value_;
};

int execute(const act::components::Json& request) {
    const auto* contract = request.find("contractVersion");
    const auto* operation = request.find("operation");
    const auto* confirmed = request.find("confirmed");
    const auto* width_value = request.find("width");
    const auto* height_value = request.find("height");
    // 在读取字段值前拒绝内部协议之外的任意消息。
    if (!request_fields_are_certified(request) ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        operation == nullptr ||
        operation->string_value() == nullptr ||
        *operation->string_value() != "isolated-screenshot" ||
        confirmed == nullptr ||
        confirmed->bool_value() == nullptr ||
        !*confirmed->bool_value() ||
        width_value == nullptr ||
        width_value->integer_value() == nullptr ||
        height_value == nullptr ||
        height_value->integer_value() == nullptr) {
        return failure(
            "INVALID_ARGUMENT",
            "The browser worker request is invalid.");
    }
    const auto width = *width_value->integer_value();
    const auto height = *height_value->integer_value();
    std::string browser_text;
    std::string profile_text;
    std::string staging_text;
    std::string url;
    if (width <= 0 || width > 10000 ||
        height <= 0 || height > 10000 ||
        !string_field(request, "browserPath", browser_text) ||
        !string_field(request, "profilePath", profile_text) ||
        !string_field(request, "stagingPath", staging_text) ||
        !string_field(request, "url", url) ||
        !valid_url(url)) {
        return failure(
            "INVALID_ARGUMENT",
            "The browser worker fields violate their bounds.");
    }
    const auto browser =
        std::filesystem::path(
            act::platform::windows::wide(browser_text));
    const auto profile =
        std::filesystem::path(
            act::platform::windows::wide(profile_text));
    const auto staging =
        std::filesystem::path(
            act::platform::windows::wide(staging_text));
    std::error_code error;
    if (!std::filesystem::is_regular_file(browser, error) ||
        error ||
        !std::filesystem::is_directory(profile, error) ||
        error ||
        staging.filename() != L"screenshot.png" ||
        !is_child_path(profile, staging)) {
        return failure(
            "INVALID_ARGUMENT",
            "The browser worker received invalid private paths.");
    }

    std::vector<std::wstring> arguments{
        browser.wstring(),
        L"--headless=new",
        L"--disable-gpu",
        L"--disable-extensions",
        L"--disable-sync",
        L"--disable-background-networking",
        L"--disable-component-update",
        L"--disable-default-apps",
        L"--disable-domain-reliability",
        L"--metrics-recording-only",
        L"--no-pings",
        L"--no-first-run",
        L"--no-default-browser-check",
        L"--run-all-compositor-stages-before-draw",
        L"--user-data-dir=" + profile.wstring(),
        L"--screenshot=" + staging.wstring(),
        L"--window-size=" + std::to_wstring(width) + L"," +
            std::to_wstring(height),
        act::platform::windows::wide(url),
    };
    std::wstring command_line;
    for (const auto& argument : arguments) {
        if (!command_line.empty()) {
            command_line.push_back(L' ');
        }
        command_line += quote_argument(argument);
    }
    const HWND foreground_before = GetForegroundWindow();
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (CreateProcessW(
            browser.c_str(),
            command_line.data(),
            nullptr,
            nullptr,
            FALSE,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            nullptr,
            nullptr,
            &startup,
            &process) == FALSE) {
        return failure(
            "BROWSER_START_FAILED",
            "The certified Chromium runtime could not be started.");
    }
    Handle process_handle(process.hProcess);
    Handle thread_handle(process.hThread);
    DWORD wait_status = WAIT_TIMEOUT;
    std::uintmax_t previous_size = 0U;
    std::uint32_t stable_samples = 0U;
    bool capture_completed = false;
    while ((wait_status = WaitForSingleObject(
                process_handle.get(), 100U)) == WAIT_TIMEOUT) {
        error.clear();
        const auto current_size =
            std::filesystem::file_size(staging, error);
        if (!error && current_size >= 24U &&
            current_size <= 64U * 1024U * 1024U) {
            stable_samples =
                current_size == previous_size
                    ? stable_samples + 1U
                    : 0U;
            previous_size = current_size;
            if (stable_samples >= 2U) {
                capture_completed = true;
                break;
            }
        }
    }
    if (wait_status == WAIT_FAILED) {
        return failure(
            "BROWSER_FAILED",
            "Waiting for the isolated Chromium process failed.");
    }
    if (wait_status == WAIT_OBJECT_0) {
        DWORD exit_code = 1U;
        if (GetExitCodeProcess(
                process_handle.get(), &exit_code) == FALSE ||
            exit_code != 0U) {
            return failure(
                "BROWSER_FAILED",
                "The isolated Chromium process returned a failure.");
        }
        capture_completed = true;
    }
    const auto size =
        std::filesystem::file_size(staging, error);
    if (error || size < 24U ||
        size > 64U * 1024U * 1024U) {
        return failure(
            "SCREENSHOT_MISSING",
            "The isolated Chromium process did not write a bounded PNG.");
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"browserExited",
              wait_status == WAIT_OBJECT_0},
             {"captureCompleted", capture_completed},
             {"foregroundUnchanged",
              foreground_before == GetForegroundWindow()},
             {"stagingBytes",
              static_cast<std::int64_t>(size)},
             {"nativeIdentifiersExposed", false},
             {"runtimePathExposed", false},
         })},
    }).dump() << '\n';
    return 0;
}

}  // namespace

int main() {
    std::string input;
    std::getline(std::cin, input);
    if (input.size() > 65536U) {
        return failure(
            "INVALID_ARGUMENT",
            "The browser worker request exceeds its size limit.");
    }
    std::string parse_error;
    const auto request =
        act::components::Json::parse(input, parse_error);
    if (!request.has_value()) {
        return failure(
            "INVALID_ARGUMENT",
            "The browser worker requires one JSON request.");
    }
    return execute(*request);
}
