#include "components/cancellation.hpp"
#include "components/json.hpp"
#include "components/worker_process.hpp"

#include <chrono>
#include <iostream>
#include <thread>

int main() {
    const act::components::WorkerProcess worker;
    const auto timeout = worker.run_companion(
        "act-worker-fixture.exe", "{}", 25U, 1024U);
    if (timeout.completion !=
            act::components::WorkerCompletion::timed_out ||
        !timeout.job_terminated) {
        std::cerr << "timeout did not terminate the worker job\n";
        return 1;
    }

    std::thread cancellation([] {
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
        act::components::request_global_cancellation();
    });
    const auto cancelled = worker.run_companion(
        "act-worker-fixture.exe", "{}", 5000U, 1024U);
    cancellation.join();
    if (cancelled.completion !=
            act::components::WorkerCompletion::cancelled ||
        !cancelled.job_terminated) {
        std::cerr << "cancellation did not terminate the worker job\n";
        return 1;
    }

    std::cout << act::components::object({
        {"ok", true},
        {"timeoutJobTerminated", timeout.job_terminated},
        {"cancellationJobTerminated", cancelled.job_terminated},
    }).dump() << '\n';
    return 0;
}
