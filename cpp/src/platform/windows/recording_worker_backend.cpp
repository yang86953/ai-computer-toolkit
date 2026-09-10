#include "platform/windows/recording_worker_backend.hpp"

#include "components/worker_process.hpp"

#include <windows.h>

#include <array>
#include <filesystem>
#include <string>

namespace act::platform::windows {
namespace {

constexpr const char* worker_contract =
    "act/recording-worker/v1";
constexpr const char* worker_name =
    "ai-computer-toolkit-recording-worker.exe";

std::optional<std::filesystem::path> module_directory() {
    std::array<wchar_t, 32768> module{};
    const DWORD length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    if (length == 0U || length >= module.size()) {
        return std::nullopt;
    }
    return std::filesystem::path(
               std::wstring(module.data(), length))
        .parent_path();
}

std::string staging_token() {
    return "p" + std::to_string(GetCurrentProcessId()) +
        "-" + std::to_string(GetTickCount64());
}

bool cleanup_staging(
    const std::string& token) {
    const auto directory = module_directory();
    if (!directory.has_value()) {
        return false;
    }
    const auto root =
        *directory / L"recording-worker-staging";
    const auto owned =
        root / std::filesystem::path(token);
    std::error_code error;
    if (std::filesystem::exists(owned, error)) {
        std::filesystem::remove_all(owned, error);
        if (error) {
            return false;
        }
    }
    error.clear();
    if (std::filesystem::is_directory(root, error) &&
        !error &&
        std::filesystem::is_empty(root, error) &&
        !error) {
        std::filesystem::remove(root, error);
        if (error) {
            return false;
        }
    }
    return true;
}

std::string utf8_path(
    const std::filesystem::path& path) {
    const auto value = path.u8string();
    return std::string(
        reinterpret_cast<const char*>(value.data()),
        value.size());
}

bool cleanup_external_staging(
    const std::filesystem::path& root,
    const std::string& token) {
    const std::wstring wide(
        token.begin(), token.end());
    const auto work =
        root / (L".act-recording-work-" + wide);
    const auto video =
        root /
        (L".act-recording-stage-" + wide + L".mp4");
    const auto analysis =
        root /
        (L".act-recording-stage-" + wide + L".analysis");
    std::error_code error;
    for (const auto& path : {work, video, analysis}) {
        if (std::filesystem::exists(path, error)) {
            std::filesystem::remove_all(path, error);
            if (error) {
                return false;
            }
        }
        error.clear();
    }
    return true;
}

BackendError process_error(
    const components::WorkerProcessResult& process) {
    using Completion = components::WorkerCompletion;
    switch (process.completion) {
        case Completion::timed_out:
            return BackendError{
                "TIMEOUT",
                "The isolated recording worker timed out."};
        case Completion::cancelled:
            return BackendError{
                "CANCELLED",
                "The isolated recording worker was cancelled."};
        case Completion::unavailable:
            return BackendError{
                "ISOLATED_WORKER_UNAVAILABLE",
                process.error_message};
        case Completion::protocol_failure:
            return BackendError{
                "OPERATION_FAILED",
                process.error_message};
        case Completion::completed:
            break;
    }
    return BackendError{
        "OPERATION_FAILED",
        "The isolated recording worker returned an invalid result."};
}

RecordingWorkerRun run(
    const char* operation,
    const std::uint32_t timeout_ms,
    const std::string& session_id = {}) {
    const std::string token = staging_token();
    components::Json::Object fields{
        {"contractVersion", worker_contract},
        {"operation", operation},
        {"stagingToken", token},
    };
    if (!session_id.empty()) {
        fields.emplace_back("sessionId", session_id);
        fields.emplace_back("confirmed", true);
        fields.emplace_back("durationMs", 1000);
        fields.emplace_back("fps", 2);
        fields.emplace_back("maxWidth", 960);
        fields.emplace_back("crf", 32);
        fields.emplace_back("maxKeyframes", 8);
        fields.emplace_back("changeThreshold", 0.035);
    }
    const components::Json request(std::move(fields));
    const auto process =
        components::WorkerProcess().run_companion(
            worker_name,
            request.dump(),
            timeout_ms,
            4U * 1024U * 1024U);
    const bool cleaned = cleanup_staging(token);
    if (!cleaned) {
        return RecordingWorkerRun{
            std::nullopt,
            BackendError{
                "STAGING_CLEANUP_FAILED",
                "The private recording staging could not be cleaned."},
            process.job_terminated,
            false,
        };
    }
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return RecordingWorkerRun{
            std::nullopt,
            process_error(process),
            process.job_terminated,
            true,
        };
    }
    std::string parse_error;
    auto response = components::Json::parse(
        process.stdout_text, parse_error);
    const auto* contract = response.has_value()
        ? response->find("contractVersion")
        : nullptr;
    const auto* ok = response.has_value()
        ? response->find("ok")
        : nullptr;
    if (!response.has_value() ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr) {
        return RecordingWorkerRun{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated recording worker violated its envelope."},
            process.job_terminated,
            true,
        };
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
            return RecordingWorkerRun{
                std::nullopt,
                BackendError{
                    "OPERATION_FAILED",
                    "The recording worker returned an invalid error."},
                process.job_terminated,
                true,
            };
        }
        return RecordingWorkerRun{
            std::nullopt,
            BackendError{
                *code->string_value(),
                *message->string_value()},
            process.job_terminated,
            true,
        };
    }
    const auto* data = response->find("data");
    if (data == nullptr ||
        data->object_items() == nullptr) {
        return RecordingWorkerRun{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The recording worker omitted result data."},
            process.job_terminated,
            true,
        };
    }
    return RecordingWorkerRun{
        *data,
        std::nullopt,
        process.job_terminated,
        true,
    };
}

