#pragma once

#include <string_view>

namespace act::components {

[[nodiscard]] bool valid_utf8(std::string_view value);

}  // namespace act::components
