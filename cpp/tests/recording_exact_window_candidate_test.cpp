#include "components/json.hpp"
#include "components/recording_commit.hpp"
#include "platform/windows/recording_worker_backend.hpp"

#include <windows.h>

#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) {
        std::cerr << "An opaque self-owned window session is required.\n";
        return 2;
    }
    const HWND foreground_before = GetForegroundWindow();
    std::array<wchar_t, 32768> module{};
    const DWORD module_length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    if (module_length == 0U ||
        module_length >= module.size()) {
        return 2;
    }
    const auto staging_root =
        std::filesystem::path(
            std::wstring(module.data(), module_length))
            .parent_path() /
        (L"recording-public-candidate-" +
         std::to_wstring(GetCurrentProcessId()));
    std::error_code cleanup_error;
    std::filesystem::remove_all(
        staging_root, cleanup_error);
    std::filesystem::create_directory(
        staging_root, cleanup_error);
    if (cleanup_error) {
        return 2;
    }
    const act::platform::windows::RecordingWorkerBackend backend;
    const auto stale =
        backend.run_exact_window_candidate(
            "s2:w:0000000000000000", 10000U);
    if (!stale.error.has_value() ||
        stale.error->code != "STALE_SESSION" ||
        !stale.job_terminated ||
        !stale.staging_cleaned) {
        std::cerr << "The recording worker accepted a stale target.\n";
        return 1;
    }
    const auto result =
        backend.run_exact_window_candidate(argv[1], 40000U);
    const auto* data = result.data.has_value()
        ? &*result.data
        : nullptr;
    const auto required_true =
        [data](const char* name) {
            const auto* value =
                data == nullptr ? nullptr : data->find(name);
            return value != nullptr &&
                   value->bool_value() != nullptr &&
                   *value->bool_value();
        };
    const auto required_false =
        [data](const char* name) {
            const auto* value =
                data == nullptr ? nullptr : data->find(name);
            return value != nullptr &&
                   value->bool_value() != nullptr &&
                   !*value->bool_value();
        };
    const auto* frames =
        data == nullptr ? nullptr : data->find("encodedFrames");
    const auto* keyframes =
        data == nullptr ? nullptr : data->find("analysisKeyframes");
    if (result.error.has_value() ||
        !result.job_terminated ||
        !result.staging_cleaned ||
        frames == nullptr ||
        frames->integer_value() == nullptr ||
        *frames->integer_value() != 2 ||
        keyframes == nullptr ||
        keyframes->integer_value() == nullptr ||
        *keyframes->integer_value() < 1 ||
        *keyframes->integer_value() > 4 ||
        !required_true("actualWgcFrames") ||
        !required_true("singleCaptureSession") ||
        !required_true("fixedArguments") ||
        !required_true("mp4SignatureValid") ||
        !required_true("storyboardEncoded") ||
        !required_true("manifestValidated") ||
        !required_true("analysisStagingRemoved") ||
        !required_true("foregroundUnchanged") ||
        !required_false("selfOwnedFixture") ||
        !required_false("audioCaptured") ||
        !required_false("cursorCaptured") ||
        !required_false("runtimePathExposed") ||
        GetForegroundWindow() != foreground_before) {
        std::cerr
            << "The exact-window recording candidate violated policy: "
            << (result.error.has_value()
                    ? result.error->code
                    : "no-error")
            << ", data "
            << (result.data.has_value()
                    ? result.data->dump()
                    : "none")
            << '\n';
        return 1;
    }
    const auto staged =
        backend.run_exact_window_stage(
            argv[1],
            staging_root,
            act::components::RecordingConfig{
                {},
                {},
                1000U,
                2U,
                960U,
                32U,
                8U,
                0.035,
                5000U,
                2U,
                false,
            },
            40000U);
    if (staged.error.has_value() ||
        !staged.job_terminated ||
        staged.staging_cleaned ||
        !staged.staged_video.has_value() ||
        !staged.staged_analysis.has_value()) {
        std::filesystem::remove_all(
            staging_root, cleanup_error);
        std::cerr
            << "The exact-window worker did not preserve owned staging.\n";
        return 1;
    }
    const auto commit =
        act::components::commit_recording_artifacts(
            act::components::RecordingCommitPlan{
                *staged.staged_video,
                *staged.staged_analysis,
                staging_root / L"evidence.mp4",
                staging_root / L"evidence.analysis",
                false,
            });
    const bool committed =
        commit.evidence.has_value() &&
        std::filesystem::is_regular_file(
            staging_root / L"evidence.mp4") &&
        std::filesystem::is_regular_file(
            staging_root /
            L"evidence.analysis" /
            L"storyboard.png") &&
        std::filesystem::is_regular_file(
            staging_root /
            L"evidence.analysis" /
            L"manifest.json") &&
        !std::filesystem::exists(
            *staged.staged_video) &&
        !std::filesystem::exists(
            *staged.staged_analysis);
    std::filesystem::remove_all(
        staging_root, cleanup_error);
    if (!committed || cleanup_error) {
        std::cerr
            << "The exact-window staged artifacts did not commit.\n";
        return 1;
    }
    std::cout << act::components::object({
        {"ok", true},
        {"exactOpaqueTarget", true},
        {"staleTargetRefused", true},
        {"actualWgcFrames", true},
        {"actualFfmpegEncode", true},
        {"analysisArtifacts", true},
        {"atomicPublicCommit", true},
        {"stagingCleaned", true},
        {"foregroundUnchanged", true},
        {"userApplicationsRecorded", 0},
    }).dump() << '\n';
    return 0;
}
#include <array>
#include <filesystem>