RecordingWorkerRun run_stage(
    const std::string& session_id,
    const std::filesystem::path& staging_root,
    const components::RecordingConfig& config,
    const std::uint32_t timeout_ms) {
    std::error_code path_error;
    const auto root = std::filesystem::absolute(
        staging_root, path_error).lexically_normal();
    if (path_error ||
        !std::filesystem::is_directory(root, path_error) ||
        path_error) {
        return RecordingWorkerRun{
            std::nullopt,
            BackendError{
                "INVALID_ARGUMENT",
                "The recording staging root is unavailable."},
            false,
            true,
        };
    }
    const std::string token = staging_token();
    const components::Json request(components::Json::Object{
        {"contractVersion", worker_contract},
        {"operation", "exact-window-recording-stage"},
        {"stagingToken", token},
        {"stagingRoot", utf8_path(root)},
        {"outputPath", config.output_path},
        {"analysisDir", config.analysis_directory},
        {"sessionId", session_id},
        {"confirmed", true},
        {"durationMs",
         static_cast<std::int64_t>(config.duration_ms)},
        {"fps",
         static_cast<std::int64_t>(config.fps)},
        {"maxWidth",
         static_cast<std::int64_t>(
             config.maximum_width)},
        {"crf",
         static_cast<std::int64_t>(config.crf)},
        {"maxKeyframes",
         static_cast<std::int64_t>(
             config.maximum_keyframes)},
        {"changeThreshold", config.change_threshold},
    });
    const auto process =
        components::WorkerProcess().run_companion(
            worker_name,
            request.dump(),
            timeout_ms,
            4U * 1024U * 1024U);
    const auto fail =
        [&](BackendError error) {
            const bool cleaned =
                cleanup_external_staging(root, token);
            return RecordingWorkerRun{
                std::nullopt,
                cleaned
                    ? std::optional<BackendError>(
                          std::move(error))
                    : std::optional<BackendError>(
                          BackendError{
                              "STAGING_CLEANUP_FAILED",
                              "External recording staging cleanup failed."}),
                process.job_terminated,
                cleaned,
            };
        };
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return fail(process_error(process));
    }
    std::string parse_error;
    auto response = components::Json::parse(
        process.stdout_text, parse_error);
    const auto* contract = response.has_value()
        ? response->find("contractVersion")
        : nullptr;
    const auto* ok = response.has_value()
        ? response->find("ok")
        : nullptr;
    const auto* data = response.has_value()
        ? response->find("data")
        : nullptr;
    const auto* preserved =
        data == nullptr ? nullptr : data->find("stagingPreserved");
    if (!response.has_value() ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr ||
        !*ok->bool_value() ||
        data == nullptr ||
        data->object_items() == nullptr ||
        preserved == nullptr ||
        preserved->bool_value() == nullptr ||
        !*preserved->bool_value()) {
        return fail(BackendError{
            "OPERATION_FAILED",
            "The staged recording worker returned an invalid result."});
    }
    const std::wstring wide(token.begin(), token.end());
    const auto video =
        root /
        (L".act-recording-stage-" + wide + L".mp4");
    const auto analysis =
        root /
        (L".act-recording-stage-" + wide + L".analysis");
    if (!std::filesystem::is_regular_file(video, path_error) ||
        path_error ||
        !std::filesystem::is_directory(analysis, path_error) ||
        path_error) {
        return fail(BackendError{
            "OPERATION_FAILED",
            "The staged recording artifacts are incomplete."});
    }
    return RecordingWorkerRun{
        *data,
        std::nullopt,
        process.job_terminated,
        false,
        video,
        analysis,
    };
}

}  // namespace

RecordingWorkerRun RecordingWorkerBackend::run_fixture_encode(
    const std::uint32_t timeout_ms) const {
    return run("fixture-ffmpeg-encode", timeout_ms);
}

RecordingWorkerRun RecordingWorkerBackend::run_timeout_fixture(
    const std::uint32_t timeout_ms) const {
    return run("fixture-hold-after-staging", timeout_ms);
}

RecordingWorkerRun
RecordingWorkerBackend::run_exact_window_candidate(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    return run(
        "exact-window-recording-candidate",
        timeout_ms,
        session_id);
}

RecordingWorkerRun
RecordingWorkerBackend::run_exact_window_stage(
    const std::string& session_id,
    const std::filesystem::path& staging_root,
    const components::RecordingConfig& config,
    const std::uint32_t timeout_ms) const {
    return run_stage(
        session_id, staging_root, config, timeout_ms);
}

bool RecordingWorkerBackend::cleanup_external_stage(
    const std::filesystem::path& staged_video,
    const std::filesystem::path& staged_analysis) const {
    if (staged_video.parent_path() !=
            staged_analysis.parent_path() ||
        !staged_video.filename().wstring().starts_with(
            L".act-recording-stage-") ||
        !staged_analysis.filename().wstring().starts_with(
            L".act-recording-stage-")) {
        return false;
    }
    std::error_code error;
    for (const auto& path :
         {staged_video, staged_analysis}) {
        if (std::filesystem::exists(path, error)) {
            std::filesystem::remove_all(path, error);
            if (error) {
                return false;
            }
        }
        error.clear();
    }
    return true;
}

}  // namespace act::platform::windows
