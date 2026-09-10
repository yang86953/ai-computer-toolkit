#include "platform/windows/browser_worker_backend.hpp"

#include "components/json.hpp"
#include "components/worker_process.hpp"
#include "platform/windows/png_file_output.hpp"
#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <thread>
#include <vector>

namespace act::platform::windows {
namespace {

constexpr const char* worker_contract = "act/browser-worker/v1";
constexpr const char* worker_name =
    "ai-computer-toolkit-browser-worker.exe";
constexpr std::size_t maximum_png_bytes =
    64U * 1024U * 1024U;
std::atomic<std::uint32_t> profile_sequence{0U};

std::optional<std::filesystem::path> environment_path(
    const wchar_t* name) {
    const DWORD required =
        GetEnvironmentVariableW(name, nullptr, 0U);
    if (required == 0U || required > 32768U) {
        return std::nullopt;
    }
    std::vector<wchar_t> value(required);
    const DWORD written = GetEnvironmentVariableW(
        name, value.data(), required);
    if (written == 0U || written >= required) {
        return std::nullopt;
    }
    return std::filesystem::path(
        std::wstring(value.data(), written));
}

bool regular_file(const std::filesystem::path& path) {
    std::error_code error;
    return std::filesystem::is_regular_file(path, error) &&
           !error;
}

std::optional<std::filesystem::path> search_path(
    const wchar_t* executable) {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length = SearchPathW(
        nullptr,
        executable,
        nullptr,
        static_cast<DWORD>(buffer.size()),
        buffer.data(),
        nullptr);
    if (length == 0U || length >= buffer.size()) {
        return std::nullopt;
    }
    return std::filesystem::path(
        std::wstring(buffer.data(), length));
}

std::optional<std::filesystem::path> browser_runtime() {
    if (const auto override_path =
            environment_path(
                L"AI_COMPUTER_TOOLKIT_BROWSER_PATH");
        override_path.has_value() &&
        regular_file(*override_path)) {
        return std::filesystem::absolute(*override_path);
    }
    for (const wchar_t* name :
         {L"chrome.exe", L"msedge.exe", L"chromium.exe"}) {
        if (const auto found = search_path(name);
            found.has_value() && regular_file(*found)) {
            return *found;
        }
    }
    struct Candidate {
        const wchar_t* environment;
        const wchar_t* relative;
    };
    constexpr Candidate candidates[]{
        {L"PROGRAMFILES",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"PROGRAMFILES(X86)",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"LOCALAPPDATA",
         L"Google\\Chrome\\Application\\chrome.exe"},
        {L"PROGRAMFILES",
         L"Microsoft\\Edge\\Application\\msedge.exe"},
        {L"PROGRAMFILES(X86)",
         L"Microsoft\\Edge\\Application\\msedge.exe"},
    };
    for (const auto& candidate : candidates) {
        const auto base =
            environment_path(candidate.environment);
        if (!base.has_value()) {
            continue;
        }
        const auto path = *base / candidate.relative;
        if (regular_file(path)) {
            return path;
        }
    }
    return std::nullopt;
}

class TemporaryProfile final {
public:
    TemporaryProfile() {
        std::array<wchar_t, 32768> buffer{};
        const DWORD length = GetTempPathW(
            static_cast<DWORD>(buffer.size()), buffer.data());
        if (length == 0U || length >= buffer.size()) {
            return;
        }
        std::error_code error;
        const std::filesystem::path root =
            std::filesystem::path(
                std::wstring(buffer.data(), length)) /
            L"ai-computer-toolkit-browser";
        std::filesystem::create_directories(root, error);
        if (error) {
            return;
        }
        for (std::uint32_t attempt = 0U;
             attempt < 16U;
             ++attempt) {
            const auto sequence =
                profile_sequence.fetch_add(1U);
            auto candidate =
                root /
                (L"profile-" +
                 std::to_wstring(GetCurrentProcessId()) + L"-" +
                 std::to_wstring(GetTickCount64()) + L"-" +
                 std::to_wstring(sequence));
            error.clear();
            if (std::filesystem::create_directory(
                    candidate, error)) {
                path_ = std::move(candidate);
                return;
            }
        }
    }

