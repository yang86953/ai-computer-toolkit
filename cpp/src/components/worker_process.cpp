#include "components/worker_process.hpp"

#include "components/cancellation.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <functional>
#include <limits>
#include <string_view>
#include <thread>

namespace act::components {
namespace {

class Handle final {
public:
    Handle() = default;
    explicit Handle(HANDLE value) : value_(value) {}
    ~Handle() {
        reset();
    }

    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;

    Handle(Handle&& other) noexcept : value_(other.release()) {}
    Handle& operator=(Handle&& other) noexcept {
        if (this != &other) {
            reset(other.release());
        }
        return *this;
    }

    [[nodiscard]] HANDLE get() const {
        return value_;
    }
    [[nodiscard]] bool valid() const {
        return value_ != nullptr && value_ != INVALID_HANDLE_VALUE;
    }
    [[nodiscard]] HANDLE release() {
        const HANDLE value = value_;
        value_ = nullptr;
        return value;
    }
    void reset(HANDLE value = nullptr) {
        if (valid()) {
            CloseHandle(value_);
        }
        value_ = value;
    }

private:
    HANDLE value_ = nullptr;
};

std::wstring companion_path(const std::string& executable_name) {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length = GetModuleFileNameW(
        nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0U || length >= buffer.size()) {
        return {};
    }
    std::wstring path(buffer.data(), length);
    const std::size_t separator = path.find_last_of(L"\\/");
    if (separator == std::wstring::npos) {
        return {};
    }
    path.resize(separator + 1U);

    if (executable_name.empty() ||
        executable_name.size() >
            static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        return {};
    }
    const int required = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        executable_name.data(),
        static_cast<int>(executable_name.size()),
        nullptr,
        0);
    if (required <= 0) {
        return {};
    }
    std::wstring wide_name(static_cast<std::size_t>(required), L'\0');
    const int converted = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        executable_name.data(),
        static_cast<int>(executable_name.size()),
        wide_name.data(),
        required);
    if (converted != required) {
        return {};
    }
    path += wide_name;
    return path;
}

bool create_pipe(Handle& read, Handle& write) {
    SECURITY_ATTRIBUTES attributes{
        sizeof(SECURITY_ATTRIBUTES), nullptr, TRUE};
    HANDLE raw_read = nullptr;
    HANDLE raw_write = nullptr;
    if (CreatePipe(&raw_read, &raw_write, &attributes, 0) == FALSE) {
        return false;
    }
    read.reset(raw_read);
    write.reset(raw_write);
    return true;
}

bool write_all(const HANDLE handle, const std::string_view content) {
    std::size_t offset = 0;
    while (offset < content.size()) {
        const std::size_t remaining = content.size() - offset;
        const DWORD chunk = static_cast<DWORD>(
            (std::min)(
                remaining,
                static_cast<std::size_t>(
                    std::numeric_limits<DWORD>::max())));
        DWORD written = 0;
        if (WriteFile(
                handle,
                content.data() + offset,
                chunk,
                &written,
                nullptr) == FALSE ||
            written == 0U) {
            return false;
        }
        offset += written;
    }
    return true;
}

void read_stream(
    const HANDLE handle,
    const std::size_t maximum_bytes,
    std::string& output,
    bool& overflow) {
    std::array<char, 8192> buffer{};
    while (true) {
        DWORD read = 0;
        if (ReadFile(
                handle,
                buffer.data(),
                static_cast<DWORD>(buffer.size()),
                &read,
                nullptr) == FALSE ||
            read == 0U) {
            break;
        }
        if (output.size() < maximum_bytes) {
            const std::size_t accepted = (std::min)(
                static_cast<std::size_t>(read),
                maximum_bytes - output.size());
            output.append(buffer.data(), accepted);
            overflow = overflow || accepted != read;
        } else {
            overflow = true;
        }
    }
}

WorkerProcessResult unavailable(const std::string& message) {
    return WorkerProcessResult{
        WorkerCompletion::unavailable,
        0U,
        {},
        message,
        false,
    };
}

bool terminate_worker(
    const HANDLE job,
    const HANDLE process,
    const std::uint32_t exit_code) {
    if (TerminateJobObject(job, exit_code) != FALSE) {
        return true;
    }
    return TerminateProcess(process, exit_code) != FALSE;
}

}  // namespace

