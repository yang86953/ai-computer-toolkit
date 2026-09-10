#include "components/utf8.hpp"

#include <cstdint>

namespace act::components {

bool valid_utf8(const std::string_view value) {
    std::size_t index = 0U;
    while (index < value.size()) {
        const auto first =
            static_cast<std::uint8_t>(value[index]);
        std::size_t continuation_count = 0U;
        std::uint32_t code_point = 0U;
        if (first <= 0x7FU) {
            ++index;
            continue;
        }
        if ((first & 0xE0U) == 0xC0U) {
            continuation_count = 1U;
            code_point = first & 0x1FU;
        } else if ((first & 0xF0U) == 0xE0U) {
            continuation_count = 2U;
            code_point = first & 0x0FU;
        } else if ((first & 0xF8U) == 0xF0U) {
            continuation_count = 3U;
            code_point = first & 0x07U;
        } else {
            return false;
        }
        if (index + continuation_count >= value.size()) {
            return false;
        }
        for (std::size_t offset = 1U;
             offset <= continuation_count;
             ++offset) {
            const auto continuation =
                static_cast<std::uint8_t>(value[index + offset]);
            if ((continuation & 0xC0U) != 0x80U) {
                return false;
            }
            code_point =
                (code_point << 6U) | (continuation & 0x3FU);
        }
        const bool overlong =
            (continuation_count == 1U && code_point < 0x80U) ||
            (continuation_count == 2U && code_point < 0x800U) ||
            (continuation_count == 3U && code_point < 0x10000U);
        if (overlong ||
            code_point > 0x10FFFFU ||
            (code_point >= 0xD800U && code_point <= 0xDFFFU)) {
            return false;
        }
        index += continuation_count + 1U;
    }
    return true;
}

}  // namespace act::components
