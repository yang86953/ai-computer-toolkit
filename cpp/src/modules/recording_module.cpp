#include "modules/recording_module.hpp"

// 复用完整扫描的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "components/recording_commit.hpp"
#include "components/recording_config.hpp"

// 提供录制 worker 超时上限计算。
#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <optional>

namespace act::modules {
namespace {

ModuleResult failure(
    const std::string& code,
    const std::string& message,
    std::optional<components::Json> details = std::nullopt) {
    return ModuleResult{
        false,
        code,
        message,
        nullptr,
        std::move(details),
    };
}

std::filesystem::path filesystem_path(
    const std::string& value) {
    const std::u8string utf8(
        reinterpret_cast<const char8_t*>(value.data()),
        value.size());
    return std::filesystem::path(utf8);
}

const std::int64_t* required_integer(
    const components::Json& data,
    const char* name) {
    const auto* value = data.find(name);
    return value == nullptr
        ? nullptr
        : value->integer_value();
}

const std::string* required_string(
    const components::Json& data,
    const char* name) {
    const auto* value = data.find(name);
    return value == nullptr
        ? nullptr
        : value->string_value();
}

const bool* required_boolean(
    const components::Json& data,
    const char* name) {
    const auto* value = data.find(name);
    return value == nullptr
        ? nullptr
        : value->bool_value();
}

bool valid_worker_evidence(
    const components::Json& data) {
    const auto* frames =
        required_integer(data, "encodedFrames");
    const auto* width = required_integer(data, "width");
    const auto* height = required_integer(data, "height");
    const auto* source_width =
        required_integer(data, "sourceWidth");
    const auto* source_height =
        required_integer(data, "sourceHeight");
    const auto* captured =
        required_integer(data, "capturedFrames");
    const auto* keyframes =
        required_integer(data, "analysisKeyframes");
    const auto* driver =
        required_string(data, "deviceDriver");
    const auto* actual =
        required_boolean(data, "actualWgcFrames");
    const auto* session =
        required_boolean(data, "singleCaptureSession");
    const auto* signature =
        required_boolean(data, "mp4SignatureValid");
    const auto* manifest =
        required_boolean(data, "manifestValidated");
    const auto* audio =
        required_boolean(data, "audioCaptured");
    const auto* cursor =
        required_boolean(data, "cursorCaptured");
    const auto* foreground =
        required_boolean(data, "foregroundUnchanged");
    return frames != nullptr && *frames > 0 &&
           width != nullptr && *width > 0 &&
           height != nullptr && *height > 0 &&
           source_width != nullptr && *source_width > 0 &&
           source_height != nullptr && *source_height > 0 &&
           captured != nullptr && *captured > 0 &&
           keyframes != nullptr && *keyframes > 0 &&
           driver != nullptr && !driver->empty() &&
           actual != nullptr && *actual &&
           session != nullptr && *session &&
           signature != nullptr && *signature &&
           manifest != nullptr && *manifest &&
           audio != nullptr && !*audio &&
           cursor != nullptr && !*cursor &&
           foreground != nullptr && *foreground;
}

}  // namespace

ModuleResult RecordingModule::record(
    const std::string& session_id,
    const components::Json& input,
    const bool confirmed) const {
    if (!confirmed) {
        return failure(
            "CONFIRMATION_REQUIRED",
            "Window recording requires explicit confirmation.");
    }

    const auto parsed =
        components::parse_recording_config(input);
    if (!parsed.config.has_value()) {
        return failure(
            parsed.error_code, parsed.error_message);
    }
    const auto& config = *parsed.config;

    const auto foreground_before =
        discovery_.foreground_token();
    const auto windows = discovery_.enumerate_windows(4096U);
    // 完整扫描窗口清单以检测 sessionId 碰撞。
    const auto match = components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [&session_id](const auto& window) {
            return window.session_id == session_id;
        });
    // 多命中时不允许录制任意窗口。
    if (match.state == components::OpaqueTargetMatchState::ambiguous) {
        // 在预检和 worker 启动前返回稳定歧义错误。
        return failure(
            "AMBIGUOUS_TARGET",
            "The opaque application-window session resolves to multiple windows.");
    }
    // 零命中保持现有过期会话错误。
    if (match.state == components::OpaqueTargetMatchState::missing) {
        return failure(
            "STALE_SESSION",
            "The opaque application-window session no longer resolves.");
    }

    // 唯一命中后才把窗口交给录制预检。
    const auto readiness = preflight_.inspect(*match.position);
    if (readiness.error.has_value()) {
        return failure(
            readiness.error->code,
            readiness.error->message);
    }
    if (readiness.preflight->eligibility !=
        "eligible-for-certified-capture-route") {
        return failure(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact target is not eligible for background recording: " +
                readiness.preflight->eligibility + ".");
    }

