#include "components/json.hpp"
#include "modules/recording_module.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

namespace {

std::string utf8_path(
    const std::filesystem::path& path) {
    const auto value = path.u8string();
    return std::string(
        reinterpret_cast<const char*>(value.data()),
        value.size());
}

act::components::Json input_for(
    const std::filesystem::path& output,
    const std::filesystem::path& analysis,
    const bool overwrite) {
    return act::components::object({
        {"path", utf8_path(output)},
        {"analysisDir", utf8_path(analysis)},
        {"durationMs", 11000},
        {"fps", 2},
        {"maxWidth", 960},
        {"crf", 32},
        {"maxKeyframes", 8},
        {"changeThreshold", 0.035},
        {"timeoutMs", 5000},
        {"overwrite", overwrite},
    });
}

bool manifest_matches(
    const std::filesystem::path& path,
    const std::filesystem::path& output,
    const std::filesystem::path& analysis) {
    std::ifstream stream(path, std::ios::binary);
    std::ostringstream text;
    text << stream.rdbuf();
    std::string parse_error;
    const auto manifest =
        act::components::Json::parse(
            text.str(), parse_error);
    const auto* video = manifest.has_value()
        ? manifest->find("video")
        : nullptr;
    const auto* analysis_value = manifest.has_value()
        ? manifest->find("analysis")
        : nullptr;
    const auto* output_value =
        video == nullptr ? nullptr : video->find("path");
    const auto* duration =
        video == nullptr ? nullptr : video->find("durationMs");
    const auto* encoded =
        video == nullptr ? nullptr : video->find("encodedFrames");
    const auto* captured =
        video == nullptr ? nullptr : video->find("capturedFrames");
    const auto* source_width =
        video == nullptr ? nullptr : video->find("sourceWidth");
    const auto* width =
        video == nullptr ? nullptr : video->find("width");
    const auto* selected = analysis_value == nullptr
        ? nullptr
        : analysis_value->find("selectedKeyframes");
    const auto* storyboard = analysis_value == nullptr
        ? nullptr
        : analysis_value->find("storyboardPath");
    if (output_value == nullptr ||
        output_value->string_value() == nullptr ||
        *output_value->string_value() != utf8_path(output) ||
        duration == nullptr ||
        duration->integer_value() == nullptr ||
        *duration->integer_value() != 11000 ||
        encoded == nullptr ||
        encoded->integer_value() == nullptr ||
        *encoded->integer_value() != 22 ||
        captured == nullptr ||
        captured->integer_value() == nullptr ||
        *captured->integer_value() != 1 ||
        source_width == nullptr ||
        source_width->integer_value() == nullptr ||
        *source_width->integer_value() <= 0 ||
        width == nullptr ||
        width->integer_value() == nullptr ||
        (*width->integer_value() & 1) != 0 ||
        selected == nullptr ||
        selected->array_items() == nullptr ||
        selected->array_items()->empty() ||
        storyboard == nullptr ||
        storyboard->string_value() == nullptr ||
        *storyboard->string_value() !=
            utf8_path(analysis / L"storyboard.png")) {
        return false;
    }
    for (const auto& keyframe :
         *selected->array_items()) {
        const auto* value = keyframe.find("path");
        if (value == nullptr ||
            value->string_value() == nullptr) {
            return false;
        }
        const std::filesystem::path keyframe_path(
            std::u8string(
                reinterpret_cast<const char8_t*>(
                    value->string_value()->data()),
                value->string_value()->size()));
        const auto name =
            keyframe_path.filename().wstring();
        if (keyframe_path.parent_path() != analysis ||
            !name.starts_with(L"frame-") ||
            !name.ends_with(L"ms.png") ||
            !std::filesystem::is_regular_file(
                keyframe_path)) {
            return false;
        }
    }
    return true;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc != 3) {
        std::cerr << "Session and test root are required.\n";
        return 2;
    }
    const std::string root_value(argv[2]);
    const std::u8string root_utf8(
        reinterpret_cast<const char8_t*>(
            root_value.data()),
        root_value.size());
    const std::filesystem::path root(root_utf8);
    std::error_code error;
    std::filesystem::remove_all(root, error);
    error.clear();
    std::filesystem::create_directory(root, error);
    if (error) {
        return 2;
    }
    const auto output = root / L"module-evidence.mp4";
    const auto analysis = root / L"module-evidence.analysis";
    const act::modules::RecordingModule module;
    const auto input = input_for(output, analysis, false);

    const auto denied =
        module.record(argv[1], input, false);
    const bool denied_without_write =
        !denied.ok &&
        denied.error_code == "CONFIRMATION_REQUIRED" &&
        !std::filesystem::exists(output) &&
        !std::filesystem::exists(analysis);
    const auto stale = module.record(
        "s2:w:0000000000000000", input, true);
    const bool stale_refused =
        !stale.ok &&
        stale.error_code == "STALE_SESSION" &&
        !std::filesystem::exists(output) &&
        !std::filesystem::exists(analysis);
    const auto result =
        module.record(argv[1], input, true);
    const auto* capability =
        result.data.find("capability");
    const auto* atomic =
        result.data.find("atomicOutput");
    const auto* foreground =
        result.data.find("foregroundUnchanged");
    const auto* encoded_frames =
        result.data.find("encodedFrames");
    const bool committed =
        result.ok &&
        capability != nullptr &&
        capability->string_value() != nullptr &&
        *capability->string_value() == "window.record@1" &&
        atomic != nullptr &&
        atomic->bool_value() != nullptr &&
        *atomic->bool_value() &&
        foreground != nullptr &&
        foreground->bool_value() != nullptr &&
        *foreground->bool_value() &&
        encoded_frames != nullptr &&
        encoded_frames->integer_value() != nullptr &&
        *encoded_frames->integer_value() == 22 &&
        std::filesystem::is_regular_file(output) &&
        std::filesystem::is_regular_file(
            analysis / L"storyboard.png") &&
        std::filesystem::is_regular_file(
            analysis / L"manifest.json");
    const bool manifest_aligned =
        committed &&
        manifest_matches(
            analysis / L"manifest.json",
            output,
            analysis);
    const auto overwrite_denied =
        module.record(argv[1], input, true);
    const bool overwrite_guarded =
        !overwrite_denied.ok &&
        overwrite_denied.error_code ==
            "OVERWRITE_CONFIRMATION_REQUIRED";
    std::filesystem::remove_all(root, error);
    if (!denied_without_write ||
        !stale_refused ||
        !committed ||
        !manifest_aligned ||
        !overwrite_guarded ||
        error) {
        std::cerr << "Recording module candidate failed: "
                  << result.error_code << " "
                  << result.error_message << '\n';
        return 1;
    }
    std::cout << act::components::object({
        {"ok", true},
        {"confirmationBeforeWrite", true},
        {"staleTargetRefused", true},
        {"exactOpaqueTarget", true},
        {"atomicCommit", true},
        {"streamedFrames", 22},
        {"rustManifestShape", true},
        {"overwriteGuarded", true},
        {"foregroundUnchanged", true},
        {"publicRouteEnabled", false},
    }).dump() << '\n';
    return 0;
}
