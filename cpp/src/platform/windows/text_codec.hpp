#pragma once

#include <string>
#include <string_view>

namespace act::platform::windows {

[[nodiscard]] std::string utf8(std::wstring_view value);
[[nodiscard]] std::wstring wide(std::string_view value);

}  // namespace act::platform::windows
