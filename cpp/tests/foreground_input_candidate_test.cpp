#include "components/key_chord.hpp"
#include "modules/foreground_input_module.hpp"

#include <iostream>

int main() {
    const auto chord =
        act::components::parse_key_chord(" ctrl + shift + a ");
    const auto duplicate =
        act::components::parse_key_chord("ctrl+control+a");
    const auto unsafe =
        act::components::parse_key_chord("WIN+R");
    const act::modules::ForegroundInputModule module;
    const auto unconfirmed = module.press_key(
        "s2:w:0000000000000000", "ENTER", false, false);
    const auto no_consent = module.press_key(
        "s2:w:0000000000000000", "ENTER", true, false);
    const auto invalid = module.press_key(
        "s2:w:0000000000000000", "WIN+R", true, true);
    const auto stale = module.press_key(
        "s2:w:0000000000000000", "ENTER", true, true);
    if (!chord.chord.has_value() ||
        chord.chord->normalized != "CTRL+SHIFT+A" ||
        duplicate.chord.has_value() ||
        unsafe.chord.has_value() ||
        unconfirmed.ok ||
        unconfirmed.error_code != "CONFIRMATION_REQUIRED" ||
        no_consent.ok ||
        no_consent.error_code !=
            "FOREGROUND_CONSENT_REQUIRED" ||
        invalid.ok ||
        invalid.error_code != "INVALID_ARGUMENT" ||
        stale.ok ||
        stale.error_code != "STALE_SESSION") {
        std::cerr
            << "Foreground input candidate weakened policy ordering.\n";
        return 1;
    }
    return 0;
}
