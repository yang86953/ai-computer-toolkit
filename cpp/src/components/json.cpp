#include "components/json.hpp"

#include <array>
#include <charconv>
#include <cctype>
#include <cmath>
#include <iomanip>
#include <limits>
#include <sstream>
#include <type_traits>

namespace act::components {
namespace {

std::string escape_string(const std::string& input) {
    std::ostringstream output;
    output << '"';
    for (const unsigned char character : input) {
        switch (character) {
            case '"':
                output << "\\\"";
                break;
            case '\\':
                output << "\\\\";
                break;
            case '\b':
                output << "\\b";
                break;
            case '\f':
                output << "\\f";
                break;
            case '\n':
                output << "\\n";
                break;
            case '\r':
                output << "\\r";
                break;
            case '\t':
                output << "\\t";
                break;
            default:
                if (character < 0x20U) {
                    output << "\\u" << std::hex << std::setw(4)
                           << std::setfill('0') << static_cast<int>(character)
                           << std::dec;
                } else {
                    output << static_cast<char>(character);
                }
        }
    }
    output << '"';
    return output.str();
}

std::string dump_double(const double value) {
    if (!std::isfinite(value)) {
        return "null";
    }
    std::array<char, 64> buffer{};
    const auto result = std::to_chars(
        buffer.data(),
        buffer.data() + buffer.size(),
        value,
        std::chars_format::general,
        std::numeric_limits<double>::max_digits10);
    if (result.ec != std::errc{}) {
        return "null";
    }
    std::string output(buffer.data(), result.ptr);
    if (output.find_first_of(".eE") == std::string::npos) {
        output += ".0";
    }
    return output;
}

class Parser final {
public:
    explicit Parser(const std::string_view input) : input_(input) {}

    std::optional<Json> parse(std::string& error) {
        skip_space();
        auto value = parse_value(error);
        if (!value.has_value()) {
            return std::nullopt;
        }
        skip_space();
        if (position_ != input_.size()) {
            error = "Unexpected trailing JSON content.";
            return std::nullopt;
        }
        return value;
    }

private:
    void skip_space() {
        while (position_ < input_.size() &&
               std::isspace(static_cast<unsigned char>(input_[position_])) !=
                   0) {
            ++position_;
        }
    }

    bool consume(const char expected) {
        if (position_ >= input_.size() || input_[position_] != expected) {
            return false;
        }
        ++position_;
        return true;
    }

    bool consume_literal(const std::string_view literal) {
        if (input_.substr(position_, literal.size()) != literal) {
            return false;
        }
        position_ += literal.size();
        return true;
    }

    static void append_utf8(std::string& output, const std::uint32_t code) {
        if (code <= 0x7FU) {
            output.push_back(static_cast<char>(code));
        } else if (code <= 0x7FFU) {
            output.push_back(static_cast<char>(0xC0U | (code >> 6U)));
            output.push_back(static_cast<char>(0x80U | (code & 0x3FU)));
        } else if (code <= 0xFFFFU) {
            output.push_back(static_cast<char>(0xE0U | (code >> 12U)));
            output.push_back(
                static_cast<char>(0x80U | ((code >> 6U) & 0x3FU)));
            output.push_back(static_cast<char>(0x80U | (code & 0x3FU)));
        } else {
            output.push_back(static_cast<char>(0xF0U | (code >> 18U)));
            output.push_back(
                static_cast<char>(0x80U | ((code >> 12U) & 0x3FU)));
            output.push_back(
                static_cast<char>(0x80U | ((code >> 6U) & 0x3FU)));
            output.push_back(static_cast<char>(0x80U | (code & 0x3FU)));
        }
    }

    std::optional<std::uint32_t> parse_hex_quad(std::string& error) {
        if (position_ + 4U > input_.size()) {
            error = "Incomplete JSON unicode escape.";
            return std::nullopt;
        }
        std::uint32_t value = 0;
        for (std::size_t index = 0; index < 4U; ++index) {
            const char character = input_[position_++];
            value <<= 4U;
            if (character >= '0' && character <= '9') {
                value |= static_cast<std::uint32_t>(character - '0');
            } else if (character >= 'a' && character <= 'f') {
                value |= static_cast<std::uint32_t>(character - 'a' + 10);
            } else if (character >= 'A' && character <= 'F') {
                value |= static_cast<std::uint32_t>(character - 'A' + 10);
            } else {
                error = "Invalid JSON unicode escape.";
                return std::nullopt;
            }
        }
        return value;
    }

