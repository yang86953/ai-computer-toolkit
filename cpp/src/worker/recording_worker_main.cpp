#include "components/json.hpp"
// 复用主进程相同的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "components/recording_analysis.hpp"
#include "components/recording_config.hpp"
#include "platform/windows/capture_preflight_backend.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/ffmpeg_encoder.hpp"
#include "platform/windows/png_encoder.hpp"
#include "platform/windows/recording_stream_pipeline.hpp"
#include "platform/windows/text_codec.hpp"
#include "platform/windows/wgc_capture.hpp"

#include <windows.h>

#include <array>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <sstream>
#include <string>

namespace {

constexpr const char* worker_contract =
    "act/recording-worker/v1";

int write_failure(
    const char* code,
    const std::string& message) {
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

bool valid_staging_token(const std::string& token) {
    if (token.size() < 3U || token.size() > 64U) {
        return false;
    }
    for (const unsigned char character : token) {
        if (!((character >= 'a' && character <= 'z') ||
              (character >= '0' && character <= '9') ||
              character == '-')) {
            return false;
        }
    }
    return true;
}

std::optional<std::filesystem::path> fixture_directory(
    const std::string& token,
    const std::optional<std::filesystem::path>&
        external_root = std::nullopt) {
    if (!valid_staging_token(token)) {
        return std::nullopt;
    }
    std::filesystem::path root;
    if (external_root.has_value()) {
        root = *external_root;
    } else {
        std::array<wchar_t, 32768> module{};
        const DWORD length = GetModuleFileNameW(
            nullptr,
            module.data(),
            static_cast<DWORD>(module.size()));
        if (length == 0U || length >= module.size()) {
            return std::nullopt;
        }
        root = std::filesystem::path(
                   std::wstring(module.data(), length))
                   .parent_path() /
            L"recording-worker-staging";
    }
    std::error_code error;
    if (!std::filesystem::exists(root, error)) {
        std::filesystem::create_directory(root, error);
    }
    if (error ||
        !std::filesystem::is_directory(root, error) ||
        error ||
        std::filesystem::is_symlink(
            std::filesystem::symlink_status(root, error)) ||
        error) {
        return std::nullopt;
    }
    const auto directory = root /
        (external_root.has_value()
             ? std::filesystem::path(
                   L".act-recording-work-" +
                   std::wstring(token.begin(), token.end()))
             : std::filesystem::path(token));
    if (std::filesystem::exists(directory, error) ||
        !std::filesystem::create_directory(directory, error) ||
        error) {
        return std::nullopt;
    }
    return directory;
}

bool write_binary(
    const std::filesystem::path& path,
    const std::vector<std::uint8_t>& bytes) {
    std::ofstream output(path, std::ios::binary);
    output.write(
        reinterpret_cast<const char*>(bytes.data()),
        static_cast<std::streamsize>(bytes.size()));
    return static_cast<bool>(output);
}

struct RecordingRunConfig {
    std::uint32_t frame_count;
    std::uint32_t duration_ms;
    std::uint32_t interval_ms;
    std::uint32_t fps;
    std::uint32_t maximum_width;
    std::uint32_t crf;
    std::uint32_t maximum_keyframes;
    double change_threshold;
    std::string output_path;
    std::string analysis_directory;
};

std::wstring keyframe_name(
    const std::size_t index,
    const std::uint64_t timestamp_ms) {
    std::wostringstream name;
    name << L"frame-" << std::setfill(L'0')
         << std::setw(2) << index + 1U
         << L"-" << std::setw(6) << timestamp_ms
         << L"ms.png";
    return name.str();
}

int encode_recording(
    const std::string& token,
    const std::optional<std::uintptr_t> native_window,
    const RecordingRunConfig& config,
    const std::optional<std::filesystem::path>&
        external_root = std::nullopt,
    const std::optional<std::pair<
        std::int32_t, std::int32_t>>&
        source_dimensions = std::nullopt) {
    const auto directory =
        fixture_directory(token, external_root);
    if (!directory.has_value()) {
        return write_failure(
            "VIDEO_ENCODER_START_FAILED",
            "The private recording staging directory is unavailable.");
    }
    const HWND foreground_before = GetForegroundWindow();
    act::platform::windows::FfmpegEncodeEvidence evidence;
    act::components::RecordingAnalysis analysis;
    std::int32_t width = 0;
    std::int32_t height = 0;
    std::int32_t source_width = 0;
    std::int32_t source_height = 0;
    std::uint32_t captured_frames = 0U;
    std::string device_driver;
    bool fixture_owned = false;
    bool single_session = false;
    if (external_root.has_value()) {
        if (!native_window.has_value() ||
            !source_dimensions.has_value()) {
            std::error_code cleanup_error;
            std::filesystem::remove(
                *directory, cleanup_error);
            return write_failure(
                "INVALID_ARGUMENT",
                "The streaming recording requires exact preflight dimensions.");
        }
        auto streamed =
            act::platform::windows::
                run_recording_stream_pipeline(
                    *native_window,
                    source_dimensions->first,
                    source_dimensions->second,
                    *directory,
                    act::platform::windows::
                        RecordingStreamPipelineConfig{
                            config.frame_count,
                            config.interval_ms,
                            config.fps,
                            config.maximum_width,
                            config.crf,
                            config.maximum_keyframes,
                            config.change_threshold,
                        });
        if (streamed.error.has_value()) {
            std::error_code cleanup_error;
            std::filesystem::remove_all(
                *directory, cleanup_error);
            return write_failure(
                streamed.error->code.c_str(),
                streamed.error->message);
        }
        auto& value = *streamed.evidence;
        evidence = std::move(value.encode);
        analysis = std::move(value.analysis);
        width = value.width;
        height = value.height;
        source_width = source_dimensions->first;
        source_height = source_dimensions->second;
        captured_frames = value.captured_frames;
        device_driver = value.device_driver;
        single_session = value.single_capture_session;
    } else {
        auto captured = native_window.has_value()
            ? act::platform::windows::
                  capture_window_recording_frames(
                      *native_window,
                      config.frame_count,
                      config.interval_ms)
            : act::platform::windows::
                  capture_fixture_recording_frames(
                      config.frame_count,
                      config.interval_ms);
        if (captured.error.has_value()) {
            std::error_code cleanup_error;
            std::filesystem::remove(
                *directory, cleanup_error);
            return write_failure(
                captured.error->code.c_str(),
                captured.error->message);
        }
        auto& sequence = *captured.sequence;
        if (sequence.width <= 0 ||
            sequence.height <= 0 ||
            sequence.fixture_owned_by_toolkit !=
                !native_window.has_value() ||
            !sequence.single_capture_session ||
            !sequence.foreground_unchanged) {
            std::error_code cleanup_error;
            std::filesystem::remove(
                *directory, cleanup_error);
            return write_failure(
                "HOST_INTERFERENCE_DETECTED",
                "The WGC sequence violated its safety boundary.");
        }
        const auto output_dimensions =
            act::components::recording_output_dimensions(
                static_cast<std::uint32_t>(sequence.width),
                static_cast<std::uint32_t>(sequence.height),
                config.maximum_width);
        if (!output_dimensions.has_value()) {
            std::error_code cleanup_error;
            std::filesystem::remove(
                *directory, cleanup_error);
            return write_failure(
                "CAPTURE_TARGET_FAILED",
                "The recording target dimensions are invalid.");
        }
        for (auto& frame : sequence.frames) {
            if (frame.width != output_dimensions->first ||
                frame.height != output_dimensions->second) {
                auto resized =
                    act::components::resize_rgba_triangle(
                        frame,
                        output_dimensions->first,
                        output_dimensions->second);
                if (!resized.has_value()) {
                    std::error_code cleanup_error;
                    std::filesystem::remove(
                        *directory, cleanup_error);
                    return write_failure(
                        "CAPTURE_READBACK_FAILED",
                        "A recording frame could not be resized.");
                }
                frame = std::move(*resized);
            }
        }
        auto encoded =
            act::platform::windows::encode_fixture_video(
                act::platform::windows::FfmpegEncodeConfig{
                    output_dimensions->first,
                    output_dimensions->second,
                    config.fps,
                    config.crf,
                    30000U,
                    *directory,
                },
                sequence.frames);
        auto analyzed =
            act::components::analyze_recording_frames(
                sequence.frames,
                config.fps,
                config.maximum_keyframes,
                config.change_threshold);
        if (encoded.error.has_value() ||
            !analyzed.analysis.has_value()) {
            std::error_code cleanup_error;
            std::filesystem::remove_all(
                *directory, cleanup_error);
            return encoded.error.has_value()
                ? write_failure(
                      encoded.error->code.c_str(),
                      encoded.error->message)
                : write_failure(
                      analyzed.error_code.c_str(),
                      analyzed.error_message);
        }
        evidence = std::move(*encoded.evidence);
        analysis = std::move(*analyzed.analysis);
        width = static_cast<std::int32_t>(
            output_dimensions->first);
        height = static_cast<std::int32_t>(
            output_dimensions->second);
        source_width = sequence.width;
        source_height = sequence.height;
        device_driver = sequence.device_driver;
        fixture_owned = sequence.fixture_owned_by_toolkit;
        single_session = sequence.single_capture_session;
        captured_frames =
            static_cast<std::uint32_t>(
                sequence.frames.size());
    }
    const auto analysis_directory =
        *directory / L"analysis";
    std::error_code error;
    if (std::filesystem::exists(
            analysis_directory, error) ||
        !std::filesystem::create_directory(
            analysis_directory, error) ||
        error) {
        std::filesystem::remove(evidence.output_path, error);
        std::filesystem::remove(*directory, error);
        return write_failure(
            "VIDEO_ANALYSIS_WRITE_FAILED",
            "The private analysis staging directory is unavailable.");
    }
    std::vector<std::filesystem::path> analysis_files;
    bool analysis_written = true;
    for (std::size_t index = 0U;
         index < analysis.keyframes.size();
         ++index) {
        const auto& keyframe =
            analysis.keyframes[index];
        const auto encoded =
            act::platform::windows::encode_rgba_png(
                keyframe.image.width,
                keyframe.image.height,
                keyframe.image.pixels);
        const auto filename = keyframe_name(
            index, keyframe.timestamp_ms);
        const auto path = analysis_directory / filename;
        if (!encoded.image.has_value() ||
            !write_binary(path, encoded.image->bytes)) {
            analysis_written = false;
            break;
        }
        analysis_files.push_back(path);
    }
    const auto storyboard_path =
        analysis_directory / L"storyboard.png";
    const auto storyboard =
        act::platform::windows::encode_rgba_png(
            analysis.storyboard.width,
            analysis.storyboard.height,
            analysis.storyboard.pixels);
    if (!storyboard.image.has_value() ||
        !write_binary(
            storyboard_path, storyboard.image->bytes)) {
        analysis_written = false;
    } else {
        analysis_files.push_back(storyboard_path);
    }
    act::components::Json::Array selected;
    for (std::size_t index = 0U;
         index < analysis.keyframes.size();
         ++index) {
        const auto& keyframe =
            analysis.keyframes[index];
        const auto filename = keyframe_name(
            index, keyframe.timestamp_ms);
        const auto staged_path =
            analysis_directory / filename;
        selected.push_back(act::components::object({
            {"index",
             static_cast<std::int64_t>(index + 1U)},
            {"frameIndex",
             static_cast<std::int64_t>(
                 keyframe.frame_index)},
            {"timestampMs",
             static_cast<std::int64_t>(
                 keyframe.timestamp_ms)},
            {"changeScore", keyframe.change_score},
            {"path",
             config.analysis_directory.empty()
                 ? act::platform::windows::utf8(
                       staged_path.wstring())
                 : act::platform::windows::utf8(
                       (std::filesystem::path(
                            act::platform::windows::wide(
                                config.analysis_directory)) /
                        filename)
                           .wstring())},
        }));
    }
    const auto manifest = act::components::object({
        {"schemaVersion", 1},
        {"video",
         act::components::object({
             {"path", config.output_path},
             {"bytes",
              static_cast<std::int64_t>(
                  evidence.output_bytes)},
             {"codec", "H.264"},
             {"container", "MP4"},
             {"fps",
              static_cast<std::int64_t>(config.fps)},
             {"durationMs",
              static_cast<std::int64_t>(
                  config.duration_ms)},
             {"sourceWidth", source_width},
             {"sourceHeight", source_height},
             {"width", width},
             {"height", height},
             {"crf",
              static_cast<std::int64_t>(config.crf)},
             {"encodedFrames",
              static_cast<std::int64_t>(
                  config.frame_count)},
             {"capturedFrames",
              static_cast<std::int64_t>(
                  captured_frames)},
             {"audioCaptured", false},
             {"cursorCaptured", false},
         })},
        {"analysis",
         act::components::object({
             {"strategy",
              "temporal-difference-keyframes"},
             {"changeThreshold",
              config.change_threshold},
             {"maxKeyframes",
              static_cast<std::int64_t>(
                  config.maximum_keyframes)},
             {"selectedKeyframes",
              act::components::Json(std::move(selected))},
             {"storyboardPath",
              config.analysis_directory.empty()
                  ? act::platform::windows::utf8(
                        storyboard_path.wstring())
                  : act::platform::windows::utf8(
                        (std::filesystem::path(
                             act::platform::windows::wide(
                                 config.analysis_directory)) /
                         L"storyboard.png")
                            .wstring())},
             {"aiInputPolicy",
              "先分析 storyboard；只有细节不足时才读取 "
              "selectedKeyframes 中的单帧，不把完整视频逐帧送入模型。"},
         })},
    });
    const auto manifest_path =
        analysis_directory / L"manifest.json";
    {
        std::ofstream output(manifest_path, std::ios::binary);
        output << manifest.dump_pretty() << '\n';
        analysis_written =
            analysis_written && static_cast<bool>(output);
    }
    analysis_files.push_back(manifest_path);
    std::string parse_error;
    const auto manifest_check =
        act::components::Json::parse(
            manifest.dump(), parse_error);
    const bool manifest_valid =
        manifest_check.has_value();
    bool analysis_directory_removed = false;
    bool output_removed = false;
    bool directory_removed = false;
    bool staging_root_removed = false;
    bool staging_preserved = false;
    if (external_root.has_value() &&
        analysis_written &&
        manifest_valid) {
        const std::wstring wide_token(
            token.begin(), token.end());
        const auto staged_video =
            *external_root /
            (L".act-recording-stage-" +
             wide_token + L".mp4");
        const auto staged_analysis =
            *external_root /
            (L".act-recording-stage-" +
             wide_token + L".analysis");
        if (std::filesystem::exists(staged_video, error) ||
            std::filesystem::exists(
                staged_analysis, error)) {
            analysis_written = false;
        } else {
            std::filesystem::rename(
                evidence.output_path,
                staged_video,
                error);
            if (error) {
                analysis_written = false;
            } else {
                error.clear();
                std::filesystem::rename(
                    analysis_directory,
                    staged_analysis,
                    error);
                if (error) {
                    std::error_code restore_error;
                    std::filesystem::rename(
                        staged_video,
                        evidence.output_path,
                        restore_error);
                    analysis_written = false;
                } else {
                    staging_preserved = true;
                }
            }
        }
        error.clear();
        directory_removed =
            std::filesystem::remove(*directory, error) &&
            !error;
    } else {
        for (const auto& path : analysis_files) {
            error.clear();
            if (!std::filesystem::remove(path, error) ||
                error) {
                analysis_written = false;
            }
        }
        error.clear();
        analysis_directory_removed =
            std::filesystem::remove(
                analysis_directory, error) &&
            !error;
        error.clear();
        output_removed =
            std::filesystem::remove(
                evidence.output_path, error) &&
            !error;
        error.clear();
        directory_removed =
            std::filesystem::remove(*directory, error) &&
            !error;
        error.clear();
        staging_root_removed =
            std::filesystem::remove(
                directory->parent_path(), error) &&
            !error;
    }
    const bool foreground_unchanged =
        foreground_before == GetForegroundWindow();
    if (!analysis_written ||
        !manifest_valid ||
        (external_root.has_value()
             ? !staging_preserved
             : (!analysis_directory_removed ||
                !output_removed)) ||
        !directory_removed ||
        !foreground_unchanged) {
        return write_failure(
            foreground_unchanged
                ? "STAGING_CLEANUP_FAILED"
                : "HOST_INTERFERENCE_DETECTED",
            "The fixture encoder did not preserve its cleanup or "
            "foreground invariant.");
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"encodedFrames",
              static_cast<std::int64_t>(
                  evidence.encoded_frames)},
             {"capturedFrames",
              static_cast<std::int64_t>(
                  captured_frames)},
             {"actualWgcFrames", true},
             {"selfOwnedFixture",
              fixture_owned},
             {"singleCaptureSession",
              single_session},
             {"width", width},
             {"height", height},
             {"sourceWidth", source_width},
             {"sourceHeight", source_height},
             {"deviceDriver",
              device_driver},
             {"analysisKeyframes",
              static_cast<std::int64_t>(
                  analysis.keyframes.size())},
             {"storyboardEncoded",
              storyboard.image.has_value()},
             {"manifestValidated", manifest_valid},
             {"analysisStagingRemoved",
              analysis_directory_removed},
             {"stagingPreserved", staging_preserved},
             {"outputBytes",
              static_cast<std::int64_t>(
                  evidence.output_bytes)},
             {"fixedArguments", evidence.fixed_arguments},
             {"h264Requested", evidence.h264_requested},
             {"mp4SignatureValid",
              evidence.mp4_signature_valid},
             {"audioCaptured", evidence.audio_captured},
             {"cursorCaptured", evidence.cursor_captured},
             {"rawStagingRemoved",
              evidence.raw_staging_removed},
             {"outputStagingRemoved", output_removed},
             {"stagingDirectoryRemoved",
              directory_removed},
             {"stagingRootRemoved",
              staging_root_removed},
             {"foregroundUnchanged",
              foreground_unchanged},
             {"runtimePathExposed", false},
         })},
    }).dump() << '\n';
    return 0;
}

