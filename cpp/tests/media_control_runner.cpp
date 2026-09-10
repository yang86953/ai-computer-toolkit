#include "components/json.hpp"
#include "systems/media_command_system.hpp"

#include <iostream>
#include <string>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 3 && argc != 4) {
        return 2;
    }
    const bool confirmed =
        argc == 3 || std::string(argv[3]) != "--unconfirmed";
    std::vector<std::string> arguments{
        "run",
        "media-session",
        argv[2],
        "--target",
        std::string("sessionId=") + argv[1],
        "--timeout-ms",
        "5000",
    };
    if (confirmed) {
        arguments.emplace_back("--confirm");
    }
    auto result =
        act::systems::MediaCommandSystem().run(arguments);
    if (!result.ok) {
        std::cout << act::components::object({
            {"ok", false},
            {"error",
             act::components::object({
                 {"code", result.error_code},
                 {"message", result.error_message},
             })},
        }).dump() << '\n';
        return 2;
    }
    std::cout << result.data.dump() << '\n';
    return 0;
}
