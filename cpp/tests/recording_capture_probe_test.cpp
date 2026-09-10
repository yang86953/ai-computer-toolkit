#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <windows.h>

#include <iostream>
#include <set>
#include <string>

int main() {
    const HWND foreground_before = GetForegroundWindow();
    const auto request = act::components::object({
        {"contractVersion", "act/capture-worker/v1"},
        {"operation", "fixture-recording-probe"},
        {"frameCount", 4},
        {"intervalMs", 80},
    }).dump();
    const auto process =
        act::components::WorkerProcess().run_companion(
            "ai-computer-toolkit-capture-worker.exe",
            request,
            10000U,
            4U * 1024U * 1024U);
    if (process.completion !=
            act::components::WorkerCompletion::completed ||
        process.exit_code != 0U ||
        !process.job_terminated) {
        std::cerr
            << "The isolated recording probe did not complete safely.\n";
        return 1;
    }

    std::string parse_error;
    const auto response = act::components::Json::parse(
        process.stdout_text, parse_error);
    const auto* ok =
        response.has_value() ? response->find("ok") : nullptr;
    const auto* data =
        response.has_value() ? response->find("data") : nullptr;
    const auto* requested =
        data == nullptr ? nullptr : data->find("framesRequested");
    const auto* captured =
        data == nullptr ? nullptr : data->find("framesCaptured");
    const auto* distinct =
        data == nullptr ? nullptr : data->find("distinctFrames");
    const auto* digests =
        data == nullptr ? nullptr : data->find("pixelDigests");
    const auto* foreground = data == nullptr
        ? nullptr
        : data->find("foregroundUnchanged");
    const auto* fixture = data == nullptr
        ? nullptr
        : data->find("fixtureOwnedByToolkit");
    const auto* one_session = data == nullptr
        ? nullptr
        : data->find("singleCaptureSession");
    const auto* surfaces = data == nullptr
        ? nullptr
        : data->find("frameSurfacesAccessed");
    const auto* persisted = data == nullptr
        ? nullptr
        : data->find("pixelsPersisted");
    const auto* file_written = data == nullptr
        ? nullptr
        : data->find("fileWritten");
    if (ok == nullptr ||
        ok->bool_value() == nullptr ||
        !*ok->bool_value() ||
        requested == nullptr ||
        requested->integer_value() == nullptr ||
        *requested->integer_value() != 4 ||
        captured == nullptr ||
        captured->integer_value() == nullptr ||
        *captured->integer_value() != 4 ||
        distinct == nullptr ||
        distinct->integer_value() == nullptr ||
        *distinct->integer_value() != 4 ||
        digests == nullptr ||
        digests->array_items() == nullptr ||
        digests->array_items()->size() != 4U ||
        foreground == nullptr ||
        foreground->bool_value() == nullptr ||
        !*foreground->bool_value() ||
        fixture == nullptr ||
        fixture->bool_value() == nullptr ||
        !*fixture->bool_value() ||
        one_session == nullptr ||
        one_session->bool_value() == nullptr ||
        !*one_session->bool_value() ||
        surfaces == nullptr ||
        surfaces->bool_value() == nullptr ||
        !*surfaces->bool_value() ||
        persisted == nullptr ||
        persisted->bool_value() == nullptr ||
        *persisted->bool_value() ||
        file_written == nullptr ||
        file_written->bool_value() == nullptr ||
        *file_written->bool_value() ||
        GetForegroundWindow() != foreground_before) {
        std::cerr
            << "The recording probe violated its bounded read-only contract.\n";
        return 1;
    }

    std::set<std::string> unique;
    for (const auto& digest : *digests->array_items()) {
        if (digest.string_value() == nullptr ||
            digest.string_value()->size() != 16U) {
            std::cerr
                << "The recording probe returned an invalid digest.\n";
            return 1;
        }
        unique.insert(*digest.string_value());
    }
    if (unique.size() != 4U) {
        std::cerr
            << "The recording probe did not observe temporal changes.\n";
        return 1;
    }

    std::cout << act::components::object({
        {"ok", true},
        {"selfOwnedFixtureOnly", true},
        {"singleCaptureSession", true},
        {"framesCaptured", 4},
        {"distinctFrames", 4},
        {"frameSurfacesAccessed", true},
        {"pixelsPersisted", false},
        {"fileWritten", false},
        {"foregroundUnchanged", true},
        {"workerJobTerminated", true},
    }).dump() << '\n';
    return 0;
}