    ~TemporaryProfile() {
        if (!path_.empty() &&
            path_.filename().wstring().starts_with(L"profile-")) {
            for (std::uint32_t attempt = 0U;
                 attempt < 40U;
                 ++attempt) {
                std::error_code error;
                std::filesystem::remove_all(path_, error);
                error.clear();
                if (!std::filesystem::exists(path_, error)) {
                    return;
                }
                std::this_thread::sleep_for(
                    std::chrono::milliseconds(50));
            }
        }
    }

    TemporaryProfile(const TemporaryProfile&) = delete;
    TemporaryProfile& operator=(const TemporaryProfile&) = delete;

    [[nodiscard]] bool valid() const {
        return !path_.empty();
    }
    [[nodiscard]] const std::filesystem::path& path() const {
        return path_;
    }

private:
    std::filesystem::path path_;
};

BackendError process_error(
    const components::WorkerProcessResult& result) {
    using Completion = components::WorkerCompletion;
    switch (result.completion) {
        case Completion::timed_out:
            return BackendError{
                "BROWSER_TIMEOUT",
                "The isolated browser job exceeded its deadline and was "
                "terminated.",
            };
        case Completion::cancelled:
            return BackendError{
                "CANCELLED",
                "The isolated browser job was cancelled and terminated.",
            };
        case Completion::unavailable:
            return BackendError{
                "ISOLATED_WORKER_UNAVAILABLE",
                result.error_message,
            };
        case Completion::protocol_failure:
            return BackendError{
                "BROWSER_FAILED", result.error_message};
        case Completion::completed:
            break;
    }
    return BackendError{
        "BROWSER_FAILED",
        "The isolated browser worker returned an invalid result.",
    };
}

std::optional<std::vector<std::uint8_t>> read_png(
    const std::filesystem::path& path) {
    std::error_code error;
    const auto size = std::filesystem::file_size(path, error);
    if (error || size < 24U || size > maximum_png_bytes) {
        return std::nullopt;
    }
    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) {
        return std::nullopt;
    }
    std::vector<std::uint8_t> bytes(
        static_cast<std::size_t>(size));
    file.read(
        reinterpret_cast<char*>(bytes.data()),
        static_cast<std::streamsize>(bytes.size()));
    if (!file || file.gcount() !=
                     static_cast<std::streamsize>(bytes.size())) {
        return std::nullopt;
    }
    constexpr std::array<std::uint8_t, 8> signature{
        0x89U, 0x50U, 0x4eU, 0x47U,
        0x0dU, 0x0aU, 0x1aU, 0x0aU,
    };
    if (!std::equal(
            signature.begin(), signature.end(), bytes.begin())) {
        return std::nullopt;
    }
    return bytes;
}

std::int64_t png_dimension(
    const std::vector<std::uint8_t>& png,
    const std::size_t offset) {
    return static_cast<std::int64_t>(
        (static_cast<std::uint32_t>(png[offset]) << 24U) |
        (static_cast<std::uint32_t>(png[offset + 1U]) << 16U) |
        (static_cast<std::uint32_t>(png[offset + 2U]) << 8U) |
        static_cast<std::uint32_t>(png[offset + 3U]));
}

BrowserScreenshotResult failure(
    const char* code,
    const std::string& message) {
    return BrowserScreenshotResult{
        std::nullopt, BackendError{code, message}};
}

}  // namespace

bool BrowserWorkerBackend::runtime_available() const {
    return browser_runtime().has_value();
}

