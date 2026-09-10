#pragma once

#include <optional>
#include <string>
#include <vector>

namespace act::components {

enum class NamedKey {
    ascii,
    enter,
    tab,
    escape,
    backspace,
    delete_key,
    space,
    left,
    up,
    right,
    down,
    home,
    end,
    f4,
};

enum class KeyModifier {
    control,
    alt,
    shift,
};

struct KeyChord {
    std::vector<KeyModifier> modifiers;
    NamedKey key;
    char ascii_character;
    std::string normalized;
};

struct KeyChordParseResult {
    std::optional<KeyChord> chord;
    std::string error;
};

[[nodiscard]] KeyChordParseResult parse_key_chord(
    const std::string& value);

}  // namespace act::components