    std::optional<std::string> parse_string(std::string& error) {
        if (!consume('"')) {
            error = "Expected a JSON string.";
            return std::nullopt;
        }
        std::string value;
        while (position_ < input_.size()) {
            const char character = input_[position_++];
            if (character == '"') {
                return value;
            }
            if (static_cast<unsigned char>(character) < 0x20U) {
                error = "Unescaped control character in JSON string.";
                return std::nullopt;
            }
            if (character != '\\') {
                value.push_back(character);
                continue;
            }
            if (position_ >= input_.size()) {
                error = "Incomplete JSON string escape.";
                return std::nullopt;
            }
            const char escaped = input_[position_++];
            switch (escaped) {
                case '"':
                case '\\':
                case '/':
                    value.push_back(escaped);
                    break;
                case 'b':
                    value.push_back('\b');
                    break;
                case 'f':
                    value.push_back('\f');
                    break;
                case 'n':
                    value.push_back('\n');
                    break;
                case 'r':
                    value.push_back('\r');
                    break;
                case 't':
                    value.push_back('\t');
                    break;
                case 'u': {
                    auto code = parse_hex_quad(error);
                    if (!code.has_value()) {
                        return std::nullopt;
                    }
                    if (*code >= 0xD800U && *code <= 0xDBFFU) {
                        if (position_ + 2U > input_.size() ||
                            input_[position_] != '\\' ||
                            input_[position_ + 1U] != 'u') {
                            error = "Missing low surrogate in JSON string.";
                            return std::nullopt;
                        }
                        position_ += 2U;
                        auto low = parse_hex_quad(error);
                        if (!low.has_value() ||
                            *low < 0xDC00U ||
                            *low > 0xDFFFU) {
                            error = "Invalid low surrogate in JSON string.";
                            return std::nullopt;
                        }
                        *code = 0x10000U +
                            ((*code - 0xD800U) << 10U) +
                            (*low - 0xDC00U);
                    } else if (*code >= 0xDC00U && *code <= 0xDFFFU) {
                        error = "Unexpected low surrogate in JSON string.";
                        return std::nullopt;
                    }
                    append_utf8(value, *code);
                    break;
                }
                default:
                    error = "Unknown JSON string escape.";
                    return std::nullopt;
            }
        }
        error = "Unterminated JSON string.";
        return std::nullopt;
    }

    std::optional<Json> parse_number(std::string& error) {
        const std::size_t start = position_;
        if (position_ < input_.size() && input_[position_] == '-') {
            ++position_;
        }
        if (position_ >= input_.size()) {
            error = "Incomplete JSON number.";
            return std::nullopt;
        }
        if (input_[position_] == '0') {
            ++position_;
            if (position_ < input_.size() &&
                std::isdigit(static_cast<unsigned char>(
                    input_[position_])) != 0) {
                error = "JSON numbers cannot contain leading zeroes.";
                return std::nullopt;
            }
        } else {
            const std::size_t digits = position_;
            while (position_ < input_.size() &&
                   std::isdigit(static_cast<unsigned char>(
                       input_[position_])) != 0) {
                ++position_;
            }
            if (digits == position_) {
                error = "Expected digits in JSON number.";
                return std::nullopt;
            }
        }
        bool floating = false;
        if (position_ < input_.size() &&
            input_[position_] == '.') {
            floating = true;
            ++position_;
            const std::size_t digits = position_;
            while (position_ < input_.size() &&
                   std::isdigit(static_cast<unsigned char>(
                       input_[position_])) != 0) {
                ++position_;
            }
            if (digits == position_) {
                error = "JSON fraction requires digits.";
                return std::nullopt;
            }
        }
        if (position_ < input_.size() &&
            (input_[position_] == 'e' ||
             input_[position_] == 'E')) {
            floating = true;
            ++position_;
            if (position_ < input_.size() &&
                (input_[position_] == '+' ||
                 input_[position_] == '-')) {
                ++position_;
            }
            const std::size_t digits = position_;
            while (position_ < input_.size() &&
                   std::isdigit(static_cast<unsigned char>(
                       input_[position_])) != 0) {
                ++position_;
            }
            if (digits == position_) {
                error = "JSON exponent requires digits.";
                return std::nullopt;
            }
        }
        if (floating) {
            double value = 0.0;
            const auto result = std::from_chars(
                input_.data() + start,
                input_.data() + position_,
                value,
                std::chars_format::general);
            if (result.ec != std::errc{} ||
                result.ptr != input_.data() + position_ ||
                !std::isfinite(value)) {
                error = "JSON number is outside the finite double range.";
                return std::nullopt;
            }
            return Json(value);
        }
        std::int64_t value = 0;
        const auto result = std::from_chars(
            input_.data() + start, input_.data() + position_, value);
        if (result.ec != std::errc{} ||
            result.ptr != input_.data() + position_) {
            error = "JSON integer is outside the signed 64-bit range.";
            return std::nullopt;
        }
        return Json(value);
    }