BrowserScreenshotResult BrowserWorkerBackend::capture(
    const std::string& url,
    const std::string& output_path,
    const bool overwrite,
    const std::uint32_t width,
    const std::uint32_t height,
    const std::uint32_t timeout_ms) const {
    const auto browser = browser_runtime();
    if (!browser.has_value()) {
        return failure(
            "BROWSER_UNAVAILABLE",
            "No certified Chromium runtime is available.");
    }
    TemporaryProfile profile;
    if (!profile.valid()) {
        return failure(
            "TEMP_PROFILE_FAILED",
            "The isolated browser profile could not be created.");
    }
    const auto staging = profile.path() / L"screenshot.png";
    const HWND foreground_before = GetForegroundWindow();
    const auto request = components::object({
        {"contractVersion", worker_contract},
        {"operation", "isolated-screenshot"},
        {"browserPath", utf8(browser->wstring())},
        {"profilePath", utf8(profile.path().wstring())},
        {"stagingPath", utf8(staging.wstring())},
        {"url", url},
        {"width", static_cast<std::int64_t>(width)},
        {"height", static_cast<std::int64_t>(height)},
        {"confirmed", true},
    });
    const auto process = components::WorkerProcess().run_companion(
        worker_name,
        request.dump(),
        timeout_ms,
        1024U * 1024U);
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return BrowserScreenshotResult{
            std::nullopt, process_error(process)};
    }
    if (!process.job_terminated) {
        return failure(
            "BROWSER_FAILED",
            "The isolated browser worker job was not fully terminated.");
    }
    std::string parse_error;
    const auto response =
        components::Json::parse(process.stdout_text, parse_error);
    const auto* contract =
        response.has_value()
            ? response->find("contractVersion")
            : nullptr;
    const auto* ok =
        response.has_value() ? response->find("ok") : nullptr;
    if (!response.has_value() ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr || ok->bool_value() == nullptr) {
        return failure(
            "BROWSER_FAILED",
            "The isolated browser worker violated its protocol.");
    }
    if (!*ok->bool_value()) {
        const auto* error = response->find("error");
        const auto* code =
            error == nullptr ? nullptr : error->find("code");
        const auto* message =
            error == nullptr ? nullptr : error->find("message");
        if (code == nullptr ||
            code->string_value() == nullptr ||
            message == nullptr ||
            message->string_value() == nullptr) {
            return failure(
                "BROWSER_FAILED",
                "The isolated browser worker returned an invalid error.");
        }
        return failure(
            code->string_value()->c_str(),
            *message->string_value());
    }
    const auto* data = response->find("data");
    const auto* unchanged =
        data == nullptr
            ? nullptr
            : data->find("foregroundUnchanged");
    const auto* completed =
        data == nullptr
            ? nullptr
            : data->find("captureCompleted");
    if (data == nullptr ||
        data->object_items() == nullptr ||
        unchanged == nullptr ||
        unchanged->bool_value() == nullptr ||
        !*unchanged->bool_value() ||
        completed == nullptr ||
        completed->bool_value() == nullptr ||
        !*completed->bool_value()) {
        return failure(
            "BROWSER_FAILED",
            "The browser worker omitted certified completion evidence.");
    }
    if (foreground_before != GetForegroundWindow()) {
        return failure(
            "FOREGROUND_CHANGED",
            "The isolated browser changed the foreground; no output was "
            "committed.");
    }
    auto png = read_png(staging);
    if (!png.has_value()) {
        return failure(
            "SCREENSHOT_MISSING",
            "The isolated browser did not produce a bounded PNG.");
    }
    const std::int64_t actual_width =
        png_dimension(*png, 16U);
    const std::int64_t actual_height =
        png_dimension(*png, 20U);
    if (actual_width <= 0 || actual_height <= 0 ||
        actual_width > 10000 || actual_height > 10000) {
        return failure(
            "BROWSER_FAILED",
            "The isolated browser produced invalid PNG dimensions.");
    }
    auto output = write_png_atomically(
        output_path, *png, overwrite);
    if (!output.output.has_value()) {
        return failure(
            output.error_code.c_str(), output.error_message);
    }
    if (foreground_before != GetForegroundWindow()) {
        return failure(
            "FOREGROUND_CHANGED",
            "The PNG was committed, but foreground changed during atomic "
            "output."
        );
    }
    return BrowserScreenshotResult{
        BrowserScreenshot{
            output.output->normalized_path,
            output.output->bytes_written,
            actual_width,
            actual_height,
            output.output->replaced_existing,
            true,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
