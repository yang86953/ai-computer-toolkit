#pragma once

#include <string>
#include <string_view>

namespace act::components {

[[nodiscard]] std::string opaque_id(char target_kind, std::string_view identity);
[[nodiscard]] std::string normalized_name(std::string_view value);

}  // namespace act::components
