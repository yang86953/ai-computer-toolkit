#include "platform/windows/ffmpeg_encoder.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <fstream>
#include <limits>
#include <string>

namespace act::platform::windows {
namespace {

struct OwnedHandle {
    HANDLE value = nullptr;

    ~OwnedHandle() {
        close();
    }

    void close() {
        if (value != nullptr &&
            value != INVALID_HANDLE_VALUE) {
            CloseHandle(value);
        }
        value = nullptr;
    }
};

FfmpegEncodeResult failure(
    const std::string& code,
    const std::string& message) {
    return FfmpegEncodeResult{
        std::nullopt,
        BackendError{code, message},
    };
}

std::optional<std::filesystem::path> ffmpeg_path() {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length = SearchPathW(
        nullptr,
        L"ffmpeg.exe",
        nullptr,
        static_cast<DWORD>(buffer.size()),
        buffer.data(),
        nullptr);
    if (length == 0U || length >= buffer.size()) {
        return std::nullopt;
    }
    std::error_code error;
    auto path = std::filesystem::absolute(
        std::filesystem::path(buffer.data()), error);
    const auto status =
        std::filesystem::symlink_status(path, error);
    if (error ||
        !std::filesystem::is_regular_file(status) ||
        std::filesystem::is_symlink(status)) {
        return std::nullopt;
    }
    return path.lexically_normal();
}

std::wstring quoted(const std::filesystem::path& path) {
    return L"\"" + path.native() + L"\"";
}

bool valid_mp4(
    const std::filesystem::path& path,
    std::uint64_t& bytes) {
    std::error_code error;
    bytes = std::filesystem::file_size(path, error);
    if (error || bytes < 12U ||
        bytes > 512ULL * 1024ULL * 1024ULL) {
        return false;
    }
    std::ifstream input(path, std::ios::binary);
    std::array<char, 12> header{};
    input.read(
        header.data(),
        static_cast<std::streamsize>(header.size()));
    return input.gcount() ==
               static_cast<std::streamsize>(header.size()) &&
           header[4] == 'f' &&
           header[5] == 't' &&
           header[6] == 'y' &&
           header[7] == 'p';
}

bool write_frame(
    const HANDLE pipe,
    const components::RgbaImage& frame) {
    std::size_t offset = 0U;
    while (offset < frame.pixels.size()) {
        const auto remaining =
            frame.pixels.size() - offset;
        const DWORD chunk = static_cast<DWORD>(
            std::min<std::size_t>(
                remaining,
                std::numeric_limits<DWORD>::max()));
        DWORD written = 0U;
        if (WriteFile(
                pipe,
                frame.pixels.data() + offset,
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

}  // namespace

FfmpegEncodeResult encode_video_stream(
    const FfmpegEncodeConfig& config,
    const std::uint32_t expected_frames,
    const FfmpegFrameProducer& producer) {
    if (config.width < 2U ||
        config.height < 2U ||
        config.width > 1920U ||
        config.height > 1920U ||
        (config.width & 1U) != 0U ||
        (config.height & 1U) != 0U ||
        config.fps < 1U ||
        config.fps > 10U ||
        config.crf < 18U ||
        config.crf > 40U ||
        config.timeout_ms < 1000U ||
        config.timeout_ms > 30000U ||
        expected_frames < 2U ||
        expected_frames > 3000U) {
        return failure(
            "INVALID_ARGUMENT",
            "The streaming encoder input violates certified bounds.");
    }
    std::error_code error;
    const auto directory = std::filesystem::absolute(
        config.staging_directory, error).lexically_normal();
    const auto directory_status =
        std::filesystem::symlink_status(directory, error);
    if (error ||
        !std::filesystem::is_directory(directory_status) ||
        std::filesystem::is_symlink(directory_status)) {
        return failure(
            "INVALID_ARGUMENT",
            "The streaming encoder requires a real staging directory.");
    }
    const std::uint64_t frame_bytes =
        static_cast<std::uint64_t>(config.width) *
        static_cast<std::uint64_t>(config.height) * 4ULL;
    if (frame_bytes >
        static_cast<std::uint64_t>(
            std::numeric_limits<std::size_t>::max())) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "The streaming frame byte count overflows this runtime.");
    }
    const auto runtime = ffmpeg_path();
    if (!runtime.has_value()) {
        return failure(
            "VIDEO_ENCODER_UNAVAILABLE",
            "A regular ffmpeg.exe was not found through private "
            "runtime discovery.");
    }
    const std::wstring suffix =
        std::to_wstring(GetCurrentProcessId()) + L"-" +
        std::to_wstring(GetTickCount64());
    const auto output =
        directory / (L"recording-" + suffix + L".mp4");
    if (std::filesystem::exists(output, error)) {
        return failure(
            "OUTPUT_COLLISION",
            "The private recording output name already exists.");
    }

    SECURITY_ATTRIBUTES security{};
    security.nLength = sizeof(security);
    security.bInheritHandle = TRUE;
    HANDLE read_value = nullptr;
    HANDLE write_value = nullptr;
    if (CreatePipe(
            &read_value,
            &write_value,
            &security,
            0U) == FALSE) {
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder input pipe could not be created.");
    }
    OwnedHandle pipe_read{read_value};
    OwnedHandle pipe_write{write_value};
    if (SetHandleInformation(
            pipe_write.value,
            HANDLE_FLAG_INHERIT,
            0U) == FALSE) {
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder input pipe inheritance could not be bounded.");
    }
    OwnedHandle null_output{
        CreateFileW(
            L"NUL",
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            nullptr)};
    OwnedHandle job{CreateJobObjectW(nullptr, nullptr)};
    if (null_output.value == INVALID_HANDLE_VALUE ||
        job.value == nullptr) {
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder process boundary could not be created.");
    }
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (SetInformationJobObject(
            job.value,
            JobObjectExtendedLimitInformation,
            &limits,
            sizeof(limits)) == FALSE) {
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder Job policy could not be applied.");
    }

    const std::wstring dimensions =
        std::to_wstring(config.width) + L"x" +
        std::to_wstring(config.height);
    const std::wstring group =
        std::to_wstring(config.fps * 4U);
    std::wstring command =
        quoted(*runtime) +
        L" -hide_banner -loglevel error -nostdin -y"
        L" -f rawvideo -pix_fmt rgba -video_size " +
        dimensions +
        L" -framerate " + std::to_wstring(config.fps) +
        L" -i pipe:0 -an -c:v libx264 -preset veryfast"
        L" -tune stillimage -crf " +
        std::to_wstring(config.crf) +
        L" -g " + group +
        L" -pix_fmt yuv420p -movflags +faststart"
        L" -f mp4 " + quoted(output);
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = pipe_read.value;
    startup.hStdOutput = null_output.value;
    startup.hStdError = null_output.value;
    PROCESS_INFORMATION process{};
    if (CreateProcessW(
            runtime->c_str(),
            command.data(),
            nullptr,
            nullptr,
            TRUE,
            CREATE_NO_WINDOW | CREATE_SUSPENDED |
                CREATE_UNICODE_ENVIRONMENT,
            nullptr,
            directory.c_str(),
            &startup,
            &process) == FALSE) {
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The fixed streaming encoder could not be started.");
    }
    OwnedHandle process_handle{process.hProcess};
    OwnedHandle thread_handle{process.hThread};
    if (AssignProcessToJobObject(
            job.value, process_handle.value) == FALSE ||
        ResumeThread(thread_handle.value) ==
            static_cast<DWORD>(-1)) {
        TerminateJobObject(job.value, 1U);
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder did not enter its termination boundary.");
    }
    pipe_read.close();

    std::uint32_t encoded_frames = 0U;
    const std::size_t expected_bytes =
        static_cast<std::size_t>(frame_bytes);
    const auto produced = producer(
        [&](const components::RgbaImage& frame) {
            if (frame.width != config.width ||
                frame.height != config.height ||
                frame.pixels.size() != expected_bytes ||
                encoded_frames >= expected_frames ||
                !write_frame(pipe_write.value, frame)) {
                return false;
            }
            ++encoded_frames;
            return true;
        });
    pipe_write.close();
    if (produced.has_value() ||
        encoded_frames != expected_frames) {
        TerminateJobObject(job.value, 1U);
        WaitForSingleObject(process_handle.value, 1000U);
        std::filesystem::remove(output, error);
        return produced.has_value()
            ? failure(produced->code, produced->message)
            : failure(
                  "VIDEO_ENCODER_WRITE_FAILED",
                  "The producer did not stream the exact frame budget.");
    }
    const DWORD wait = WaitForSingleObject(
        process_handle.value, config.timeout_ms);
    if (wait != WAIT_OBJECT_0) {
        TerminateJobObject(job.value, 1U);
        WaitForSingleObject(process_handle.value, 1000U);
        std::filesystem::remove(output, error);
        return failure(
            wait == WAIT_TIMEOUT
                ? "TIMEOUT"
                : "VIDEO_ENCODER_FAILED",
            "The fixed encoder did not complete within its bound.");
    }
    DWORD exit_code = 1U;
    if (GetExitCodeProcess(
            process_handle.value, &exit_code) == FALSE ||
        exit_code != 0U) {
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_ENCODER_FAILED",
            "ffmpeg rejected the fixed streaming H.264 pipeline.");
    }
    std::uint64_t output_bytes = 0U;
    if (!valid_mp4(output, output_bytes)) {
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_OUTPUT_INVALID",
            "The encoder output lacks a bounded MP4 ftyp box.");
    }
    return FfmpegEncodeResult{
        FfmpegEncodeEvidence{
            output,
            output_bytes,
            encoded_frames,
            true,
            true,
            true,
            false,
            false,
            true,
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
