#include "components/json.hpp"
#include "components/recording_config.hpp"

#include <windows.h>

#include <filesystem>
#include <fstream>
#include <iostream>

namespace {

bool expect(
    const bool condition,
    const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        return false;
    }
    return true;
}

std::optional<act::components::Json> parse(
    const std::string& text) {
    std::string error;
    return act::components::Json::parse(text, error);
}

}  // namespace

int main() {
    bool ok = true;
    std::error_code error;
    const auto directory =
        std::filesystem::temp_directory_path() /
        (L"act-recording-config-" +
         std::to_wstring(GetCurrentProcessId()));
    std::filesystem::create_directory(directory, error);
    const auto output = directory / L"evidence.mp4";
    const auto analysis = directory / L"custom.analysis";
    const std::string output_json = output.generic_string();
    const std::string analysis_json =
        analysis.generic_string();
    const std::string request =
        "{\"path\":\"" + output_json +
        "\",\"changeThreshold\":0.035}";
    const auto input = parse(request);
    const auto result =
        act::components::parse_recording_config(*input);
    ok = expect(
             result.config.has_value() &&
                 result.config->duration_ms == 30000U &&
                 result.config->fps == 2U &&
                 result.config->maximum_width == 960U &&
                 result.config->crf == 32U &&
                 result.config->maximum_keyframes == 8U &&
                 result.config->frame_timeout_ms == 5000U &&
                 result.config->frame_budget == 60U &&
                 result.config->change_threshold == 0.035,
             "Recording defaults or decimal input are incompatible.") &&
         ok;

    const auto dimensions =
        act::components::recording_output_dimensions(
            1920U, 1080U, 960U);
    const auto odd_dimensions =
        act::components::recording_output_dimensions(
            801U, 601U, 960U);
    ok = expect(
             dimensions == std::pair{960U, 540U} &&
                 odd_dimensions ==
                     std::pair{800U, 600U},
             "Recording dimensions differ from the Rust baseline.") &&
         ok;

    for (const std::string& invalid : {
             "{\"path\":\"" + output_json +
                 "\",\"fps\":11}",
             "{\"path\":\"" + output_json +
                 "\",\"durationMs\":999}",
             "{\"path\":\"" + output_json +
                 "\",\"maxWidth\":1930}",
             "{\"path\":\"" + output_json +
                 "\",\"crf\":41}",
             "{\"path\":\"" + output_json +
                 "\",\"maxKeyframes\":21}",
             "{\"path\":\"" + output_json +
                 "\",\"changeThreshold\":0.004}",
             "{\"path\":\"" + output_json +
                 "\",\"timeoutMs\":249}",
             "{\"path\":\"" + output_json +
                 "\",\"overwrite\":\"yes\"}",
             "{\"path\":\"" + output_json +
                 "\",\"ffmpegArgs\":\"-arbitrary\"}",
         }) {
        const auto value = parse(invalid);
        const auto rejected =
            act::components::parse_recording_config(*value);
        ok = expect(
                 !rejected.config.has_value() &&
                     rejected.error_code ==
                         "INVALID_ARGUMENT",
                 "Unbounded recording input was accepted.") &&
             ok;
    }

    {
        std::ofstream file(output, std::ios::binary);
        file << "owned";
    }
    const auto existing = parse(
        "{\"path\":\"" + output_json + "\"}");
    const auto refused =
        act::components::parse_recording_config(*existing);
    ok = expect(
             !refused.config.has_value() &&
                 refused.error_code ==
                     "OVERWRITE_CONFIRMATION_REQUIRED",
             "Existing MP4 did not require overwrite consent.") &&
         ok;
    std::filesystem::remove(output, error);

    std::filesystem::create_directory(analysis, error);
    {
        std::ofstream file(
            analysis / L"user-owned.txt",
            std::ios::binary);
        file << "preserve";
    }
    const auto nonempty = parse(
        "{\"path\":\"" + output_json +
        "\",\"analysisDir\":\"" + analysis_json +
        "\"}");
    const auto nonempty_refused =
        act::components::parse_recording_config(*nonempty);
    ok = expect(
             !nonempty_refused.config.has_value() &&
                 nonempty_refused.error_code ==
                     "OVERWRITE_CONFIRMATION_REQUIRED",
             "Non-empty analysis directory was accepted silently.") &&
         ok;
    const auto confirmed = parse(
        "{\"path\":\"" + output_json +
        "\",\"analysisDir\":\"" + analysis_json +
        "\",\"overwrite\":true}");
    ok = expect(
             act::components::parse_recording_config(
                 *confirmed).config.has_value(),
             "Confirmed existing analysis directory was rejected.") &&
         ok;
    ok = expect(
             std::filesystem::exists(
                 analysis / L"user-owned.txt"),
             "Config validation modified an analysis directory.") &&
         ok;
    std::filesystem::remove(
        analysis / L"user-owned.txt", error);
    std::filesystem::remove(analysis, error);
    std::filesystem::remove(directory, error);
    return ok ? 0 : 1;
}
