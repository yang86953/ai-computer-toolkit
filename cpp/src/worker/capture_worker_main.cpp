#include "components/json.hpp"
// 复用主进程相同的 opaque 目标唯一匹配组件。
#include "components/opaque_target_match.hpp"
#include "platform/windows/capture_preflight_backend.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "platform/windows/png_file_output.hpp"
#include "platform/windows/text_codec.hpp"
#include "platform/windows/wgc_capture.hpp"

#include <windows.h>

#include <array>
#include <cctype>
#include <iostream>
#include <optional>
#include <string>

namespace {

constexpr const char* worker_contract = "act/capture-worker/v1";

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

int write_result(
    const act::platform::windows::CaptureFrameResult& result) {
    if (result.error.has_value()) {
        return write_failure(
            result.error->code.c_str(), result.error->message);
    }
    if (!result.metadata.has_value()) {
        return write_failure(
            "OPERATION_FAILED",
            "The capture worker omitted frame metadata.");
    }
    const auto& frame = *result.metadata;
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"fixtureOwnedByToolkit",
              frame.fixture_owned_by_toolkit},
             {"frameAcquired", true},
             {"frameWidth", frame.width},
             {"frameHeight", frame.height},
             {"deviceDriver", frame.device_driver},
             {"frameSurfaceAccessed",
              frame.frame_surface_accessed},
             {"pixelsPersisted", frame.pixels_persisted},
             {"fileWritten", frame.file_written},
             {"foregroundUnchanged",
              frame.foreground_unchanged},
             {"privacyIndicatorMayHaveAppeared",
              frame.privacy_indicator_may_have_appeared},
             {"pixelBytesRead", frame.pixel_bytes_read},
             {"rowPitch", frame.row_pitch},
             {"pixelDigest", frame.pixel_digest},
             {"pixelFormat", frame.pixel_format},
             {"pngEncoded", frame.png_encoded},
             {"pngBytes", frame.png_bytes},
             {"pngDigest", frame.png_digest},
             {"pngSignatureValid", frame.png_signature_valid},
             {"outputPath", frame.output_path},
             {"replacedExisting", frame.replaced_existing},
         })},
    }).dump() << '\n';
    return frame.foreground_unchanged ? 0 : 3;
}

int write_recording_probe(
    const act::platform::windows::RecordingProbeResult& result) {
    if (result.error.has_value()) {
        return write_failure(
            result.error->code.c_str(), result.error->message);
    }
    if (!result.metadata.has_value()) {
        return write_failure(
            "OPERATION_FAILED",
            "The capture worker omitted recording probe metadata.");
    }
    const auto& probe = *result.metadata;
    act::components::Json::Array digests;
    digests.reserve(probe.pixel_digests.size());
    for (const auto& digest : probe.pixel_digests) {
        digests.emplace_back(digest);
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"fixtureOwnedByToolkit",
              probe.fixture_owned_by_toolkit},
             {"singleCaptureSession",
              probe.single_capture_session},
             {"frameSurfacesAccessed",
              probe.frame_surfaces_accessed},
             {"framesRequested",
              static_cast<std::int64_t>(
                  probe.frames_requested)},
             {"framesCaptured",
              static_cast<std::int64_t>(
                  probe.frames_captured)},
             {"distinctFrames",
              static_cast<std::int64_t>(
                  probe.distinct_frames)},
             {"frameWidth", probe.width},
             {"frameHeight", probe.height},
             {"deviceDriver", probe.device_driver},
             {"pixelDigests",
              act::components::Json(std::move(digests))},
             {"pixelsPersisted", probe.pixels_persisted},
             {"fileWritten", probe.file_written},
             {"foregroundUnchanged",
              probe.foreground_unchanged},
             {"privacyIndicatorMayHaveAppeared",
              probe.privacy_indicator_may_have_appeared},
         })},
    }).dump() << '\n';
    return probe.foreground_unchanged ? 0 : 3;
}

