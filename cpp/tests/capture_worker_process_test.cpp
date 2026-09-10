#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <windows.h>

#include <array>
#include <filesystem>
#include <iostream>

int main() {
    const act::components::WorkerProcess worker;
    std::array<wchar_t, 32768> policy_module{};
    const DWORD policy_module_length = GetModuleFileNameW(
        nullptr,
        policy_module.data(),
        static_cast<DWORD>(policy_module.size()));
    if (policy_module_length == 0U ||
        policy_module_length >= policy_module.size()) {
        std::cerr << "policy test executable path is unavailable\n";
        return 1;
    }
    const std::filesystem::path policy_output =
        std::filesystem::path(std::wstring(
            policy_module.data(), policy_module_length))
            .parent_path() /
        ("window-screenshot-policy-" +
         std::to_string(GetCurrentProcessId()) + ".png");
    std::error_code policy_filesystem_error;
    std::filesystem::remove(
        policy_output, policy_filesystem_error);

    const auto screenshot_request =
        act::components::object({
            {"contractVersion", "act/capture-worker/v1"},
            {"operation", "window-screenshot"},
            {"sessionId", "s2:w:0000000000000000"},
            {"outputPath", policy_output.string()},
            {"confirmed", false},
            {"overwrite", false},
        }).dump();
    const auto unconfirmed_screenshot = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        screenshot_request,
        10000U,
        1024U * 1024U);
    std::string policy_parse_error;
    auto unconfirmed_screenshot_response =
        act::components::Json::parse(
            unconfirmed_screenshot.stdout_text,
            policy_parse_error);
    const auto* unconfirmed_screenshot_error =
        unconfirmed_screenshot_response.has_value()
            ? unconfirmed_screenshot_response->find("error")
            : nullptr;
    const auto* unconfirmed_screenshot_code =
        unconfirmed_screenshot_error == nullptr
            ? nullptr
            : unconfirmed_screenshot_error->find("code");
    if (unconfirmed_screenshot.exit_code == 0U ||
        unconfirmed_screenshot_code == nullptr ||
        unconfirmed_screenshot_code->string_value() == nullptr ||
        *unconfirmed_screenshot_code->string_value() !=
            "CONFIRMATION_REQUIRED" ||
        std::filesystem::exists(policy_output)) {
        std::cerr << "real screenshot worker bypassed confirmation\n";
        return 1;
    }

    const auto stale_request =
        act::components::object({
            {"contractVersion", "act/capture-worker/v1"},
            {"operation", "window-screenshot"},
            {"sessionId", "s2:w:0000000000000000"},
            {"outputPath", policy_output.string()},
            {"confirmed", true},
            {"overwrite", false},
        }).dump();
    const auto stale_screenshot = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        stale_request,
        10000U,
        1024U * 1024U);
    auto stale_screenshot_response =
        act::components::Json::parse(
            stale_screenshot.stdout_text,
            policy_parse_error);
    const auto* stale_screenshot_error =
        stale_screenshot_response.has_value()
            ? stale_screenshot_response->find("error")
            : nullptr;
    const auto* stale_screenshot_code =
        stale_screenshot_error == nullptr
            ? nullptr
            : stale_screenshot_error->find("code");
    if (stale_screenshot.exit_code == 0U ||
        stale_screenshot_code == nullptr ||
        stale_screenshot_code->string_value() == nullptr ||
        *stale_screenshot_code->string_value() != "STALE_SESSION" ||
        std::filesystem::exists(policy_output)) {
        std::cerr << "real screenshot worker accepted a stale target\n";
        return 1;
    }

    const auto request = act::components::object({
        {"contractVersion", "act/capture-worker/v1"},
        {"operation", "fixture-memory-frame"},
    }).dump();

    const auto timeout = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        request,
        1U,
        1024U * 1024U);
    if (timeout.completion !=
            act::components::WorkerCompletion::timed_out ||
        !timeout.job_terminated) {
        std::cerr << "capture worker timeout did not terminate its job\n";
        return 1;
    }

    const auto normal = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        request,
        10000U,
        1024U * 1024U);
    if (normal.completion !=
            act::components::WorkerCompletion::completed ||
        normal.exit_code != 0U) {
        std::cerr << "capture worker did not complete normally\n";
        return 1;
    }
    std::string parse_error;
    auto response =
        act::components::Json::parse(normal.stdout_text, parse_error);
    const auto* ok =
        response.has_value() ? response->find("ok") : nullptr;
    const auto* data =
        response.has_value() ? response->find("data") : nullptr;
    if (!response.has_value() ||
        ok == nullptr ||
        ok->bool_value() == nullptr ||
        !*ok->bool_value() ||
        data == nullptr ||
        data->object_items() == nullptr) {
        std::cerr << "capture worker returned an invalid envelope\n";
        return 1;
    }

    const auto* acquired = data->find("frameAcquired");
    const auto* width = data->find("frameWidth");
    const auto* height = data->find("frameHeight");
    const auto* driver = data->find("deviceDriver");
    const auto* pixels = data->find("pixelsPersisted");
    const auto* file = data->find("fileWritten");
    const auto* foreground = data->find("foregroundUnchanged");
    const auto* privacy =
        data->find("privacyIndicatorMayHaveAppeared");
    if (acquired == nullptr ||
        acquired->bool_value() == nullptr ||
        !*acquired->bool_value() ||
        width == nullptr ||
        width->integer_value() == nullptr ||
        height == nullptr ||
        height->integer_value() == nullptr ||
        driver == nullptr ||
        driver->string_value() == nullptr ||
        pixels == nullptr ||
        pixels->bool_value() == nullptr ||
        *pixels->bool_value() ||
        file == nullptr ||
        file->bool_value() == nullptr ||
        *file->bool_value() ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr ||
        !*foreground->bool_value() ||
        privacy == nullptr ||
        privacy->bool_value() == nullptr ||
        !*privacy->bool_value()) {
        std::cerr << "capture worker violated its fixture safety result\n";
        return 1;
    }

    const auto readback_request = act::components::object({
        {"contractVersion", "act/capture-worker/v1"},
        {"operation", "fixture-surface-readback"},
    }).dump();
    const auto readback = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        readback_request,
        10000U,
        1024U * 1024U);
    if (readback.completion !=
            act::components::WorkerCompletion::completed ||
        readback.exit_code != 0U) {
        std::cerr << "fixture surface readback did not complete\n";
        return 1;
    }
    auto readback_response =
        act::components::Json::parse(
            readback.stdout_text, parse_error);
    const auto* readback_data =
        readback_response.has_value()
            ? readback_response->find("data")
            : nullptr;
    const auto* surface = readback_data == nullptr
                              ? nullptr
                              : readback_data->find(
                                    "frameSurfaceAccessed");
    const auto* bytes = readback_data == nullptr
                            ? nullptr
                            : readback_data->find("pixelBytesRead");
    const auto* pitch = readback_data == nullptr
                            ? nullptr
                            : readback_data->find("rowPitch");
    const auto* digest = readback_data == nullptr
                             ? nullptr
                             : readback_data->find("pixelDigest");
    const auto* readback_pixels =
        readback_data == nullptr
            ? nullptr
            : readback_data->find("pixelsPersisted");
    const auto* readback_file =
        readback_data == nullptr
            ? nullptr
            : readback_data->find("fileWritten");
    if (readback_data == nullptr ||
        surface == nullptr ||
        surface->bool_value() == nullptr ||
        !*surface->bool_value() ||
        bytes == nullptr ||
        bytes->integer_value() == nullptr ||
        *bytes->integer_value() != 128LL * 96LL * 4LL ||
        pitch == nullptr ||
        pitch->integer_value() == nullptr ||
        *pitch->integer_value() < 128LL * 4LL ||
        digest == nullptr ||
        digest->string_value() == nullptr ||
        digest->string_value()->size() != 16U ||
        readback_pixels == nullptr ||
        readback_pixels->bool_value() == nullptr ||
        *readback_pixels->bool_value() ||
        readback_file == nullptr ||
        readback_file->bool_value() == nullptr ||
        *readback_file->bool_value()) {
        std::cerr << "fixture surface readback violated its bounds\n";
        return 1;
    }

    const auto png_request = act::components::object({
        {"contractVersion", "act/capture-worker/v1"},
        {"operation", "fixture-memory-png"},
    }).dump();
    const auto png = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        png_request,
        10000U,
        1024U * 1024U);
    if (png.completion !=
            act::components::WorkerCompletion::completed ||
        png.exit_code != 0U) {
        std::cerr << "fixture memory PNG did not complete\n";
        return 1;
    }
    auto png_response =
        act::components::Json::parse(
            png.stdout_text, parse_error);
    const auto* png_data =
        png_response.has_value()
            ? png_response->find("data")
            : nullptr;
    const auto* png_encoded =
        png_data == nullptr ? nullptr : png_data->find("pngEncoded");
    const auto* png_bytes =
        png_data == nullptr ? nullptr : png_data->find("pngBytes");
    const auto* png_digest =
        png_data == nullptr ? nullptr : png_data->find("pngDigest");
    const auto* png_signature =
        png_data == nullptr
            ? nullptr
            : png_data->find("pngSignatureValid");
    const auto* png_format =
        png_data == nullptr ? nullptr : png_data->find("pixelFormat");
    const auto* png_file =
        png_data == nullptr ? nullptr : png_data->find("fileWritten");
    if (png_data == nullptr ||
        png_encoded == nullptr ||
        png_encoded->bool_value() == nullptr ||
        !*png_encoded->bool_value() ||
        png_bytes == nullptr ||
        png_bytes->integer_value() == nullptr ||
        *png_bytes->integer_value() <= 8 ||
        png_digest == nullptr ||
        png_digest->string_value() == nullptr ||
        png_digest->string_value()->size() != 16U ||
        png_signature == nullptr ||
        png_signature->bool_value() == nullptr ||
        !*png_signature->bool_value() ||
        png_format == nullptr ||
        png_format->string_value() == nullptr ||
        *png_format->string_value() != "rgba8" ||
        png_file == nullptr ||
        png_file->bool_value() == nullptr ||
        *png_file->bool_value()) {
        std::cerr << "fixture memory PNG violated its safety contract\n";
        return 1;
    }

    std::array<wchar_t, 32768> module{};
    const DWORD module_length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    if (module_length == 0U ||
        module_length >= module.size()) {
        std::cerr << "test executable path is unavailable\n";
        return 1;
    }
    const std::filesystem::path fixture_directory =
        std::filesystem::path(
            std::wstring(module.data(), module_length))
            .parent_path() /
        "atomic-png-worker-fixtures";
    std::error_code filesystem_error;
    std::filesystem::create_directory(
        fixture_directory, filesystem_error);
    if (filesystem_error) {
        std::cerr << "worker fixture directory is unavailable\n";
        return 1;
    }
    const std::string fixture_name =
        "worker-" +
        std::to_string(GetCurrentProcessId()) + ".png";
    const std::filesystem::path fixture_path =
        fixture_directory / fixture_name;
    std::filesystem::remove(fixture_path, filesystem_error);

    const auto file_request =
        act::components::object({
            {"contractVersion", "act/capture-worker/v1"},
            {"operation", "fixture-png-file"},
            {"outputFileName", fixture_name},
            {"confirmed", false},
            {"overwrite", false},
        }).dump();
    const auto unconfirmed = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        file_request,
        10000U,
        1024U * 1024U);
    std::string unconfirmed_parse_error;
    auto unconfirmed_response =
        act::components::Json::parse(
            unconfirmed.stdout_text,
            unconfirmed_parse_error);
    const auto* unconfirmed_error =
        unconfirmed_response.has_value()
            ? unconfirmed_response->find("error")
            : nullptr;
    const auto* unconfirmed_code =
        unconfirmed_error == nullptr
            ? nullptr
            : unconfirmed_error->find("code");
    if (unconfirmed.exit_code == 0U ||
        unconfirmed_code == nullptr ||
        unconfirmed_code->string_value() == nullptr ||
        *unconfirmed_code->string_value() !=
            "CONFIRMATION_REQUIRED" ||
        std::filesystem::exists(fixture_path)) {
        std::cerr << "fixture file output bypassed confirmation\n";
        return 1;
    }

    const auto confirmed_request =
        act::components::object({
            {"contractVersion", "act/capture-worker/v1"},
            {"operation", "fixture-png-file"},
            {"outputFileName", fixture_name},
            {"confirmed", true},
            {"overwrite", false},
        }).dump();
    const auto first_file = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        confirmed_request,
        10000U,
        1024U * 1024U);
    auto first_file_response =
        act::components::Json::parse(
            first_file.stdout_text, parse_error);
    const auto* first_file_data =
        first_file_response.has_value()
            ? first_file_response->find("data")
            : nullptr;
    const auto* first_file_written =
        first_file_data == nullptr
            ? nullptr
            : first_file_data->find("fileWritten");
    const auto* first_replaced =
        first_file_data == nullptr
            ? nullptr
            : first_file_data->find("replacedExisting");
    if (first_file.exit_code != 0U ||
        first_file_written == nullptr ||
        first_file_written->bool_value() == nullptr ||
        !*first_file_written->bool_value() ||
        first_replaced == nullptr ||
        first_replaced->bool_value() == nullptr ||
        *first_replaced->bool_value() ||
        !std::filesystem::exists(fixture_path) ||
        std::filesystem::file_size(fixture_path) <= 8U) {
        std::cerr << "fixture worker did not commit PNG atomically\n";
        return 1;
    }

    const auto refused_file = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        confirmed_request,
        10000U,
        1024U * 1024U);
    auto refused_response =
        act::components::Json::parse(
            refused_file.stdout_text, parse_error);
    const auto* refused_error =
        refused_response.has_value()
            ? refused_response->find("error")
            : nullptr;
    const auto* refused_code =
        refused_error == nullptr
            ? nullptr
            : refused_error->find("code");
    if (refused_file.exit_code == 0U ||
        refused_code == nullptr ||
        refused_code->string_value() == nullptr ||
        *refused_code->string_value() !=
            "OVERWRITE_CONFIRMATION_REQUIRED") {
        std::cerr << "fixture worker did not refuse overwrite\n";
        return 1;
    }

    const auto overwrite_request =
        act::components::object({
            {"contractVersion", "act/capture-worker/v1"},
            {"operation", "fixture-png-file"},
            {"outputFileName", fixture_name},
            {"confirmed", true},
            {"overwrite", true},
        }).dump();
    const auto replaced_file = worker.run_companion(
        "ai-computer-toolkit-capture-worker.exe",
        overwrite_request,
        10000U,
        1024U * 1024U);
    auto replaced_response =
        act::components::Json::parse(
            replaced_file.stdout_text, parse_error);
    const auto* replaced_data =
        replaced_response.has_value()
            ? replaced_response->find("data")
            : nullptr;
    const auto* replaced_existing =
        replaced_data == nullptr
            ? nullptr
            : replaced_data->find("replacedExisting");
    if (replaced_file.exit_code != 0U ||
        replaced_existing == nullptr ||
        replaced_existing->bool_value() == nullptr ||
        !*replaced_existing->bool_value()) {
        std::cerr << "fixture worker did not confirm atomic replace\n";
        return 1;
    }
    std::filesystem::remove(fixture_path, filesystem_error);
    std::filesystem::remove(
        fixture_directory, filesystem_error);
    if (std::filesystem::exists(fixture_path)) {
        std::cerr << "fixture worker output was not cleaned\n";
        return 1;
    }

    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", "act/capture-worker/v1"},
        {"timeoutJobTerminated", timeout.job_terminated},
        {"frameAcquired", true},
        {"frameWidth", *width->integer_value()},
        {"frameHeight", *height->integer_value()},
        {"deviceDriver", *driver->string_value()},
        {"pixelsPersisted", false},
        {"fileWritten", false},
        {"foregroundUnchanged", true},
        {"privacyIndicatorMayHaveAppeared", true},
        {"surfaceReadback", true},
        {"pixelBytesRead", *bytes->integer_value()},
        {"rowPitch", *pitch->integer_value()},
        {"pixelDigest", *digest->string_value()},
        {"pngEncoded", true},
        {"pngBytes", *png_bytes->integer_value()},
        {"pngDigest", *png_digest->string_value()},
        {"pngSignatureValid", true},
        {"pixelFormat", "rgba8"},
        {"workerFileOutput", true},
        {"workerUnconfirmedWriteRefused", true},
        {"workerUnconfirmedOverwriteRefused", true},
        {"workerConfirmedReplace", true},
        {"workerFixtureCleaned", true},
        {"realScreenshotUnconfirmedRefused", true},
        {"realScreenshotStaleTargetRefused", true},
        {"realScreenshotAttempted", false},
    }).dump() << '\n';
    return 0;
}
