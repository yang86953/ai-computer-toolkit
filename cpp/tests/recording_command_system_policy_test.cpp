#include "systems/recording_command_system.hpp"

#include <iostream>
#include <vector>

int main() {
    const act::systems::RecordingCommandSystem system;
    const auto unconfirmed = system.run_desktop(
        {"run", "desktop", "record"});
    const auto legacy = system.run_desktop({
        "run",
        "desktop",
        "record",
        "--target",
        "sessionId=window:1234",
        "--confirm",
    });
    const auto unknown = system.run_desktop({
        "run",
        "desktop",
        "record",
        "--target",
        "sessionId=s2:w:0123456789abcdef",
        "--arg",
        "path=C:\\owned\\record.mp4",
        "--arg",
        "unknown=true",
        "--confirm",
    });
    const auto app_unconfirmed = system.run_app(
        {"run", "app", "record"});
    const auto app_missing_input = system.run_app({
        "run", "app", "record", "--confirm"});
    if (unconfirmed.ok ||
        unconfirmed.error_code !=
            "CONFIRMATION_REQUIRED" ||
        legacy.ok ||
        legacy.error_code !=
            "TARGET_ID_MIGRATION_REQUIRED" ||
        unknown.ok ||
        unknown.error_code != "INVALID_ARGUMENT" ||
        app_unconfirmed.ok ||
        app_unconfirmed.error_code !=
            "CONFIRMATION_REQUIRED" ||
        app_missing_input.ok ||
        app_missing_input.error_code !=
            "INVALID_ARGUMENT") {
        std::cerr
            << "Recording command candidate weakened request ordering.\n";
        return 1;
    }
    return 0;
}
