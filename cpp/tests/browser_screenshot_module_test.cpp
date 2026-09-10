#include "modules/browser_screenshot_module.hpp"
// 导入 CLI System 以验证任意 Chromium 参数在 Module 前被拒绝。
#include "systems/browser_command_system.hpp"

#include <windows.h>

#include <filesystem>
#include <fstream>
#include <iostream>

namespace {

bool expect(
    const bool condition,
    const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        return false;
    }
    return true;
}

}  // namespace

int main() {
    act::modules::BrowserScreenshotModule module;
    bool ok = true;
    const auto unconfirmed = module.capture(
        "javascript:alert(1)", "", false, false);
    ok = expect(
             !unconfirmed.ok &&
                 unconfirmed.error_code ==
                     "CONFIRMATION_REQUIRED",
             "Confirmation must precede all browser input validation.") &&
         ok;

    const auto invalid_url = module.capture(
        "javascript:alert(1)",
        "C:\\invalid\\page.png",
        true,
        false);
    ok = expect(
             !invalid_url.ok &&
                 invalid_url.error_code == "INVALID_ARGUMENT",
             "Unsafe browser URL schemes must be rejected.") &&
         ok;

    const auto invalid_size = module.capture(
        "https://example.invalid",
        "C:\\invalid\\page.png",
        true,
        false,
        10001U,
        720U,
        30000U);
    ok = expect(
             !invalid_size.ok &&
                 invalid_size.error_code == "INVALID_ARGUMENT",
             "Browser viewport bounds must fail closed.") &&
         ok;

    const auto invalid_timeout = module.capture(
        "https://example.invalid",
        "C:\\invalid\\page.png",
        true,
        false,
        1280U,
        720U,
        999U);
    ok = expect(
             !invalid_timeout.ok &&
                 invalid_timeout.error_code == "INVALID_ARGUMENT",
             "Browser timeout bounds must fail closed.") &&
         ok;

    // 构造 BrowserCommandSystem 以覆盖公开 CLI 参数语法。
    const act::systems::BrowserCommandSystem system;
    // 构造未确认的任意参数请求以锁定错误优先级。
    const auto unconfirmed_arbitrary = system.run({
        // 固定 verb。
        "run",
        // 固定 surface。
        "browser",
        // 固定 operation。
        "screenshot",
        // 尝试直接提供任意参数标记。
        "--arg",
        // 注入未认证 Chromium flag 字段。
        "chromiumFlag=--disable-web-security",
    });
    // confirmation-first 必须先于固定参数校验。
    ok = expect(
             // 操作不得成功。
             !unconfirmed_arbitrary.ok &&
                 // 未确认错误必须优先。
                 unconfirmed_arbitrary.error_code == "CONFIRMATION_REQUIRED",
             // 失败时给出稳定测试说明。
             "Confirmation must precede arbitrary Chromium argument validation.") &&
         // 保留之前的测试结果。
         ok;
    // 注入一个未认证 Chromium flag 字段。
    const auto arbitrary_argument = system.run({
        // 固定 verb。
        "run",
        // 固定 surface。
        "browser",
        // 固定 operation。
        "screenshot",
        // 显式满足 confirmation-first。
        "--confirm",
        // 提供认证 target 标记。
        "--target",
        // 提供有界 URL 槽。
        "url=https://example.invalid",
        // 提供认证参数标记。
        "--arg",
        // 提供必填 PNG 路径槽。
        "path=C:\\invalid\\page.png",
        // 尝试注入第二个参数标记。
        "--arg",
        // 使用未认证 Chromium flag 字段。
        "chromiumFlag=--disable-web-security",
    });
    // 任意外部进程参数必须在路径或 runtime 解析前失败。
    ok = expect(
             // 操作不得成功。
             !arbitrary_argument.ok &&
                 // 失败语义保持封闭参数错误。
                 arbitrary_argument.error_code == "INVALID_ARGUMENT",
             // 失败时给出稳定测试说明。
             "Arbitrary Chromium arguments must be rejected.") &&
         // 保留之前的测试结果。
         ok;

    std::error_code error;
    const auto directory =
        std::filesystem::temp_directory_path() /
        (L"act-browser-module-" +
         std::to_wstring(GetCurrentProcessId()));
    std::filesystem::create_directory(directory, error);
    const auto output = directory / L"existing.png";
    {
        std::ofstream file(output, std::ios::binary);
        file << "owned";
    }
    const auto overwrite = module.capture(
        "https://example.invalid",
        output.string(),
        true,
        false);
    ok = expect(
             !overwrite.ok &&
                 overwrite.error_code ==
                     "OVERWRITE_CONFIRMATION_REQUIRED",
             "Existing browser output must require overwrite consent.") &&
         ok;
    std::filesystem::remove(output, error);
    std::filesystem::remove(directory, error);
    return ok ? 0 : 1;
}
