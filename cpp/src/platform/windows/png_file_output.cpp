#include "platform/windows/png_file_output.hpp"

#include "platform/windows/text_codec.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <limits>
#include <string_view>
#include <vector>

namespace act::platform::windows {
namespace {

constexpr std::size_t maximum_png_bytes =
    64U * 1024U * 1024U;
constexpr std::array<std::uint8_t, 8> png_signature{
    0x89U, 0x50U, 0x4eU, 0x47U,
    0x0dU, 0x0aU, 0x1aU, 0x0aU,
};

PngFileOutputResult failure(
    const char* code,
    const char* message) {
    return PngFileOutputResult{
        std::nullopt,
        code,
        message,
    };
}

PngOutputPlanResult plan_failure(
    const char* code,
    const char* message) {
    return PngOutputPlanResult{
        std::nullopt,
        code,
        message,
    };
}

class Handle final {
public:
    explicit Handle(HANDLE value) : value_(value) {}
    ~Handle() {
        close();
    }
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    Handle(Handle&& other) noexcept
        : value_(other.release()) {}
    Handle& operator=(Handle&& other) noexcept {
        if (this != &other) {
            close();
            value_ = other.release();
        }
        return *this;
    }

    [[nodiscard]] bool valid() const {
        return value_ != INVALID_HANDLE_VALUE;
    }
    [[nodiscard]] HANDLE get() const {
        return value_;
    }
    bool close() {
        if (!valid()) {
            return true;
        }
        const bool closed = CloseHandle(value_) != FALSE;
        value_ = INVALID_HANDLE_VALUE;
        return closed;
    }
    [[nodiscard]] HANDLE release() {
        const HANDLE value = value_;
        value_ = INVALID_HANDLE_VALUE;
        return value;
    }

private:
    HANDLE value_ = INVALID_HANDLE_VALUE;
};

std::optional<std::wstring> normalized_path(
    const std::string& path) {
    const std::wstring input = wide(path);
    if (input.empty()) {
        return std::nullopt;
    }
    const DWORD required =
        GetFullPathNameW(input.c_str(), 0U, nullptr, nullptr);
    if (required == 0U || required > 32768U) {
        return std::nullopt;
    }
    std::vector<wchar_t> buffer(required);
    const DWORD written = GetFullPathNameW(
        input.c_str(),
        static_cast<DWORD>(buffer.size()),
        buffer.data(),
        nullptr);
    if (written == 0U || written >= buffer.size()) {
        return std::nullopt;
    }
    return std::wstring(buffer.data(), written);
}

bool is_directory(const std::wstring& path) {
    const DWORD attributes = GetFileAttributesW(path.c_str());
    return attributes != INVALID_FILE_ATTRIBUTES &&
           (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0U;
}

std::wstring temporary_path(
    const std::wstring& parent,
    const std::wstring& filename,
    const std::uint32_t attempt) {
    return parent + L"\\." + filename + L"." +
           std::to_wstring(GetCurrentProcessId()) + L"." +
           std::to_wstring(GetTickCount64()) + L"." +
           std::to_wstring(attempt) + L".part.png";
}

}  // namespace

PngOutputPlanResult validate_png_output_path(
    const std::string& output_path,
    const bool overwrite) {
    if (output_path.size() < 5U ||
        !output_path.ends_with(".png") ||
        output_path.find_last_of("\\/") == std::string::npos) {
        return plan_failure(
            "INVALID_ARGUMENT",
            "PNG output requires a path with an existing parent and .png "
            "extension.");
    }
    const auto normalized = normalized_path(output_path);
    if (!normalized.has_value()) {
        return plan_failure(
            "INVALID_ARGUMENT",
            "PNG output path is not valid UTF-8 or cannot be normalized.");
    }
    const std::size_t separator = normalized->find_last_of(L"\\/");
    if (separator == std::wstring::npos ||
        separator + 1U >= normalized->size()) {
        return plan_failure(
            "INVALID_ARGUMENT",
            "PNG output path must include a filename.");
    }
    const std::wstring parent = normalized->substr(0U, separator);
    if (!is_directory(parent)) {
        return plan_failure(
            "INVALID_ARGUMENT",
            "PNG output parent directory does not exist.");
    }

    const DWORD existing_attributes =
        GetFileAttributesW(normalized->c_str());
    const bool existed =
        existing_attributes != INVALID_FILE_ATTRIBUTES;
    if (existed &&
        (existing_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0U) {
        return plan_failure(
            "INVALID_ARGUMENT",
            "PNG output path resolves to a directory.");
    }
    if (existed && !overwrite) {
        return plan_failure(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "PNG output exists and overwrite confirmation is absent.");
    }
    return PngOutputPlanResult{
        PngOutputPlan{
            utf8(*normalized),
            existed,
        },
        {},
        {},
    };
}

PngFileOutputResult write_png_atomically(
    const std::string& output_path,
    const std::vector<std::uint8_t>& png,
    const bool overwrite) {
    const auto plan =
        validate_png_output_path(output_path, overwrite);
    if (!plan.plan.has_value()) {
        return PngFileOutputResult{
            std::nullopt,
            plan.error_code,
            plan.error_message,
        };
    }
    if (png.size() < png_signature.size() ||
        png.size() > maximum_png_bytes ||
        !std::equal(
            png_signature.begin(),
            png_signature.end(),
            png.begin())) {
        return failure(
            "INVALID_ARGUMENT",
            "PNG output bytes violate the signature or 64 MiB limit.");
    }
    const auto normalized =
        normalized_path(plan.plan->normalized_path);
    if (!normalized.has_value()) {
        return failure(
            "INVALID_ARGUMENT",
            "Normalized PNG output path could not be decoded.");
    }
    const std::size_t separator = normalized->find_last_of(L"\\/");
    const std::wstring parent = normalized->substr(0U, separator);
    const std::wstring filename = normalized->substr(separator + 1U);
    const bool existed = plan.plan->target_exists;

    std::wstring temporary;
    Handle file(INVALID_HANDLE_VALUE);
    for (std::uint32_t attempt = 0U;
         attempt < 16U;
         ++attempt) {
        temporary = temporary_path(parent, filename, attempt);
        file = Handle(CreateFileW(
            temporary.c_str(),
            GENERIC_WRITE,
            0U,
            nullptr,
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY,
            nullptr));
        if (file.valid()) {
            break;
        }
        if (GetLastError() != ERROR_FILE_EXISTS &&
            GetLastError() != ERROR_ALREADY_EXISTS) {
            return failure(
                "SCREENSHOT_WRITE_FAILED",
                "PNG temporary file could not be created.");
        }
    }
    if (!file.valid()) {
        return failure(
            "SCREENSHOT_WRITE_FAILED",
            "PNG temporary filename attempts were exhausted.");
    }

    bool write_ok = true;
    std::size_t offset = 0U;
    while (offset < png.size()) {
        const std::size_t remaining = png.size() - offset;
        const DWORD chunk = static_cast<DWORD>(
            std::min<std::size_t>(
                remaining,
                std::numeric_limits<DWORD>::max()));
        DWORD written = 0U;
        if (WriteFile(
                file.get(),
                png.data() + offset,
                chunk,
                &written,
                nullptr) == FALSE ||
            written == 0U) {
            write_ok = false;
            break;
        }
        offset += written;
    }
    if (write_ok) {
        write_ok = FlushFileBuffers(file.get()) != FALSE;
    }
    write_ok = file.close() && write_ok;
    if (!write_ok) {
        DeleteFileW(temporary.c_str());
        return failure(
            "SCREENSHOT_WRITE_FAILED",
            "PNG temporary file could not be flushed and closed.");
    }

    const DWORD move_flags =
        MOVEFILE_WRITE_THROUGH |
        (overwrite ? MOVEFILE_REPLACE_EXISTING : 0U);
    if (MoveFileExW(
            temporary.c_str(),
            normalized->c_str(),
            move_flags) == FALSE) {
        const DWORD error = GetLastError();
        DeleteFileW(temporary.c_str());
        if (!overwrite &&
            (error == ERROR_FILE_EXISTS ||
             error == ERROR_ALREADY_EXISTS)) {
            return failure(
                "OVERWRITE_CONFIRMATION_REQUIRED",
                "PNG output appeared before atomic commit.");
        }
        return failure(
            "SCREENSHOT_WRITE_FAILED",
            "PNG temporary file could not be atomically committed.");
    }

    return PngFileOutputResult{
        PngFileOutput{
            utf8(*normalized),
            static_cast<std::int64_t>(png.size()),
            existed,
        },
        {},
        {},
    };
}

}  // namespace act::platform::windows
