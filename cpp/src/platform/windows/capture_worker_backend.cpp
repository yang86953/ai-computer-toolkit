#include "platform/windows/capture_worker_backend.hpp"

#include "components/json.hpp"
#include "components/worker_process.hpp"

namespace act::platform::windows {
namespace {

constexpr const char* worker_contract = "act/capture-worker/v1";
constexpr const char* worker_name =
    "ai-computer-toolkit-capture-worker.exe";

BackendError process_error(
    const components::WorkerProcessResult& result) {
    using Completion = components::WorkerCompletion;
    switch (result.completion) {
        case Completion::timed_out:
            return BackendError{"TIMEOUT", result.error_message};
        case Completion::cancelled:
            return BackendError{"CANCELLED", result.error_message};
        case Completion::unavailable:
            return BackendError{
                "ISOLATED_WORKER_UNAVAILABLE", result.error_message};
        case Completion::protocol_failure:
            return BackendError{"OPERATION_FAILED", result.error_message};
        case Completion::completed:
            break;
    }
    return BackendError{
        "OPERATION_FAILED",
        "The isolated capture worker returned an invalid result.",
    };
}

bool read_bool(
    const components::Json& object,
    const std::string_view key,
    bool& output) {
    const auto* value = object.find(key);
    if (value == nullptr || value->bool_value() == nullptr) {
        return false;
    }
    output = *value->bool_value();
    return true;
}

CaptureFrameProbeResult invalid_result(const char* message) {
    return CaptureFrameProbeResult{
        std::nullopt,
        BackendError{"OPERATION_FAILED", message},
    };
}

}  // namespace

CaptureFrameProbeResult CaptureWorkerBackend::probe_frame(
    const std::string& session_id,
    const std::uint32_t timeout_ms) const {
    const auto request = components::object({
        {"contractVersion", worker_contract},
        {"operation", "window-frame-metadata"},
        {"sessionId", session_id},
        {"confirmed", true},
    });
    const auto process = components::WorkerProcess().run_companion(
        worker_name,
        request.dump(),
        timeout_ms,
        1024U * 1024U);
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return CaptureFrameProbeResult{
            std::nullopt, process_error(process)};
    }

    std::string parse_error;
    auto response =
        components::Json::parse(process.stdout_text, parse_error);
    if (!response.has_value()) {
        return invalid_result(
            "The isolated capture worker returned malformed JSON.");
    }
    const auto* contract = response->find("contractVersion");
    const auto* ok = response->find("ok");
    if (contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr) {
        return invalid_result(
            "The isolated capture worker violated its protocol envelope.");
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
            return invalid_result(
                "The isolated capture worker returned an invalid error.");
        }
        return CaptureFrameProbeResult{
            std::nullopt,
            BackendError{
                *code->string_value(), *message->string_value()},
        };
    }

    const auto* data = response->find("data");
    if (data == nullptr || data->object_items() == nullptr) {
        return invalid_result(
            "The isolated capture worker omitted result data.");
    }
    const auto* width = data->find("frameWidth");
    const auto* height = data->find("frameHeight");
    const auto* driver = data->find("deviceDriver");
    CaptureFrameProbe frame{};
    if (width == nullptr ||
        width->integer_value() == nullptr ||
        *width->integer_value() <= 0 ||
        height == nullptr ||
        height->integer_value() == nullptr ||
        *height->integer_value() <= 0 ||
        driver == nullptr ||
        driver->string_value() == nullptr ||
        !read_bool(
            *data,
            "frameSurfaceAccessed",
            frame.frame_surface_accessed) ||
        !read_bool(
            *data, "pixelsPersisted", frame.pixels_persisted) ||
        !read_bool(*data, "fileWritten", frame.file_written) ||
        !read_bool(
            *data,
            "foregroundUnchanged",
            frame.foreground_unchanged) ||
        !read_bool(
            *data,
            "privacyIndicatorMayHaveAppeared",
            frame.privacy_indicator_may_have_appeared)) {
        return invalid_result(
            "The isolated capture worker returned invalid frame metadata.");
    }
    frame.width = *width->integer_value();
    frame.height = *height->integer_value();
    frame.device_driver = *driver->string_value();
    if (frame.frame_surface_accessed ||
        frame.pixels_persisted ||
        frame.file_written ||
        !frame.foreground_unchanged) {
        return invalid_result(
            "The isolated capture worker violated its safety boundary.");
    }
    return CaptureFrameProbeResult{
        std::move(frame), std::nullopt};
}

