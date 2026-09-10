#include "components/json.hpp"
#include "platform/windows/structured_image_backend.hpp"

#include <windows.h>

#include <iostream>
#include <string>

namespace {

constexpr const char* worker_contract =
    "act/structured-image-observation-worker/v1";

int failure(
    const std::string& code,
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

int provider_status() {
    const HWND before = GetForegroundWindow();
    const auto status =
        act::platform::windows::StructuredImageBackend().status();
    const HWND after = GetForegroundWindow();
    if (status.error_code.has_value()) {
        return failure(
            *status.error_code,
            status.error_message.value_or(
                "Provider status failed."));
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"installed", status.installed},
             {"connected", status.connected},
             {"version",
              status.version.has_value()
                  ? act::components::Json(*status.version)
                  : act::components::Json(nullptr)},
             {"foregroundUnchanged", before == after},
             {"writeMethodsCalled", false},
         })},
    }).dump() << '\n';
    return 0;
}

int provider_sessions() {
    const HWND before = GetForegroundWindow();
    const auto inventory =
        act::platform::windows::StructuredImageBackend().inventory();
    const HWND after = GetForegroundWindow();
    if (inventory.error_code.has_value()) {
        return failure(
            *inventory.error_code,
            inventory.error_message.value_or(
                "Provider session observation failed."));
    }
    act::components::Json::Array documents;
    documents.reserve(inventory.documents.size());
    for (const auto& document : inventory.documents) {
        documents.push_back(act::components::object({
            {"sessionId", document.session_id},
            {"name", document.name},
            {"sourcePath",
             document.source_path.has_value()
                 ? act::components::Json(*document.source_path)
                 : act::components::Json(nullptr)},
            {"width", document.width_px},
            {"height", document.height_px},
            {"resolution", document.resolution_dpi},
            {"layerCount", document.layer_count},
            {"saved", document.saved},
            {"active", document.active},
        }));
    }
    std::cout << act::components::object({
        {"ok", true},
        {"contractVersion", worker_contract},
        {"data",
         act::components::object({
             {"applicationSessionId",
              inventory.application_session_id.has_value()
                  ? act::components::Json(
                        *inventory.application_session_id)
                  : act::components::Json(nullptr)},
             {"documents",
              act::components::Json(std::move(documents))},
             {"count",
              static_cast<std::int64_t>(
                  inventory.documents.size())},
             {"foregroundUnchanged", before == after},
             {"writeMethodsCalled", false},
         })},
    }).dump() << '\n';
    return 0;
}

}  // namespace

int main() {
    std::string request_text;
    if (!std::getline(std::cin, request_text)) {
        return failure(
            "INVALID_ARGUMENT",
            "The provider worker requires one request.");
    }
    std::string parse_error;
    const auto request =
        act::components::Json::parse(request_text, parse_error);
    const auto* contract = request.has_value()
        ? request->find("contractVersion")
        : nullptr;
    const auto* operation = request.has_value()
        ? request->find("operation")
        : nullptr;
    if (!request.has_value() ||
        request->object_items() == nullptr ||
        request->object_items()->size() != 2U ||
        contract == nullptr ||
        contract->string_value() == nullptr ||
        *contract->string_value() != worker_contract ||
        operation == nullptr ||
        operation->string_value() == nullptr) {
        return failure(
            "INVALID_ARGUMENT",
            "The provider request violates protocol v1.");
    }
    if (*operation->string_value() == "provider-status") {
        return provider_status();
    }
    if (*operation->string_value() == "provider-sessions") {
        return provider_sessions();
    }
    return failure(
        "INVALID_ARGUMENT",
        "The provider operation is not read-only.");
}