    std::optional<Json> parse_array(std::string& error) {
        consume('[');
        Json::Array values;
        skip_space();
        if (consume(']')) {
            return Json(std::move(values));
        }
        while (true) {
            skip_space();
            auto value = parse_value(error);
            if (!value.has_value()) {
                return std::nullopt;
            }
            values.push_back(std::move(*value));
            skip_space();
            if (consume(']')) {
                return Json(std::move(values));
            }
            if (!consume(',')) {
                error = "Expected ',' or ']' in JSON array.";
                return std::nullopt;
            }
        }
    }

    std::optional<Json> parse_object(std::string& error) {
        consume('{');
        Json::Object fields;
        skip_space();
        if (consume('}')) {
            return Json(std::move(fields));
        }
        while (true) {
            skip_space();
            auto key = parse_string(error);
            if (!key.has_value()) {
                return std::nullopt;
            }
            skip_space();
            if (!consume(':')) {
                error = "Expected ':' in JSON object.";
                return std::nullopt;
            }
            skip_space();
            auto value = parse_value(error);
            if (!value.has_value()) {
                return std::nullopt;
            }
            fields.emplace_back(std::move(*key), std::move(*value));
            skip_space();
            if (consume('}')) {
                return Json(std::move(fields));
            }
            if (!consume(',')) {
                error = "Expected ',' or '}' in JSON object.";
                return std::nullopt;
            }
        }
    }

    std::optional<Json> parse_value(std::string& error) {
        if (position_ >= input_.size()) {
            error = "Expected a JSON value.";
            return std::nullopt;
        }
        switch (input_[position_]) {
            case '"': {
                auto value = parse_string(error);
                return value.has_value()
                    ? std::optional<Json>(Json(std::move(*value)))
                    : std::nullopt;
            }
            case '{':
                return parse_object(error);
            case '[':
                return parse_array(error);
            case 't':
                if (consume_literal("true")) {
                    return Json(true);
                }
                break;
            case 'f':
                if (consume_literal("false")) {
                    return Json(false);
                }
                break;
            case 'n':
                if (consume_literal("null")) {
                    return Json(nullptr);
                }
                break;
            default:
                if (input_[position_] == '-' ||
                    std::isdigit(
                        static_cast<unsigned char>(input_[position_])) != 0) {
                    return parse_number(error);
                }
                break;
        }
        error = "Invalid JSON value.";
        return std::nullopt;
    }