    const auto output =
        filesystem_path(config.output_path);
    const auto analysis =
        filesystem_path(config.analysis_directory);
    const std::uint64_t timeout_wide =
        static_cast<std::uint64_t>(config.duration_ms) +
        static_cast<std::uint64_t>(config.frame_timeout_ms) +
        30000U;
    const auto timeout = static_cast<std::uint32_t>(
        std::min<std::uint64_t>(timeout_wide, 360000U));
    auto staged = worker_.run_exact_window_stage(
        session_id,
        output.parent_path(),
        config,
        timeout);
    const auto cleanup =
        [&staged, this]() {
            if (!staged.staged_video.has_value() ||
                !staged.staged_analysis.has_value()) {
                return staged.staging_cleaned;
            }
            return worker_.cleanup_external_stage(
                *staged.staged_video,
                *staged.staged_analysis);
        };
    if (staged.error.has_value()) {
        return failure(
            staged.error->code,
            staged.error->message);
    }
    if (!staged.data.has_value() ||
        !staged.staged_video.has_value() ||
        !staged.staged_analysis.has_value() ||
        !valid_worker_evidence(*staged.data)) {
        const bool cleaned = cleanup();
        return failure(
            cleaned
                ? "OPERATION_FAILED"
                : "STAGING_CLEANUP_FAILED",
            cleaned
                ? "The isolated recording worker returned incomplete evidence."
                : "Incomplete recording evidence and staging cleanup failed.");
    }

    if (discovery_.foreground_token() !=
        foreground_before) {
        const bool cleaned = cleanup();
        return failure(
            cleaned
                ? "HOST_INTERFERENCE_DETECTED"
                : "STAGING_CLEANUP_FAILED",
            cleaned
                ? "Foreground changed before recording artifacts were committed."
                : "Foreground changed and recording staging cleanup failed.");
    }

    const auto committed =
        components::commit_recording_artifacts(
            components::RecordingCommitPlan{
                *staged.staged_video,
                *staged.staged_analysis,
                output,
                analysis,
                config.overwrite,
            });
    if (!committed.evidence.has_value()) {
        const bool cleaned = cleanup();
        return failure(
            cleaned
                ? committed.error_code
                : "STAGING_CLEANUP_FAILED",
            cleaned
                ? committed.error_message
                : "Recording commit failed and staging cleanup also failed.");
    }

    if (discovery_.foreground_token() !=
        foreground_before) {
        return failure(
            "HOST_INTERFERENCE_DETECTED",
            "Recording artifacts were committed, but foreground changed.",
            components::object({
                {"outcome", "committed-requires-inspection"},
                {"path", config.output_path},
                {"analysisDir", config.analysis_directory},
            }));
    }

    const auto& data = *staged.data;
    const auto& evidence = *committed.evidence;
    return ModuleResult{
        true,
        {},
        {},
        components::object({
            {"capability", "window.record@1"},
            {"targetId", session_id},
            {"targetKind", "application-window"},
            {"executionDomain", "isolated-worker"},
            {"confirmationRequired", true},
            {"confirmationSatisfied", true},
            {"overwriteConfirmed", config.overwrite},
            {"path", config.output_path},
            {"analysisDir", config.analysis_directory},
            {"bytes",
             static_cast<std::int64_t>(
                 evidence.video_bytes)},
            {"width", *required_integer(data, "width")},
            {"height", *required_integer(data, "height")},
            {"sourceWidth",
             *required_integer(data, "sourceWidth")},
            {"sourceHeight",
             *required_integer(data, "sourceHeight")},
            {"deviceDriver",
             *required_string(data, "deviceDriver")},
            {"durationMs",
             static_cast<std::int64_t>(
                 config.duration_ms)},
            {"fps",
             static_cast<std::int64_t>(config.fps)},
            {"encodedFrames",
             *required_integer(data, "encodedFrames")},
            {"capturedFrames",
             *required_integer(data, "capturedFrames")},
            {"keyframeFiles",
             static_cast<std::int64_t>(
                 evidence.keyframe_files)},
            {"storyboardPath",
             config.analysis_directory +
                 "\\storyboard.png"},
            {"manifestPath",
             config.analysis_directory +
                 "\\manifest.json"},
            {"atomicOutput", true},
            {"audioCaptured", false},
            {"cursorCaptured", false},
            {"systemCaptureIndicatorMayAppear", true},
            {"foregroundUnchanged", true},
        }),
    };
}

}  // namespace act::modules