std::optional<std::string> fixture_output_path(
    const std::string& filename) {
    if (filename.size() < 5U ||
        filename.size() > 128U ||
        !filename.ends_with(".png")) {
        return std::nullopt;
    }
    for (const unsigned char character : filename) {
        if (!(std::isalnum(character) != 0 ||
              character == '.' ||
              character == '-' ||
              character == '_')) {
            return std::nullopt;
        }
    }
    std::array<wchar_t, 32768> module{};
    const DWORD length = GetModuleFileNameW(
        nullptr,
        module.data(),
        static_cast<DWORD>(module.size()));
    if (length == 0U || length >= module.size()) {
        return std::nullopt;
    }
    std::wstring directory(module.data(), length);
    const std::size_t separator =
        directory.find_last_of(L"\\/");
    if (separator == std::wstring::npos) {
        return std::nullopt;
    }
    directory.resize(separator);
    directory += L"\\atomic-png-worker-fixtures\\";
    directory += act::platform::windows::wide(filename);
    return act::platform::windows::utf8(directory);
}

int write_fixture_png(
    const act::components::Json& request) {
    const auto* confirmed = request.find("confirmed");
    const auto* filename = request.find("outputFileName");
    const auto* overwrite_value = request.find("overwrite");
    if (confirmed == nullptr ||
        confirmed->bool_value() == nullptr ||
        !*confirmed->bool_value()) {
        return write_failure(
            "CONFIRMATION_REQUIRED",
            "Fixture PNG file output requires explicit confirmation.");
    }
    if (filename == nullptr ||
        filename->string_value() == nullptr) {
        return write_failure(
            "INVALID_ARGUMENT",
            "Fixture PNG file output requires outputFileName.");
    }
    const auto path =
        fixture_output_path(*filename->string_value());
    if (!path.has_value()) {
        return write_failure(
            "INVALID_ARGUMENT",
            "Fixture output filename violates its confined policy.");
    }
    const bool overwrite =
        overwrite_value != nullptr &&
        overwrite_value->bool_value() != nullptr &&
        *overwrite_value->bool_value();

    auto capture =
        act::platform::windows::capture_fixture_memory_png();
    if (capture.error.has_value()) {
        return write_failure(
            capture.error->code.c_str(),
            capture.error->message);
    }
    auto& frame = *capture.metadata;
    auto output =
        act::platform::windows::write_png_atomically(
            *path, frame.encoded_png, overwrite);
    if (!output.output.has_value()) {
        return write_failure(
            output.error_code.c_str(), output.error_message);
    }
    frame.file_written = true;
    frame.pixels_persisted = true;
    frame.output_path = output.output->normalized_path;
    frame.replaced_existing =
        output.output->replaced_existing;
    frame.encoded_png.clear();
    frame.encoded_png.shrink_to_fit();
    return write_result(capture);
}

bool confirmed_request(
    const act::components::Json& request) {
    const auto* confirmed = request.find("confirmed");
    return confirmed != nullptr &&
           confirmed->bool_value() != nullptr &&
           *confirmed->bool_value();
}

int write_window_screenshot(
    const act::components::Json& request,
    const act::platform::windows::WindowRecord& window) {
    const auto* output_path = request.find("outputPath");
    const auto* overwrite_value = request.find("overwrite");
    if (output_path == nullptr ||
        output_path->string_value() == nullptr) {
        return write_failure(
            "INVALID_ARGUMENT",
            "window-screenshot requires outputPath.");
    }
    const bool overwrite =
        overwrite_value != nullptr &&
        overwrite_value->bool_value() != nullptr &&
        *overwrite_value->bool_value();
    const auto plan =
        act::platform::windows::validate_png_output_path(
            *output_path->string_value(), overwrite);
    if (!plan.plan.has_value()) {
        return write_failure(
            plan.error_code.c_str(), plan.error_message);
    }

    const act::platform::windows::CapturePreflightBackend preflight;
    const auto readiness = preflight.inspect(window);
    if (readiness.error.has_value()) {
        return write_failure(
            readiness.error->code.c_str(),
            readiness.error->message);
    }
    if (readiness.preflight->eligibility !=
        "eligible-for-certified-capture-route") {
        return write_failure(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The exact target is not eligible for background capture: " +
                readiness.preflight->eligibility + ".");
    }
    return write_result(
        act::platform::windows::capture_window_png_file(
            window.native_window,
            plan.plan->normalized_path,
            overwrite));
}

}  // namespace