ScreenshotCaptureResult CaptureWorkerBackend::capture_screenshot(
    const std::string& session_id,
    const std::string& output_path,
    const bool overwrite,
    const std::uint32_t timeout_ms) const {
    const auto request = components::object({
        {"contractVersion", worker_contract},
        {"operation", "window-screenshot"},
        {"sessionId", session_id},
        {"outputPath", output_path},
        {"confirmed", true},
        {"overwrite", overwrite},
    });
    const auto process = components::WorkerProcess().run_companion(
        worker_name,
        request.dump(),
        timeout_ms,
        1024U * 1024U);
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return ScreenshotCaptureResult{
            std::nullopt, process_error(process)};
    }

    std::string parse_error;
    auto response =
        components::Json::parse(process.stdout_text, parse_error);
    if (!response.has_value()) {
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated capture worker returned malformed JSON.",
            },
        };
    }
    const auto* contract = response->find("contractVersion");
    const auto* ok = response->find("ok");
    if (contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr ||
        ok->bool_value() == nullptr) {
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated capture worker violated its protocol envelope.",
            },
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
            return ScreenshotCaptureResult{
                std::nullopt,
                BackendError{
                    "OPERATION_FAILED",
                    "The isolated capture worker returned an invalid error.",
                },
            };
        }
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                *code->string_value(), *message->string_value()},
        };
    }

    const auto* data = response->find("data");
    const auto* width =
        data == nullptr ? nullptr : data->find("frameWidth");
    const auto* height =
        data == nullptr ? nullptr : data->find("frameHeight");
    const auto* bytes =
        data == nullptr ? nullptr : data->find("pngBytes");
    const auto* driver =
        data == nullptr ? nullptr : data->find("deviceDriver");
    const auto* digest =
        data == nullptr ? nullptr : data->find("pixelDigest");
    const auto* path =
        data == nullptr ? nullptr : data->find("outputPath");
    ScreenshotCapture screenshot{};
    bool surface_accessed = false;
    bool pixels_persisted = false;
    bool file_written = false;
    bool png_encoded = false;
    bool png_signature = false;
    if (data == nullptr ||
        data->object_items() == nullptr ||
        width == nullptr ||
        width->integer_value() == nullptr ||
        *width->integer_value() <= 0 ||
        *width->integer_value() > 4096 ||
        height == nullptr ||
        height->integer_value() == nullptr ||
        *height->integer_value() <= 0 ||
        *height->integer_value() > 4096 ||
        bytes == nullptr ||
        bytes->integer_value() == nullptr ||
        *bytes->integer_value() <= 0 ||
        *bytes->integer_value() > 64LL * 1024LL * 1024LL ||
        driver == nullptr ||
        driver->string_value() == nullptr ||
        (*driver->string_value() != "hardware" &&
         *driver->string_value() != "warp") ||
        digest == nullptr ||
        digest->string_value() == nullptr ||
        digest->string_value()->size() != 16U ||
        path == nullptr ||
        path->string_value() == nullptr ||
        path->string_value()->empty() ||
        *path->string_value() != output_path ||
        !read_bool(
            *data, "frameSurfaceAccessed", surface_accessed) ||
        !read_bool(
            *data, "pixelsPersisted", pixels_persisted) ||
        !read_bool(*data, "fileWritten", file_written) ||
        !read_bool(*data, "pngEncoded", png_encoded) ||
        !read_bool(
            *data, "pngSignatureValid", png_signature) ||
        !read_bool(
            *data,
            "foregroundUnchanged",
            screenshot.foreground_unchanged) ||
        !read_bool(
            *data,
            "privacyIndicatorMayHaveAppeared",
            screenshot.privacy_indicator_may_have_appeared) ||
        !read_bool(
            *data,
            "replacedExisting",
            screenshot.replaced_existing) ||
        !surface_accessed ||
        !pixels_persisted ||
        !file_written ||
        !png_encoded ||
        !png_signature) {
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated worker returned invalid screenshot evidence.",
            },
        };
    }
    screenshot.width = *width->integer_value();
    screenshot.height = *height->integer_value();
    screenshot.bytes = *bytes->integer_value();
    screenshot.device_driver = *driver->string_value();
    screenshot.pixel_digest = *digest->string_value();
    screenshot.output_path = *path->string_value();
    if (!overwrite && screenshot.replaced_existing) {
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                "OPERATION_FAILED",
                "The isolated worker reported an unconfirmed replacement.",
            },
        };
    }
    if (!screenshot.foreground_unchanged) {
        return ScreenshotCaptureResult{
            std::nullopt,
            BackendError{
                "HOST_INTERFERENCE_DETECTED",
                "The screenshot file may exist, but foreground changed "
                "during isolated capture.",
            },
        };
    }
    return ScreenshotCaptureResult{
        std::move(screenshot), std::nullopt};
}

}  // namespace act::platform::windows