WorkerProcessResult WorkerProcess::run_companion(
    const std::string& executable_name,
    const std::string& request,
    const std::uint32_t timeout_ms,
    const std::size_t maximum_output_bytes) const {
    const std::wstring path = companion_path(executable_name);
    if (path.empty() ||
        GetFileAttributesW(path.c_str()) == INVALID_FILE_ATTRIBUTES) {
        return unavailable("The isolated observation worker is unavailable.");
    }

    Handle stdin_read;
    Handle stdin_write;
    Handle stdout_read;
    Handle stdout_write;
    Handle stderr_read;
    Handle stderr_write;
    if (!create_pipe(stdin_read, stdin_write) ||
        !create_pipe(stdout_read, stdout_write) ||
        !create_pipe(stderr_read, stderr_write)) {
        return unavailable("Failed to create isolated worker pipes.");
    }
    if (SetHandleInformation(
            stdin_write.get(), HANDLE_FLAG_INHERIT, 0) == FALSE ||
        SetHandleInformation(
            stdout_read.get(), HANDLE_FLAG_INHERIT, 0) == FALSE ||
        SetHandleInformation(
            stderr_read.get(), HANDLE_FLAG_INHERIT, 0) == FALSE) {
        return unavailable("Failed to protect parent worker pipe handles.");
    }

    Handle job(CreateJobObjectW(nullptr, nullptr));
    if (!job.valid()) {
        return unavailable("Failed to create an isolated worker job.");
    }
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (SetInformationJobObject(
            job.get(),
            JobObjectExtendedLimitInformation,
            &limits,
            sizeof(limits)) == FALSE) {
        return unavailable("Failed to configure isolated worker cleanup.");
    }

    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = stdin_read.get();
    startup.hStdOutput = stdout_write.get();
    startup.hStdError = stderr_write.get();
    PROCESS_INFORMATION process_info{};
    std::wstring command_line = L"\"" + path + L"\"";
    const DWORD creation_flags =
        CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT;
    if (CreateProcessW(
            nullptr,
            command_line.data(),
            nullptr,
            nullptr,
            TRUE,
            creation_flags,
            nullptr,
            nullptr,
            &startup,
            &process_info) == FALSE) {
        return unavailable("Failed to start the isolated observation worker.");
    }

    Handle process(process_info.hProcess);
    Handle thread(process_info.hThread);
    if (AssignProcessToJobObject(job.get(), process.get()) == FALSE) {
        TerminateProcess(process.get(), 3U);
        WaitForSingleObject(process.get(), 5000U);
        return unavailable(
            "Failed to assign the observation worker to its cleanup job.");
    }

    stdin_read.reset();
    stdout_write.reset();
    stderr_write.reset();
    std::string stdout_text;
    std::string stderr_text;
    bool stdout_overflow = false;
    bool stderr_overflow = false;
    std::thread stdout_reader(
        read_stream,
        stdout_read.get(),
        maximum_output_bytes,
        std::ref(stdout_text),
        std::ref(stdout_overflow));
    std::thread stderr_reader(
        read_stream,
        stderr_read.get(),
        static_cast<std::size_t>(65536U),
        std::ref(stderr_text),
        std::ref(stderr_overflow));

    if (ResumeThread(thread.get()) == std::numeric_limits<DWORD>::max()) {
        TerminateJobObject(job.get(), 3U);
        stdin_write.reset();
        WaitForSingleObject(process.get(), 5000U);
        stdout_reader.join();
        stderr_reader.join();
        return unavailable("Failed to resume the isolated worker.");
    }
    thread.reset();

    const std::string request_line = request + '\n';
    if (!write_all(stdin_write.get(), request_line)) {
        TerminateJobObject(job.get(), 3U);
        stdin_write.reset();
        WaitForSingleObject(process.get(), 5000U);
        stdout_reader.join();
        stderr_reader.join();
        return unavailable("Failed to send the isolated worker request.");
    }
    stdin_write.reset();

    const auto deadline =
        std::chrono::steady_clock::now() +
        std::chrono::milliseconds(timeout_ms);
    WorkerCompletion completion = WorkerCompletion::completed;
    bool job_terminated = false;
    DWORD wait_status = WAIT_TIMEOUT;
    while ((wait_status = WaitForSingleObject(process.get(), 10U)) ==
           WAIT_TIMEOUT) {
        if (global_cancellation_requested()) {
            completion = WorkerCompletion::cancelled;
            job_terminated = terminate_worker(
                job.get(), process.get(), 4U);
            break;
        }
        if (std::chrono::steady_clock::now() >= deadline) {
            completion = WorkerCompletion::timed_out;
            job_terminated = terminate_worker(
                job.get(), process.get(), 5U);
            break;
        }
    }
    if (wait_status == WAIT_FAILED) {
        completion = WorkerCompletion::protocol_failure;
        job_terminated = terminate_worker(
            job.get(), process.get(), 6U);
    }
    if (WaitForSingleObject(process.get(), 5000U) == WAIT_TIMEOUT) {
        job_terminated =
            terminate_worker(job.get(), process.get(), 7U) ||
            job_terminated;
        WaitForSingleObject(process.get(), 5000U);
    }
    DWORD exit_code = 0U;
    GetExitCodeProcess(process.get(), &exit_code);
    if (completion == WorkerCompletion::completed) {
        job_terminated =
            TerminateJobObject(job.get(), 0U) != FALSE;
    }
    stdout_reader.join();
    stderr_reader.join();

    if (completion == WorkerCompletion::timed_out) {
        return WorkerProcessResult{
            completion,
            exit_code,
            {},
            "The isolated observation worker exceeded its deadline.",
            job_terminated,
        };
    }
    if (completion == WorkerCompletion::cancelled) {
        return WorkerProcessResult{
            completion,
            exit_code,
            {},
            "The isolated observation worker was cancelled.",
            job_terminated,
        };
    }
    if (completion == WorkerCompletion::protocol_failure) {
        return WorkerProcessResult{
            completion,
            exit_code,
            {},
            "Waiting for the isolated worker failed.",
            job_terminated,
        };
    }
    if (stdout_overflow || stderr_overflow) {
        return WorkerProcessResult{
            WorkerCompletion::protocol_failure,
            exit_code,
            {},
            "The isolated worker exceeded its bounded output allowance.",
            false,
        };
    }
    if (!stderr_text.empty()) {
        return WorkerProcessResult{
            WorkerCompletion::protocol_failure,
            exit_code,
            {},
            "The isolated worker wrote unexpected diagnostic output.",
            false,
        };
    }
    return WorkerProcessResult{
        WorkerCompletion::completed,
        exit_code,
        std::move(stdout_text),
        {},
        job_terminated,
    };
}

}  // namespace act::components
