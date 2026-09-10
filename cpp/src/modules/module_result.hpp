#pragma once

#include "components/json.hpp"

#include <optional>
#include <string>

namespace act::modules {

struct ModuleResult {
    bool ok;
    std::string error_code;
    std::string error_message;
    components::Json data;
    std::optional<components::Json> error_details =
        std::nullopt;
};

}  // namespace act::modules