int hold_fixture(const std::string& token) {
    const auto directory = fixture_directory(token);
    if (!directory.has_value()) {
        return write_failure(
            "VIDEO_ENCODER_START_FAILED",
            "The private timeout staging directory is unavailable.");
    }
    const auto marker = *directory / L"owned.marker";
    std::ofstream output(marker, std::ios::binary);
    output << "owned";
    output.close();
    Sleep(60000U);
    return write_failure(
        "OPERATION_FAILED",
        "The timeout fixture unexpectedly completed.");
}

std::optional<RecordingRunConfig> candidate_config(
    const act::components::Json& request) {
    const auto integer =
        [&request](const char* name)
        -> std::optional<std::uint32_t> {
            const auto* value = request.find(name);
            if (value == nullptr ||
                value->integer_value() == nullptr ||
                *value->integer_value() < 0 ||
                *value->integer_value() >
                    std::numeric_limits<std::uint32_t>::max()) {
                return std::nullopt;
            }
            return static_cast<std::uint32_t>(
                *value->integer_value());
        };
    const auto duration = integer("durationMs");
    const auto fps = integer("fps");
    const auto maximum_width = integer("maxWidth");
    const auto crf = integer("crf");
    const auto maximum_keyframes =
        integer("maxKeyframes");
    const auto* output_path = request.find("outputPath");
    const auto* analysis_directory =
        request.find("analysisDir");
    const auto* threshold_value =
        request.find("changeThreshold");
    double threshold = 0.0;
    if (threshold_value != nullptr &&
        threshold_value->double_value() != nullptr) {
        threshold = *threshold_value->double_value();
    } else if (
        threshold_value != nullptr &&
        threshold_value->integer_value() != nullptr) {
        threshold = static_cast<double>(
            *threshold_value->integer_value());
    } else {
        return std::nullopt;
    }
    if (!duration.has_value() ||
        *duration < 1000U ||
        *duration > 300000U ||
        !fps.has_value() ||
        *fps < 1U ||
        *fps > 10U ||
        !maximum_width.has_value() ||
        *maximum_width < 320U ||
        *maximum_width > 1920U ||
        !crf.has_value() ||
        *crf < 18U ||
        *crf > 40U ||
        !maximum_keyframes.has_value() ||
        *maximum_keyframes < 2U ||
        *maximum_keyframes > 20U ||
        !std::isfinite(threshold) ||
        threshold < 0.005 ||
        threshold > 0.5) {
        return std::nullopt;
    }
    const std::uint64_t frames =
        (static_cast<std::uint64_t>(*duration) *
             *fps +
         999U) /
        1000U;
    if (frames < 2U || frames > 3000U) {
        return std::nullopt;
    }
    return RecordingRunConfig{
        static_cast<std::uint32_t>(frames),
        *duration,
        1000U / *fps,
        *fps,
        *maximum_width,
        *crf,
        *maximum_keyframes,
        threshold,
        output_path != nullptr &&
                output_path->string_value() != nullptr
            ? *output_path->string_value()
            : std::string{},
        analysis_directory != nullptr &&
                analysis_directory->string_value() != nullptr
            ? *analysis_directory->string_value()
            : std::string{},
    };
}

