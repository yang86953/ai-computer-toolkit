#include "components/json.hpp"
#include "systems/recording_command_system.hpp"

#include <iostream>
#include <string>
#include <vector>

int main(int argc, char** argv) {
    if (argc == 2 &&
        std::string(argv[1]) == "unconfirmed") {
        const auto result =
            act::systems::RecordingCommandSystem().run_desktop(
                {"run", "desktop", "record"});
        std::cout << act::components::object({
            {"ok", result.ok},
            {"error",
             act::components::object({
                 {"code", result.error_code},
                 {"message", result.error_message},
             })},
            {"data", result.data},
        }).dump() << '\n';
        return result.ok ? 0 : 2;
    }
    if (argc == 3 &&
        std::string(argv[1]) == "app") {
        const auto result =
            act::systems::RecordingCommandSystem().run_app({
                "run",
                "app",
                "record",
                "--input",
                argv[2],
                "--confirm",
            });
        std::cout << act::components::object({
            {"ok", result.ok},
            {"error",
             result.ok
                 ? act::components::Json(nullptr)
                 : act::components::object({
                       {"code", result.error_code},
                       {"message", result.error_message},
                   })},
            {"data", result.data},
        }).dump() << '\n';
        return result.ok ? 0 : 2;
    }
    if (argc != 4) {
        std::cerr
            << "Session, output path, and analysis path are required.\n";
        return 2;
    }
    const std::vector<std::string> arguments{
        "run",
        "desktop",
        "record",
        "--target",
        "sessionId=" + std::string(argv[1]),
        "--arg",
        "path=" + std::string(argv[2]),
        "--arg",
        "analysisDir=" + std::string(argv[3]),
        "--arg",
        "durationMs=2000",
        "--arg",
        "fps=2",
        "--arg",
        "maxWidth=960",
        "--arg",
        "crf=32",
        "--arg",
        "maxKeyframes=8",
        "--arg",
        "changeThreshold=0.035",
        "--arg",
        "timeoutMs=5000",
        "--arg",
        "overwrite=false",
        "--confirm",
    };
    const auto result =
        act::systems::RecordingCommandSystem().run_desktop(
            arguments);
    std::cout << act::components::object({
        {"ok", result.ok},
        {"error",
         result.ok
             ? act::components::Json(nullptr)
             : act::components::object({
                   {"code", result.error_code},
                   {"message", result.error_message},
               })},
        {"data", result.data},
    }).dump() << '\n';
    return result.ok ? 0 : 2;
}
