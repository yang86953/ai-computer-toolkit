#include "components/json.hpp"
#include "platform/windows/recording_worker_backend.hpp"

#include <windows.h>

#include <array>
#include <filesystem>
#include <iostream>
#include <string>

std::filesystem::path module_directory() {
    std::array<wchar_t, 32768> module{};
    const DWORD length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    return length == 0U || length >= module.size()
        ? std::filesystem::path{}
        : std::filesystem::path(
              std::wstring(module.data(), length))
              .parent_path();
}

int main() {
    const HWND foreground_before = GetForegroundWindow();
    const act::platform::windows::RecordingWorkerBackend backend;
    const auto timeout =
        backend.run_timeout_fixture(500U);
    if (!timeout.error.has_value() ||
        timeout.error->code != "TIMEOUT" ||
        !timeout.job_terminated ||
        !timeout.staging_cleaned) {
        std::cerr
            << "The recording timeout did not clean its full Job.\n";
        return 1;
    }
    const auto normal =
        backend.run_fixture_encode(40000U);
    const auto* data = normal.data.has_value()
        ? &*normal.data
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
    const auto* bytes =
        data == nullptr ? nullptr : data->find("outputBytes");
    const auto* keyframes =
        data == nullptr ? nullptr : data->find("analysisKeyframes");
    const auto fixture_directory =
        module_directory() / L"recording-worker-staging";
    if (normal.error.has_value() ||
        !normal.job_terminated ||
        !normal.staging_cleaned ||
        frames == nullptr ||
        frames->integer_value() == nullptr ||
        *frames->integer_value() != 4 ||
        bytes == nullptr ||
        bytes->integer_value() == nullptr ||
        *bytes->integer_value() <= 12 ||
        keyframes == nullptr ||
        keyframes->integer_value() == nullptr ||
        *keyframes->integer_value() < 2 ||
        *keyframes->integer_value() > 4 ||
        !required_true("actualWgcFrames") ||
        !required_true("singleCaptureSession") ||
        !required_true("fixedArguments") ||
        !required_true("h264Requested") ||
        !required_true("mp4SignatureValid") ||
        !required_true("storyboardEncoded") ||
        !required_true("manifestValidated") ||
        !required_true("analysisStagingRemoved") ||
        !required_true("rawStagingRemoved") ||
        !required_true("outputStagingRemoved") ||
        !required_true("stagingDirectoryRemoved") ||
        !required_true("foregroundUnchanged") ||
        !required_false("audioCaptured") ||
        !required_false("cursorCaptured") ||
        !required_false("runtimePathExposed") ||
        module_directory().empty() ||
        std::filesystem::exists(fixture_directory) ||
        GetForegroundWindow() != foreground_before) {
        std::cerr
            << "The recording worker violated its private staging "
               "contract.\n";
        return 1;
    }
    std::cout << act::components::object({
        {"ok", true},
        {"actualFfmpegEncode", true},
        {"actualWgcFrames", true},
        {"singleCaptureSession", true},
        {"fixedArguments", true},
        {"h264Requested", true},
        {"mp4SignatureValid", true},
        {"encodedFrames", 4},
        {"analysisArtifacts", true},
        {"audioCaptured", false},
        {"cursorCaptured", false},
        {"stagingCleaned", true},
        {"foregroundUnchanged", true},
        {"runtimePathExposed", false},
        {"workerJobTerminated", true},
        {"timeoutJobTerminated", true},
        {"timeoutStagingCleaned", true},
    }).dump() << '\n';
    return 0;
}