int encode_exact_window_candidate(
    const act::components::Json& request,
    const std::string& token,
    const bool preserve_staging) {
    const auto* confirmed = request.find("confirmed");
    if (confirmed == nullptr ||
        confirmed->bool_value() == nullptr ||
        !*confirmed->bool_value()) {
        return write_failure(
            "CONFIRMATION_REQUIRED",
            "Exact-window recording requires explicit confirmation.");
    }
    const auto config = candidate_config(request);
    if (!config.has_value()) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The exact-window recording candidate config is invalid.");
    }
    std::optional<std::filesystem::path> external_root;
    if (preserve_staging) {
        const auto* root = request.find("stagingRoot");
        if (root == nullptr ||
            root->string_value() == nullptr ||
            root->string_value()->empty()) {
            return write_failure(
                "INVALID_ARGUMENT",
                "The staged recording candidate requires a root.");
        }
        std::error_code error;
        external_root = std::filesystem::absolute(
            std::filesystem::path(
                act::platform::windows::wide(
                    *root->string_value())),
            error).lexically_normal();
        if (error ||
            !std::filesystem::is_directory(
                *external_root, error) ||
            error ||
            std::filesystem::is_symlink(
                std::filesystem::symlink_status(
                    *external_root, error)) ||
            error) {
            return write_failure(
                "INVALID_ARGUMENT",
                "The staged recording root is not a real directory.");
        }
    }
    const auto* session_id = request.find("sessionId");
    if (session_id == nullptr ||
        session_id->string_value() == nullptr ||
        session_id->string_value()->empty()) {
        return write_failure(
            "INVALID_ARGUMENT",
            "Exact-window recording requires an opaque sessionId.");
    }
    const act::platform::windows::DiscoveryBackend discovery;
    const auto windows = discovery.enumerate_windows(4096U);
    // 完整扫描 worker 当前窗口清单以检测碰撞。
    const auto match = act::components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [session_id](const auto& window) {
            return window.session_id ==
                *session_id->string_value();
        });
    // 非唯一命中时不允许 worker 录制任意窗口。
    if (match.state != act::components::OpaqueTargetMatchState::unique) {
        return write_failure(
            match.state == act::components::OpaqueTargetMatchState::ambiguous ? "AMBIGUOUS_TARGET" : "STALE_SESSION",
            match.state == act::components::OpaqueTargetMatchState::ambiguous ? "The opaque recording target resolves to multiple windows." : "The opaque recording target no longer resolves.");
    }
    const act::platform::windows::CapturePreflightBackend preflight;
    const auto readiness = preflight.inspect(*match.position);
    if (readiness.error.has_value()) {
        return write_failure(
            readiness.error->code.c_str(),
            readiness.error->message);
    }
    if (readiness.preflight->eligibility !=
        "eligible-for-certified-capture-route") {
        return write_failure(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact recording target is not eligible: " +
                readiness.preflight->eligibility + ".");
    }
    return encode_recording(
        token,
        match.position->native_window,
        *config,
        external_root,
        std::pair{
            readiness.preflight->capture_width,
            readiness.preflight->capture_height});
}

}  // namespace

