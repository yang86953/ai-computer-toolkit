#include "systems/media_command_system.hpp"

#include <iostream>

int main() {
    const act::systems::MediaCommandSystem system;
    const auto unconfirmed = system.run({
        "run",
        "media-session",
        "play",
        "--target",
        "sessionId=s2:m:0000000000000000",
    });
    const auto invalid_operation = system.run({
        "run",
        "media-session",
        "arbitrary",
        "--target",
        "sessionId=s2:m:0000000000000000",
        "--confirm",
    });
    const auto legacy = system.run({
        "run",
        "media-session",
        "play",
        "--target",
        "sessionId=media:sample",
        "--confirm",
    });
    const auto stale = system.run({
        "run",
        "media-session",
        "play",
        "--target",
        "sessionId=s2:m:0000000000000000",
        "--timeout-ms",
        "5000",
        "--confirm",
    });
    if (unconfirmed.ok ||
        unconfirmed.error_code != "CONFIRMATION_REQUIRED" ||
        invalid_operation.ok ||
        invalid_operation.error_code != "INVALID_ARGUMENT" ||
        legacy.ok ||
        legacy.error_code !=
            "TARGET_ID_MIGRATION_REQUIRED" ||
        stale.ok ||
        stale.error_code != "TARGET_NOT_FOUND") {
        std::cerr
            << "Media control candidate weakened request ordering "
               "or exact-target safety.\n";
        return 1;
    }
    return 0;
}
