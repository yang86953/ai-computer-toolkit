#include "act/control_system.hpp"
#include "components/cancellation.hpp"
#include "components/json.hpp"

#include <windows.h>

#include <iostream>
#include <string>
#include <string_view>
#include <vector>

namespace {

BOOL WINAPI handle_console_control(const DWORD event) {
    if (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT) {
        act::components::request_global_cancellation();
        return TRUE;
    }
    return FALSE;
}

}  // namespace

int main(const int argc, char** argv) {
    SetConsoleCtrlHandler(handle_console_control, TRUE);
    std::vector<std::string> arguments;
    arguments.reserve(argc > 1 ? static_cast<std::size_t>(argc - 1) : 0U);
    bool pretty = false;
    for (int index = 1; index < argc; ++index) {
        if (std::string_view(argv[index]) == "--pretty") {
            pretty = true;
        } else {
            arguments.emplace_back(argv[index]);
        }
    }

    act::ComputerControlSystem system;
    const act::CliResult result = system.execute(arguments);
    if (pretty) {
        std::string error;
        auto parsed = act::components::Json::parse(
            result.json, error);
        if (parsed.has_value()) {
            std::cout << parsed->dump_pretty() << '\n';
        } else {
            std::cout << result.json << '\n';
        }
    } else {
        std::cout << result.json << '\n';
    }
    return result.exit_code;
}