int main() {
    std::string request_text;
    if (!std::getline(std::cin, request_text)) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The recording worker requires one JSON request.");
    }
    std::string parse_error;
    const auto request = act::components::Json::parse(
        request_text, parse_error);
    const auto* contract = request.has_value()
        ? request->find("contractVersion")
        : nullptr;
    const auto* operation = request.has_value()
        ? request->find("operation")
        : nullptr;
    if (!request.has_value() ||
        request->object_items() == nullptr ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        operation == nullptr ||
        operation->string_value() == nullptr) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The recording worker request violates protocol v1.");
    }
    const auto* token = request->find("stagingToken");
    if (token == nullptr ||
        token->string_value() == nullptr ||
        !valid_staging_token(*token->string_value())) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The recording worker requires an internal staging token.");
    }
    if (*operation->string_value() ==
        "fixture-hold-after-staging") {
        return hold_fixture(*token->string_value());
    }
    if (*operation->string_value() ==
            "exact-window-recording-candidate" ||
        *operation->string_value() ==
            "exact-window-recording-stage") {
        return encode_exact_window_candidate(
            *request,
            *token->string_value(),
            *operation->string_value() ==
                "exact-window-recording-stage");
    }
    if (*operation->string_value() !=
        "fixture-ffmpeg-encode") {
        return write_failure(
            "INVALID_ARGUMENT",
            "The recording worker operation is not published.");
    }
    return encode_recording(
        *token->string_value(),
        std::nullopt,
        RecordingRunConfig{
            4U,
            2000U,
            80U,
            2U,
            960U,
            32U,
            4U,
            0.035,
            {},
            {}});
}
