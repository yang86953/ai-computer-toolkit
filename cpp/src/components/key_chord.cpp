#include "components/key_chord.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <string_view>

namespace act::components {
namespace {

std::string trim_upper(const std::string_view value) {
    std::size_t first = 0U;
    while (first < value.size() &&
           std::isspace(
               static_cast<unsigned char>(value[first])) != 0) {
        ++first;
    }
    std::size_t last = value.size();
    while (last > first &&
           std::isspace(
               static_cast<unsigned char>(value[last - 1U])) != 0) {
        --last;
    }
    std::string normalized(value.substr(first, last - first));
    std::transform(
        normalized.begin(),
        normalized.end(),
        normalized.begin(),
        [](const unsigned char character) {
            return static_cast<char>(std::toupper(character));
        });
    return normalized;
}

std::optional<KeyModifier> modifier(
    const std::string_view value) {
    if (value == "CTRL" || value == "CONTROL") {
        return KeyModifier::control;
    }
    if (value == "ALT") {
        return KeyModifier::alt;
    }
    if (value == "SHIFT") {
        return KeyModifier::shift;
    }
    return std::nullopt;
}

std::optional<NamedKey> named_key(
    const std::string_view value) {
    constexpr std::array pairs{
        std::pair{"ENTER", NamedKey::enter},
        std::pair{"RETURN", NamedKey::enter},
        std::pair{"TAB", NamedKey::tab},
        std::pair{"ESC", NamedKey::escape},
        std::pair{"ESCAPE", NamedKey::escape},
        std::pair{"BACKSPACE", NamedKey::backspace},
        std::pair{"BACK", NamedKey::backspace},
        std::pair{"DELETE", NamedKey::delete_key},
        std::pair{"DEL", NamedKey::delete_key},
        std::pair{"SPACE", NamedKey::space},
        std::pair{"LEFT", NamedKey::left},
        std::pair{"UP", NamedKey::up},
        std::pair{"RIGHT", NamedKey::right},
        std::pair{"DOWN", NamedKey::down},
        std::pair{"HOME", NamedKey::home},
        std::pair{"END", NamedKey::end},
        std::pair{"F4", NamedKey::f4},
    };
    const auto found = std::find_if(
        pairs.begin(), pairs.end(), [value](const auto& item) {
            return item.first == value;
        });
    return found == pairs.end()
               ? std::nullopt
               : std::optional<NamedKey>(found->second);
}

}  // namespace

KeyChordParseResult parse_key_chord(
    const std::string& value) {
    if (value.empty() || value.size() > 64U) {
        return KeyChordParseResult{
            std::nullopt,
            "Key chord must contain 1..64 UTF-8 bytes.",
        };
    }
    std::vector<std::string> parts;
    std::size_t start = 0U;
    while (start <= value.size()) {
        const std::size_t end = value.find('+', start);
        const auto part = trim_upper(std::string_view(value).substr(
            start,
            end == std::string::npos
                ? std::string::npos
                : end - start));
        if (part.empty()) {
            return KeyChordParseResult{
                std::nullopt,
                "Key chord contains an empty key name.",
            };
        }
        parts.push_back(part);
        if (end == std::string::npos) {
            break;
        }
        start = end + 1U;
    }
    if (parts.size() > 4U) {
        return KeyChordParseResult{
            std::nullopt,
            "Key chord supports at most three modifiers and one key.",
        };
    }

    KeyChord chord{{}, NamedKey::ascii, '\0', {}};
    for (std::size_t index = 0U;
         index + 1U < parts.size();
         ++index) {
        const auto parsed = modifier(parts[index]);
        if (!parsed.has_value() ||
            std::find(
                chord.modifiers.begin(),
                chord.modifiers.end(),
                *parsed) != chord.modifiers.end()) {
            return KeyChordParseResult{
                std::nullopt,
                "Key chord contains an unsupported or duplicate modifier.",
            };
        }
        chord.modifiers.push_back(*parsed);
    }
    const auto& final = parts.back();
    const auto named = named_key(final);
    if (named.has_value()) {
        chord.key = *named;
    } else if (
        final.size() == 1U &&
        static_cast<unsigned char>(final.front()) < 128U &&
        std::isalnum(
            static_cast<unsigned char>(final.front())) != 0) {
        chord.key = NamedKey::ascii;
        chord.ascii_character = final.front();
    } else {
        return KeyChordParseResult{
            std::nullopt,
            "Key chord final key is not in the certified allowlist.",
        };
    }
    chord.normalized.clear();
    for (const auto parsed : chord.modifiers) {
        if (!chord.normalized.empty()) {
            chord.normalized += '+';
        }
        switch (parsed) {
            case KeyModifier::control:
                chord.normalized += "CTRL";
                break;
            case KeyModifier::alt:
                chord.normalized += "ALT";
                break;
            case KeyModifier::shift:
                chord.normalized += "SHIFT";
                break;
        }
    }
    if (!chord.normalized.empty()) {
        chord.normalized += '+';
    }
    chord.normalized += final;
    return KeyChordParseResult{
        std::move(chord),
        {},
    };
}

}  // namespace act::components
