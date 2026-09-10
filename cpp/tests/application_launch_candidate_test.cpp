#include "components/json.hpp"
#include "modules/application_launch_compatibility_module.hpp"
#include "platform/windows/application_launch_backend.hpp"
#include "platform/windows/discovery_backend.hpp"
#include "systems/application_launch_command_system.hpp"

#include <chrono>
#include <filesystem>
#include <iostream>
#include <thread>

int main(int argc, char** argv) {
    if (argc != 3) {
        return 2;
    }
    const std::filesystem::path fixture(argv[1]);
    const std::filesystem::path ready(argv[2]);
    std::error_code ignored;
    std::filesystem::remove(ready, ignored);

    const act::systems::ApplicationLaunchCommandSystem system;
    const auto unconfirmed = system.run_desktop(
        {"run", "desktop", "launch"});
    const auto legacy = system.run_desktop({
        "run", "desktop", "launch", "--confirm"});
    const auto stale = system.run_desktop({
        "run",
        "desktop",
        "launch",
        "--target",
        "sessionId=s2:a:0000000000000000",
        "--confirm",
    });
    if (unconfirmed.ok ||
        unconfirmed.error_code != "CONFIRMATION_REQUIRED" ||
        legacy.ok ||
        legacy.error_code !=
            "TARGET_ID_MIGRATION_REQUIRED" ||
        stale.ok ||
        stale.error_code != "TARGET_NOT_FOUND") {
        std::cerr << "Application launch policy ordering failed.\n";
        return 1;
    }

    act::platform::windows::InstalledApplicationRecord record{
        "s2:a:0000000000000001",
        "ACT launch fixture",
        {},
        {},
        {"test-owned"},
        {},
        act::platform::windows::ApplicationLaunchProvider::shell_item,
        fixture.string(),
    };
    const act::platform::windows::DiscoveryBackend foreground;
    const std::string before = foreground.foreground_token();
    const auto launched =
        act::platform::windows::ApplicationLaunchBackend().launch(record);
    for (int attempt = 0;
         attempt < 50 && !std::filesystem::exists(ready);
         ++attempt) {
        std::this_thread::sleep_for(
            std::chrono::milliseconds(50));
    }
    if (launched.error.has_value() ||
        !launched.evidence.has_value() ||
        !launched.evidence->launch_dispatched ||
        !std::filesystem::exists(ready) ||
        before != foreground.foreground_token()) {
        std::cerr << "Self-owned Shell launch evidence failed.\n";
        return 1;
    }

    const auto mapped =
        act::modules::ApplicationLaunchCompatibilityModule()
            .map_desktop_result(act::modules::ModuleResult{
                true,
                {},
                {},
                act::components::object({
                    {"capability", "application.open@1"},
                    {"targetId", record.session_id},
                    {"launchDispatched", true},
                    {"foregroundUnchanged", true},
                }),
            });
    const auto* shape =
        mapped.ok
            ? mapped.data.find("compatibilityShape")
            : nullptr;
    if (!mapped.ok ||
        shape == nullptr ||
        shape->string_value() == nullptr ||
        *shape->string_value() !=
            "secured-opaque-application-launch-v1") {
        std::cerr << "Application launch compatibility mapping failed.\n";
        return 1;
    }
    std::filesystem::remove(ready, ignored);
    return 0;
}
