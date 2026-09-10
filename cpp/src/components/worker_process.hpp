#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

namespace act::components {

enum class WorkerCompletion {
    completed,
    timed_out,
    cancelled,
    unavailable,
    protocol_failure,
};

struct WorkerProcessResult {
    WorkerCompletion completion;
    std::uint32_t exit_code;
    std::string stdout_text;
    std::string error_message;
    bool job_terminated;
};

class WorkerProcess final {
public:
    [[nodiscard]] WorkerProcessResult run_companion(
        const std::string& executable_name,
        const std::string& request,
        std::uint32_t timeout_ms,
        std::size_t maximum_output_bytes) const;
};

}  // namespace act::components
