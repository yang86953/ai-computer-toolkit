#pragma once

#include "components/json.hpp"

#include <cstddef>
#include <optional>
#include <string>

namespace act::components {

struct JsonInputResult {
    std::optional<Json> value;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] JsonInputResult read_json_input(
    const std::string& source,
    std::size_t maximum_bytes = 1048576U);

}  // namespace act::components
