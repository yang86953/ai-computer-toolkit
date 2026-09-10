#include "components/opaque_id.hpp"

#include <cctype>
#include <cstdint>
#include <iomanip>
#include <sstream>

namespace act::components {

std::string opaque_id(
    const char target_kind,
    const std::string_view identity) {
    std::uint64_t hash = 14695981039346656037ULL;
    for (const unsigned char character : identity) {
        hash ^= static_cast<std::uint64_t>(character);
        hash *= 1099511628211ULL;
    }
    std::ostringstream output;
    output << "s2:" << target_kind << ':' << std::hex << std::setw(16)
           << std::setfill('0') << hash;
    return output.str();
}

std::string normalized_name(const std::string_view value) {
    std::string output;
    output.reserve(value.size());
    for (const unsigned char character : value) {
        if (character < 0x80U && std::isalnum(character) != 0) {
            output.push_back(
                static_cast<char>(std::tolower(character)));
        }
    }
    return output;
}

}  // namespace act::components
