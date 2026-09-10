#include "components/cancellation.hpp"

#include <atomic>

namespace act::components {
namespace {

std::atomic_bool cancellation_requested{false};

}  // namespace

void request_global_cancellation() noexcept {
    cancellation_requested.store(true, std::memory_order_release);
}

bool global_cancellation_requested() noexcept {
    return cancellation_requested.load(std::memory_order_acquire);
}

}  // namespace act::components
