#pragma once

namespace act::components {

void request_global_cancellation() noexcept;
[[nodiscard]] bool global_cancellation_requested() noexcept;

}  // namespace act::components
