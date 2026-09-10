#include "platform/windows/structured_image_observation_backend.hpp"

#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <algorithm>
#include <string_view>

namespace act::platform::windows {
namespace {

constexpr const char* worker_contract =
    "act/structured-image-observation-worker/v1";
constexpr const char* worker_name =
    "ai-computer-toolkit-structured-image-worker.exe";

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
                "ISOLATED_WORKER_UNAVAILABLE",
                result.error_message,
            };
        case Completion::protocol_failure:
        case Completion::completed:
            return BackendError{
                "OPERATION_FAILED",
                result.error_message.empty()
                    ? "The isolated provider worker returned an invalid result."
                    : result.error_message,
            };
    }
    return BackendError{
        "OPERATION_FAILED", "Unknown worker completion state."};
}

std::optional<BackendError> envelope(
    const components::WorkerProcessResult& process,
    components::Json& response,
    const components::Json*& data) {
    if (process.completion !=
        components::WorkerCompletion::completed) {
        return process_error(process);
    }
    std::string parse_error;
    auto parsed =
        components::Json::parse(process.stdout_text, parse_error);
    if (!parsed.has_value()) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated provider worker returned malformed JSON.",
        };
    }
    response = std::move(*parsed);
    const auto* contract = response.find("contractVersion");
    const auto* ok = response.find("ok");
    if (contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        ok == nullptr || ok->bool_value() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated provider worker violated its envelope.",
        };
    }
    if (!*ok->bool_value()) {
        const auto* error = response.find("error");
        const auto* code =
            error == nullptr ? nullptr : error->find("code");
        const auto* message =
            error == nullptr ? nullptr : error->find("message");
        if (code == nullptr || code->string_value() == nullptr ||
            message == nullptr ||
            message->string_value() == nullptr) {
            return BackendError{
                "OPERATION_FAILED",
                "The isolated provider worker returned an invalid error.",
            };
        }
        return BackendError{
            *code->string_value(), *message->string_value()};
    }
    data = response.find("data");
    if (data == nullptr || data->object_items() == nullptr) {
        return BackendError{
            "OPERATION_FAILED",
            "The isolated provider worker omitted result data.",
        };
    }
    return std::nullopt;
}

bool read_bool(
    const components::Json& value,
    const std::string_view key,
    bool& output) {
    const auto* field = value.find(key);
    if (field == nullptr || field->bool_value() == nullptr) {
        return false;
    }
    output = *field->bool_value();
    return true;
}

bool valid_session_id(
    const std::string_view value,
    const char kind) {
    if (value.size() != 21U ||
        !value.starts_with(
            std::string("s2:") + kind + ':')) {
        return false;
    }
    return std::all_of(
        value.begin() + 5,
        value.end(),
        [](const unsigned char character) {
            return (character >= '0' && character <= '9') ||
                   (character >= 'a' && character <= 'f');
        });
}

std::optional<double> number(
    const components::Json* value) {
    if (value == nullptr) {
        return std::nullopt;
    }
    if (value->double_value() != nullptr) {
        return *value->double_value();
    }
    if (value->integer_value() != nullptr) {
        return static_cast<double>(*value->integer_value());
    }
    return std::nullopt;
}

}  // namespace

StructuredImageStatusObservation
StructuredImageObservationBackend::status(
    const std::uint32_t timeout_ms) const {
    const components::Json request(components::Json::Object{
        {"contractVersion", worker_contract},
        {"operation", "provider-status"},
    });
    const auto process = components::WorkerProcess().run_companion(
        worker_name,
        request.dump(),
        timeout_ms,
        1024U * 1024U);
    components::Json response;
    const components::Json* data = nullptr;
    const auto error = envelope(process, response, data);
    if (error.has_value()) {
        return StructuredImageStatusObservation{
            std::nullopt, true, error};
    }
    bool installed = false;
    bool connected = false;
    bool foreground = false;
    bool write_methods = true;
    const auto* version = data->find("version");
    if (data->object_items()->size() != 5U ||
        !read_bool(*data, "installed", installed) ||
        !read_bool(*data, "connected", connected) ||
        !read_bool(
            *data, "foregroundUnchanged", foreground) ||
        !read_bool(
            *data, "writeMethodsCalled", write_methods) ||
        write_methods ||
        version == nullptr ||
        (version->string_value() == nullptr &&
         version->dump() != "null")) {
        return StructuredImageStatusObservation{
            std::nullopt,
            true,
            BackendError{
                "OPERATION_FAILED",
                "The provider status result violated its safety schema.",
            },
        };
    }
    std::optional<std::string> version_text;
    if (version->string_value() != nullptr) {
        version_text = *version->string_value();
    }
    return StructuredImageStatusObservation{
        StructuredImageStatus{
            installed,
            connected,
            std::move(version_text),
            std::nullopt,
            std::nullopt,
        },
        foreground,
        std::nullopt,
    };
}