int main() {
    std::string request_text;
    if (!std::getline(std::cin, request_text)) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The capture worker requires one JSON request.");
    }
    std::string parse_error;
    auto request =
        act::components::Json::parse(request_text, parse_error);
    const auto* contract =
        request.has_value()
            ? request->find("contractVersion")
            : nullptr;
    const auto* operation =
        request.has_value() ? request->find("operation") : nullptr;
    if (!request.has_value() ||
        request->object_items() == nullptr ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        operation == nullptr ||
        operation->string_value() == nullptr) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The capture worker request violates protocol v1.");
    }

    if (*operation->string_value() == "fixture-memory-frame") {
        return write_result(
            act::platform::windows::capture_fixture_frame());
    }
    if (*operation->string_value() ==
        "fixture-surface-readback") {
        return write_result(
            act::platform::windows::
                capture_fixture_surface_readback());
    }
    if (*operation->string_value() == "fixture-memory-png") {
        return write_result(
            act::platform::windows::
                capture_fixture_memory_png());
    }
    if (*operation->string_value() ==
        "fixture-recording-probe") {
        const auto* frame_count = request->find("frameCount");
        const auto* interval = request->find("intervalMs");
        if (frame_count == nullptr ||
            frame_count->integer_value() == nullptr ||
            interval == nullptr ||
            interval->integer_value() == nullptr ||
            *frame_count->integer_value() < 0 ||
            *interval->integer_value() < 0) {
            return write_failure(
                "INVALID_ARGUMENT",
                "The recording probe requires integer bounds.");
        }
        return write_recording_probe(
            act::platform::windows::
                capture_fixture_recording_probe(
                    static_cast<std::uint32_t>(
                        *frame_count->integer_value()),
                    static_cast<std::uint32_t>(
                        *interval->integer_value())));
    }
    if (*operation->string_value() == "fixture-png-file") {
        return write_fixture_png(*request);
    }
    const bool frame_metadata =
        *operation->string_value() == "window-frame-metadata";
    const bool window_screenshot =
        *operation->string_value() == "window-screenshot";
    if (!frame_metadata && !window_screenshot) {
        return write_failure(
            "INVALID_ARGUMENT",
            "The capture worker operation is not published.");
    }
    if (!confirmed_request(*request)) {
        return write_failure(
            "CONFIRMATION_REQUIRED",
            "The real-window capture operation requires confirmation.");
    }
    const auto* session_id = request->find("sessionId");
    if (session_id == nullptr ||
        session_id->string_value() == nullptr ||
        session_id->string_value()->empty()) {
        return write_failure(
            "INVALID_ARGUMENT",
            "Real-window capture requires an opaque sessionId.");
    }

    const act::platform::windows::DiscoveryBackend backend;
    const auto windows = backend.enumerate_windows(4096U);
    // 完整扫描 worker 当前窗口清单以检测碰撞。
    const auto match = act::components::match_opaque_target(
        windows.begin(),
        windows.end(),
        [session_id](const auto& window) {
            return window.session_id == *session_id->string_value();
        });
    // 多命中时不允许 worker 捕获任意窗口。
    if (match.state == act::components::OpaqueTargetMatchState::ambiguous) {
        // 返回稳定的 JSON over stdio 歧义错误。
        return write_failure(
            "AMBIGUOUS_TARGET",
            "The opaque application-window session resolves to multiple windows.");
    }
    // 零命中保持现有过期会话错误。
    if (match.state == act::components::OpaqueTargetMatchState::missing) {
        return write_failure(
            "STALE_SESSION",
            "The opaque application-window session no longer resolves.");
    }
    if (window_screenshot) {
        return write_window_screenshot(*request, *match.position);
    }
    return write_result(
        act::platform::windows::capture_window_frame(
            match.position->native_window));
}
