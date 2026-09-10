#pragma once

#include <cstdint>
#include <initializer_list>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

namespace act::components {

class Json final {
public:
    using Array = std::vector<Json>;
    using Object = std::vector<std::pair<std::string, Json>>;

    Json();
    Json(std::nullptr_t);
    Json(bool value);
    Json(std::int64_t value);
    Json(int value);
    Json(double value);
    Json(std::string value);
    Json(const char* value);
    Json(Array value);
    Json(Object value);

    [[nodiscard]] std::string dump() const;
    [[nodiscard]] std::string dump_pretty(
        std::size_t indentation = 2U) const;
    [[nodiscard]] static std::optional<Json> parse(
        std::string_view input,
        std::string& error);
    [[nodiscard]] const Json* find(std::string_view key) const;
    [[nodiscard]] const std::string* string_value() const;
    [[nodiscard]] const bool* bool_value() const;
    [[nodiscard]] const std::int64_t* integer_value() const;
    [[nodiscard]] const double* double_value() const;
    [[nodiscard]] const Array* array_items() const;
    [[nodiscard]] const Object* object_items() const;

private:
    [[nodiscard]] std::string dump_pretty_impl(
        std::size_t level,
        std::size_t indentation) const;

    using Value =
        std::variant<
            std::nullptr_t,
            bool,
            std::int64_t,
            double,
            std::string,
            Array,
            Object>;
    Value value_;
};

[[nodiscard]] Json object(
    std::initializer_list<std::pair<std::string, Json>> fields);
[[nodiscard]] Json array(std::initializer_list<Json> items);

}  // namespace act::components
