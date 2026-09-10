#include "systems/browser_command_system.hpp"

// 导入固定字段白名单使用的数组类型。
#include <array>
#include <charconv>
#include <cstdint>
#include <optional>
#include <string_view>

namespace act::systems {
namespace {

bool has_flag(
    const std::vector<std::string>& arguments,
    const std::string_view flag) {
    for (const auto& argument : arguments) {
        if (argument == flag) {
            return true;
        }
    }
    return false;
}

std::optional<std::string> named_value(
    const std::vector<std::string>& arguments,
    const std::string_view flag,
    const std::string_view name) {
    const std::string prefix = std::string(name) + '=';
    for (std::size_t index = 3U;
         index + 1U < arguments.size();
         ++index) {
        if (arguments[index] == flag &&
            arguments[index + 1U].starts_with(prefix)) {
            return arguments[index + 1U].substr(prefix.size());
        }
    }
    return std::nullopt;
}

std::optional<std::uint32_t> unsigned_value(
    const std::vector<std::string>& arguments,
    const std::string_view name,
    const std::uint32_t fallback) {
    const auto raw = named_value(arguments, "--arg", name);
    if (!raw.has_value()) {
        return fallback;
    }
    std::uint32_t value = 0U;
    const auto result = std::from_chars(
        raw->data(), raw->data() + raw->size(), value);
    if (result.ec != std::errc{} ||
        result.ptr != raw->data() + raw->size()) {
        return std::nullopt;
    }
    return value;
}

std::optional<bool> overwrite_value(
    const std::vector<std::string>& arguments) {
    const auto raw =
        named_value(arguments, "--arg", "overwrite");
    if (!raw.has_value() || *raw == "false") {
        return false;
    }
    if (*raw == "true") {
        return true;
    }
    return std::nullopt;
}

// 判断一个命名参数是否属于认证 Chromium 动态槽。
bool named_argument_is_certified(
    // 接收参数所属的 CLI 标记。
    const std::string_view flag,
    // 接收完整 name=value 文本。
    const std::string_view raw) {
    // target 只允许唯一 URL 槽。
    if (flag == "--target") {
        // URL 值随后仍由 Browser Module 验证 scheme 与长度。
        return raw.starts_with("url=");
    }
    // target 与 arg 之外的命名标记一律拒绝。
    if (flag != "--arg") {
        // 未认证 CLI 形状失败闭合。
        return false;
    }
    // 固定 Chromium 模板允许的有界参数名称。
    constexpr std::array allowed{
        // PNG 输出路径。
        "path",
        // viewport 宽度。
        "width",
        // viewport 高度。
        "height",
        // worker deadline。
        "timeoutMs",
        // 独立覆盖许可。
        "overwrite",
    };
    // 逐个匹配稳定 name= 前缀。
    for (const auto* name : allowed) {
        // 构造不会包含调用方数据的认证前缀。
        const std::string prefix = std::string(name) + '=';
        // 匹配成功表示动态槽名称受控。
        if (raw.starts_with(prefix)) {
            // 槽值随后由强类型 Module 校验。
            return true;
        }
    }
    // argv、flag、profile 与 executable 等未知字段全部拒绝。
    return false;
}

// 验证 BrowserCommandSystem 接收的完整参数语法。
bool arguments_are_certified(
    // 接收 control system 已分派的 browser screenshot argv。
    const std::vector<std::string>& arguments) {
    // 从三个固定位置参数之后开始检查。
    for (std::size_t index = 3U;
         // 覆盖全部剩余参数。
         index < arguments.size();
         // 每轮按实际消费数量推进。
         ++index) {
        // confirmation 是唯一允许的独立 flag。
        if (arguments[index] == "--confirm") {
            // 继续检查后续参数。
            continue;
        }
        // 命名标记必须同时携带一个 name=value 值。
        if (index + 1U >= arguments.size() ||
            // 标记和值必须属于认证动态槽。
            !named_argument_is_certified(
                arguments[index], arguments[index + 1U])) {
            // 任意或不完整参数失败闭合。
            return false;
        }
        // 跳过已经验证的 name=value 值。
        ++index;
    }
    // 全部参数均符合固定模板语法。
    return true;
}

}  // namespace

modules::ModuleResult BrowserCommandSystem::status() const {
    return screenshots_.status();
}

modules::ModuleResult BrowserCommandSystem::run(
    const std::vector<std::string>& arguments) const {
    if (!has_flag(arguments, "--confirm")) {
        return screenshots_.capture({}, {}, false, false);
    }
    // confirmation-first 后拒绝任意 Chromium flag 或未知命名字段。
    if (!arguments_are_certified(arguments)) {
        // 未认证参数不得进入 Browser Module 或 worker。
        return modules::ModuleResult{
            // 固定参数校验失败。
            false,
            // 保持公开封闭参数错误码。
            "INVALID_ARGUMENT",
            // 不回显潜在命令内容。
            "browser.screenshot contains an unknown external-process argument.",
            // 参数错误没有数据结果。
            nullptr,
        };
    }
    const auto url =
        named_value(arguments, "--target", "url");
    const auto path =
        named_value(arguments, "--arg", "path");
    const auto width =
        unsigned_value(arguments, "width", 1280U);
    const auto height =
        unsigned_value(arguments, "height", 720U);
    const auto timeout =
        unsigned_value(arguments, "timeoutMs", 30000U);
    const auto overwrite = overwrite_value(arguments);
    if (!url.has_value() || !path.has_value() ||
        !width.has_value() || !height.has_value() ||
        !timeout.has_value() || !overwrite.has_value()) {
        return modules::ModuleResult{
            false,
            "INVALID_ARGUMENT",
            "browser.screenshot requires URL, PNG path, integer width, "
            "height and timeoutMs, and boolean overwrite.",
            nullptr,
        };
    }
    return screenshots_.capture(
        *url,
        *path,
        true,
        *overwrite,
        *width,
        *height,
        *timeout);
}

}  // namespace act::systems
