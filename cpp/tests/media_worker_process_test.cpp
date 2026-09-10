#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <iostream>
#include <string>

namespace {

const act::components::Json* error_code(
    const act::components::Json& response) {
    const auto* error = response.find("error");
    return error == nullptr ? nullptr : error->find("code");
}

}  // namespace

int main() {
    const act::components::WorkerProcess worker;
    const auto timeout_request = act::components::object({
        {"contractVersion", "act/media-observation-worker/v1"},
        {"operation", "fixture-delay"},
        {"delayMilliseconds", 250},
    }).dump();
    const auto request = act::components::object({
        {"contractVersion", "act/media-observation-worker/v1"},
        {"operation", "media-sessions-read"},
        {"maximumItems", 128},
    }).dump();
    const auto timeout = worker.run_companion(
        "ai-computer-toolkit-media-worker.exe",
        timeout_request,
        1U,
        4U * 1024U * 1024U);
    if (timeout.completion !=
            act::components::WorkerCompletion::timed_out ||
        !timeout.job_terminated) {
        std::cerr << "media worker timeout did not terminate its job\n";
        return 1;
    }

    const auto normal = worker.run_companion(
        "ai-computer-toolkit-media-worker.exe",
        request,
        10000U,
        4U * 1024U * 1024U);
    if (normal.completion !=
            act::components::WorkerCompletion::completed ||
        normal.exit_code != 0U) {
        std::cerr << "media worker did not recover after timeout\n";
        return 1;
    }
    std::string parse_error;
    auto response =
        act::components::Json::parse(normal.stdout_text, parse_error);
    const auto* ok =
        response.has_value() ? response->find("ok") : nullptr;
    const auto* data =
        response.has_value() ? response->find("data") : nullptr;
    const auto* sessions =
        data == nullptr ? nullptr : data->find("sessions");
    const auto* foreground =
        data == nullptr
            ? nullptr
            : data->find("foregroundUnchanged");
    const auto* metadata =
        data == nullptr ? nullptr : data->find("metadataRead");
    const auto* writes =
        data == nullptr
            ? nullptr
            : data->find("writeMethodsCalled");
    if (!response.has_value() ||
        ok == nullptr ||
        ok->bool_value() == nullptr ||
        !*ok->bool_value() ||
        data == nullptr ||
        data->object_items() == nullptr ||
        sessions == nullptr ||
        sessions->array_items() == nullptr ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr ||
        !*foreground->bool_value() ||
        metadata == nullptr ||
        metadata->bool_value() == nullptr ||
        !*metadata->bool_value() ||
        writes == nullptr ||
        writes->bool_value() == nullptr ||
        *writes->bool_value()) {
        std::cerr << "media worker violated its read-only result\n";
        return 1;
    }

    const auto stale_request = act::components::object({
        {"contractVersion", "act/media-observation-worker/v1"},
        {"operation", "media-sessions-read"},
        {"maximumItems", 1},
        {"sessionId", "s2:m:0000000000000000"},
    }).dump();
    const auto stale = worker.run_companion(
        "ai-computer-toolkit-media-worker.exe",
        stale_request,
        10000U,
        4U * 1024U * 1024U);
    auto stale_response =
        act::components::Json::parse(
            stale.stdout_text, parse_error);
    const auto* stale_code =
        stale_response.has_value()
            ? error_code(*stale_response)
            : nullptr;
    if (stale.exit_code == 0U ||
        stale_code == nullptr ||
        stale_code->string_value() == nullptr ||
        *stale_code->string_value() != "STALE_SESSION") {
        std::cerr << "media worker accepted a stale opaque target\n";
        return 1;
    }

    std::cout << act::components::object({
        {"ok", true},
        {"timeoutJobTerminated", true},
        {"normalReadRecovered", true},
        {"sessionCount",
         static_cast<std::int64_t>(
             sessions->array_items()->size())},
        {"foregroundUnchanged", true},
        {"metadataRead", true},
        {"writeMethodsCalled", false},
        {"staleTargetRefused", true},
    }).dump() << '\n';
    return 0;
}
