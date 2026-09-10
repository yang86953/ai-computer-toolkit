#include "platform/windows/ffmpeg_encoder.hpp"

#include <windows.h>

#include <array>
#include <fstream>
#include <limits>
#include <string>

namespace act::platform::windows {
namespace {

FfmpegEncodeResult failure(
    const char* code,
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
    if (error ||
        !std::filesystem::is_regular_file(path, error) ||
        error ||
        std::filesystem::is_symlink(
            std::filesystem::symlink_status(path, error)) ||
        error) {
        return std::nullopt;
    }
    return path.lexically_normal();
}

std::wstring quoted(const std::filesystem::path& path) {
    return L"\"" + path.native() + L"\"";
}

struct OwnedHandle {
    HANDLE value = nullptr;

    ~OwnedHandle() {
        if (value != nullptr &&
            value != INVALID_HANDLE_VALUE) {
            CloseHandle(value);
        }
    }
};

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

}  // namespace

FfmpegEncodeResult encode_fixture_video(
    const FfmpegEncodeConfig& config,
    const std::vector<components::RgbaImage>& frames) {
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
        frames.size() < 2U ||
        frames.size() > 20U) {
        return failure(
            "INVALID_ARGUMENT",
            "The fixture encoder input violates certified bounds.");
    }
    std::error_code error;
    const auto directory = std::filesystem::absolute(
        config.staging_directory, error).lexically_normal();
    if (error ||
        !std::filesystem::is_directory(directory, error) ||
        error ||
        std::filesystem::is_symlink(
            std::filesystem::symlink_status(directory, error)) ||
        error) {
        return failure(
            "INVALID_ARGUMENT",
            "The fixture encoder requires a real staging directory.");
    }
    const std::uint64_t frame_bytes =
        static_cast<std::uint64_t>(config.width) *
        static_cast<std::uint64_t>(config.height) * 4ULL;
    if (frame_bytes >
        static_cast<std::uint64_t>(
            std::numeric_limits<std::size_t>::max())) {
        return failure(
            "RESOURCE_LIMIT_EXCEEDED",
            "The fixture frame byte count overflows this runtime.");
    }
    for (const auto& frame : frames) {
        if (frame.width != config.width ||
            frame.height != config.height ||
            frame.pixels.size() !=
                static_cast<std::size_t>(frame_bytes)) {
            return failure(
                "INVALID_ARGUMENT",
                "A fixture frame does not match the encoder dimensions.");
        }
    }

    const std::wstring suffix =
        std::to_wstring(GetCurrentProcessId()) + L"-" +
        std::to_wstring(GetTickCount64());
    const auto raw = directory /
        (L"recording-" + suffix + L".rgba");
    const auto output = directory /
        (L"recording-" + suffix + L".mp4");
    if (std::filesystem::exists(raw, error) ||
        std::filesystem::exists(output, error)) {
        return failure(
            "OUTPUT_COLLISION",
            "The private recording staging name already exists.");
    }
    {
        std::ofstream stream(raw, std::ios::binary);
        if (!stream) {
            return failure(
                "VIDEO_ENCODER_WRITE_FAILED",
                "The private raw staging file could not be created.");
        }
        for (const auto& frame : frames) {
            stream.write(
                reinterpret_cast<const char*>(
                    frame.pixels.data()),
                static_cast<std::streamsize>(
                    frame.pixels.size()));
        }
        if (!stream) {
            stream.close();
            std::filesystem::remove(raw, error);
            return failure(
                "VIDEO_ENCODER_WRITE_FAILED",
                "The private raw staging file could not be written.");
        }
    }

    const auto runtime = ffmpeg_path();
    if (!runtime.has_value()) {
        std::filesystem::remove(raw, error);
        return failure(
            "VIDEO_ENCODER_UNAVAILABLE",
            "A regular ffmpeg.exe was not found through private "
            "runtime discovery.");
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
        L" -i " + quoted(raw) +
        L" -an -c:v libx264 -preset veryfast"
        L" -tune stillimage -crf " +
        std::to_wstring(config.crf) +
        L" -g " + group +
        L" -pix_fmt yuv420p -movflags +faststart"
        L" -f mp4 " + quoted(output);

    SECURITY_ATTRIBUTES security{};
    security.nLength = sizeof(security);
    security.bInheritHandle = TRUE;
    OwnedHandle null_input{
        CreateFileW(
            L"NUL",
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            nullptr)};
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
    if (null_input.value == INVALID_HANDLE_VALUE ||
        null_output.value == INVALID_HANDLE_VALUE ||
        job.value == nullptr) {
        std::filesystem::remove(raw, error);
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
        std::filesystem::remove(raw, error);
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder Job policy could not be applied.");
    }
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = null_input.value;
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
        std::filesystem::remove(raw, error);
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The fixed ffmpeg process could not be started.");
    }
    OwnedHandle process_handle{process.hProcess};
    OwnedHandle thread_handle{process.hThread};
    if (AssignProcessToJobObject(
            job.value, process_handle.value) == FALSE ||
        ResumeThread(thread_handle.value) ==
            static_cast<DWORD>(-1)) {
        TerminateJobObject(job.value, 1U);
        std::filesystem::remove(raw, error);
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_ENCODER_START_FAILED",
            "The encoder did not enter its termination boundary.");
    }
    const DWORD wait = WaitForSingleObject(
        process_handle.value, config.timeout_ms);
    if (wait != WAIT_OBJECT_0) {
        TerminateJobObject(job.value, 1U);
        WaitForSingleObject(process_handle.value, 1000U);
        std::filesystem::remove(raw, error);
        std::filesystem::remove(output, error);
        return failure(
            wait == WAIT_TIMEOUT ? "TIMEOUT" : "VIDEO_ENCODER_FAILED",
            "The fixed encoder did not complete within its bound.");
    }
    DWORD exit_code = 1U;
    const bool exited =
        GetExitCodeProcess(process_handle.value, &exit_code) != FALSE &&
        exit_code == 0U;
    std::filesystem::remove(raw, error);
    if (!exited) {
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_ENCODER_FAILED",
            "ffmpeg rejected the fixed H.264 MP4 pipeline.");
    }
    std::uint64_t output_bytes = 0U;
    if (!valid_mp4(output, output_bytes)) {
        std::filesystem::remove(output, error);
        return failure(
            "VIDEO_OUTPUT_INVALID",
            "The encoder output does not contain a bounded MP4 ftyp box.");
    }
    return FfmpegEncodeResult{
        FfmpegEncodeEvidence{
            output,
            output_bytes,
            static_cast<std::uint32_t>(frames.size()),
            true,
            true,
            true,
            false,
            false,
            !std::filesystem::exists(raw, error),
        },
        std::nullopt,
    };
}

}  // namespace act::platform::windows
