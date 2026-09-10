#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>

namespace act::platform::windows {

struct TextArtifactResult {
    bool ok;
    std::string error_code;
    std::string error_message;
    std::filesystem::path path;
    std::string verified_text;
};

struct ApplicationLaunchResult {
    bool ok;
    std::string error_code;
    std::string error_message;
    std::uint32_t process_id;
    bool foreground_unchanged;
};

class TextDocumentBackend final {
public:
    [[nodiscard]] bool runtime_available() const;
    [[nodiscard]] bool existing_notepad_process() const;
    [[nodiscard]] TextArtifactResult create_temporary_artifact(
        std::string_view text) const;
    [[nodiscard]] ApplicationLaunchResult open_in_notepad(
        const std::filesystem::path& path) const;
    void remove_owned_artifact(
        const std::filesystem::path& path) const;
};

}  // namespace act::platform::windows