    std::string_view input_;
    std::size_t position_ = 0;
};

}  // namespace

Json::Json() : value_(nullptr) {}
Json::Json(std::nullptr_t) : value_(nullptr) {}
Json::Json(const bool value) : value_(value) {}
Json::Json(const std::int64_t value) : value_(value) {}
Json::Json(const int value) : value_(static_cast<std::int64_t>(value)) {}
Json::Json(const double value) : value_(value) {}
Json::Json(std::string value) : value_(std::move(value)) {}
Json::Json(const char* value) : value_(std::string(value)) {}
Json::Json(Array value) : value_(std::move(value)) {}
Json::Json(Object value) : value_(std::move(value)) {}

std::string Json::dump() const {
    return std::visit(
        [](const auto& value) -> std::string {
            using T = std::decay_t<decltype(value)>;
            if constexpr (std::is_same_v<T, std::nullptr_t>) {
                return "null";
            } else if constexpr (std::is_same_v<T, bool>) {
                return value ? "true" : "false";
            } else if constexpr (std::is_same_v<T, std::int64_t>) {
                return std::to_string(value);
            } else if constexpr (std::is_same_v<T, double>) {
                return dump_double(value);
            } else if constexpr (std::is_same_v<T, std::string>) {
                return escape_string(value);
            } else if constexpr (std::is_same_v<T, Array>) {
                std::string output = "[";
                for (std::size_t index = 0; index < value.size(); ++index) {
                    if (index != 0U) {
                        output += ',';
                    }
                    output += value[index].dump();
                }
                output += ']';
                return output;
            } else {
                std::string output = "{";
                for (std::size_t index = 0; index < value.size(); ++index) {
                    if (index != 0U) {
                        output += ',';
                    }
                    output += escape_string(value[index].first);
                    output += ':';
                    output += value[index].second.dump();
                }
                output += '}';
                return output;
            }
        },
        value_);
}

std::string Json::dump_pretty(
    const std::size_t indentation) const {
    return dump_pretty_impl(0U, indentation);
}

std::string Json::dump_pretty_impl(
    const std::size_t level,
    const std::size_t indentation) const {
    return std::visit(
        [level, indentation](
            const auto& value) -> std::string {
            using T = std::decay_t<decltype(value)>;
            if constexpr (std::is_same_v<T, std::nullptr_t>) {
                return "null";
            } else if constexpr (std::is_same_v<T, bool>) {
                return value ? "true" : "false";
            } else if constexpr (std::is_same_v<T, std::int64_t>) {
                return std::to_string(value);
            } else if constexpr (std::is_same_v<T, double>) {
                return dump_double(value);
            } else if constexpr (std::is_same_v<T, std::string>) {
                return escape_string(value);
            } else if constexpr (std::is_same_v<T, Array>) {
                if (value.empty()) {
                    return "[]";
                }
                std::string output = "[\n";
                for (std::size_t index = 0;
                     index < value.size();
                     ++index) {
                    if (index != 0U) {
                        output += ",\n";
                    }
                    output += std::string(
                        (level + 1U) * indentation, ' ');
                    output += value[index].dump_pretty_impl(
                        level + 1U, indentation);
                }
                output += '\n';
                output += std::string(level * indentation, ' ');
                output += ']';
                return output;
            } else {
                if (value.empty()) {
                    return "{}";
                }
                std::string output = "{\n";
                for (std::size_t index = 0;
                     index < value.size();
                     ++index) {
                    if (index != 0U) {
                        output += ",\n";
                    }
                    output += std::string(
                        (level + 1U) * indentation, ' ');
                    output += escape_string(value[index].first);
                    output += ": ";
                    output += value[index].second.dump_pretty_impl(
                        level + 1U, indentation);
                }
                output += '\n';
                output += std::string(level * indentation, ' ');
                output += '}';
                return output;
            }
        },
        value_);
}

std::optional<Json> Json::parse(
    const std::string_view input,
    std::string& error) {
    error.clear();
    return Parser(input).parse(error);
}

const Json* Json::find(const std::string_view key) const {
    const auto* fields = std::get_if<Object>(&value_);
    if (fields == nullptr) {
        return nullptr;
    }
    for (const auto& [name, value] : *fields) {
        if (name == key) {
            return &value;
        }
    }
    return nullptr;
}

const std::string* Json::string_value() const {
    return std::get_if<std::string>(&value_);
}

const bool* Json::bool_value() const {
    return std::get_if<bool>(&value_);
}

const std::int64_t* Json::integer_value() const {
    return std::get_if<std::int64_t>(&value_);
}

const double* Json::double_value() const {
    return std::get_if<double>(&value_);
}

const Json::Array* Json::array_items() const {
    return std::get_if<Array>(&value_);
}

const Json::Object* Json::object_items() const {
    return std::get_if<Object>(&value_);
}

Json object(
    const std::initializer_list<std::pair<std::string, Json>> fields) {
    return Json(Json::Object(fields));
}

Json array(const std::initializer_list<Json> items) {
    return Json(Json::Array(items));
}

}  // namespace act::components