StructuredImageInventoryObservation
StructuredImageObservationBackend::inventory(
    const std::uint32_t timeout_ms) const {
    const components::Json request(components::Json::Object{
        {"contractVersion", worker_contract},
        {"operation", "provider-sessions"},
    });
    const auto process = components::WorkerProcess().run_companion(
        worker_name,
        request.dump(),
        timeout_ms,
        4U * 1024U * 1024U);
    components::Json response;
    const components::Json* data = nullptr;
    const auto error = envelope(process, response, data);
    if (error.has_value()) {
        return StructuredImageInventoryObservation{
            std::nullopt, true, error};
    }
    bool foreground = false;
    bool write_methods = true;
    const auto* application = data->find("applicationSessionId");
    const auto* documents_value = data->find("documents");
    const auto* documents = documents_value == nullptr
        ? nullptr
        : documents_value->array_items();
    const auto* count = data->find("count");
    if (data->object_items()->size() != 5U ||
        documents == nullptr ||
        count == nullptr || count->integer_value() == nullptr ||
        *count->integer_value() < 0 ||
        static_cast<std::size_t>(*count->integer_value()) !=
            documents->size() ||
        !read_bool(
            *data, "foregroundUnchanged", foreground) ||
        !read_bool(
            *data, "writeMethodsCalled", write_methods) ||
        write_methods || application == nullptr) {
        return StructuredImageInventoryObservation{
            std::nullopt,
            true,
            BackendError{
                "OPERATION_FAILED",
                "The provider session result violated its safety schema.",
            },
        };
    }
    std::optional<std::string> application_id;
    if (application->string_value() != nullptr) {
        if (!valid_session_id(
                *application->string_value(), 'a')) {
            return StructuredImageInventoryObservation{
                std::nullopt,
                true,
                BackendError{
                    "OPERATION_FAILED",
                    "Provider returned an invalid application target.",
                },
            };
        }
        application_id = *application->string_value();
    }
    StructuredImageInventory inventory{
        {}, std::move(application_id),
        std::nullopt, std::nullopt};
    for (const auto& item : *documents) {
        const auto* session = item.find("sessionId");
        const auto* name = item.find("name");
        const auto* source = item.find("sourcePath");
        const auto width = number(item.find("width"));
        const auto height = number(item.find("height"));
        const auto resolution = number(item.find("resolution"));
        const auto* layers = item.find("layerCount");
        const auto* saved = item.find("saved");
        const auto* active = item.find("active");
        if (item.object_items() == nullptr ||
            item.object_items()->size() != 9U ||
            session == nullptr ||
            session->string_value() == nullptr ||
            !valid_session_id(*session->string_value(), 'd') ||
            name == nullptr || name->string_value() == nullptr ||
            source == nullptr || !width.has_value() ||
            !height.has_value() || !resolution.has_value() ||
            layers == nullptr ||
            layers->integer_value() == nullptr ||
            saved == nullptr || saved->bool_value() == nullptr ||
            active == nullptr || active->bool_value() == nullptr) {
            return StructuredImageInventoryObservation{
                std::nullopt,
                true,
                BackendError{
                    "OPERATION_FAILED",
                    "Provider returned an invalid document session.",
                },
            };
        }
        std::optional<std::string> source_path;
        if (source->string_value() != nullptr) {
            source_path = *source->string_value();
        } else if (source->dump() != "null") {
            return StructuredImageInventoryObservation{
                std::nullopt,
                true,
                BackendError{
                    "OPERATION_FAILED",
                    "Provider returned an invalid source path.",
                },
            };
        }
        inventory.documents.push_back(
            StructuredImageDocumentRecord{
                *session->string_value(),
                *name->string_value(),
                std::move(source_path),
                *width,
                *height,
                *resolution,
                *layers->integer_value(),
                *saved->bool_value(),
                *active->bool_value(),
                0U,
                0,
            });
    }
    return StructuredImageInventoryObservation{
        std::move(inventory),
        foreground,
        std::nullopt,
    };
}

}  // namespace act::platform::windows
