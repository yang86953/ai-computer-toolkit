#include "components/recording_commit.hpp"

#include <windows.h>

#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>

namespace {

void write(
    const std::filesystem::path& path,
    const std::string& value) {
    std::ofstream output(path, std::ios::binary);
    output << value;
}

act::components::RecordingCommitPlan stage(
    const std::filesystem::path& root,
    const std::string& token,
    const std::string& video,
    const std::string& marker,
    const bool overwrite) {
    const auto staged_video =
        root /
        (L".act-recording-stage-" +
         std::wstring(token.begin(), token.end()) +
         L".mp4");
    const auto staged_analysis =
        root /
        (L".act-recording-stage-" +
         std::wstring(token.begin(), token.end()) +
         L".analysis");
    std::filesystem::create_directory(staged_analysis);
    write(staged_video, video);
    write(staged_analysis / L"frame-01.png", marker);
    write(staged_analysis / L"storyboard.png", marker);
    write(
        staged_analysis / L"manifest.json",
        "{\"schemaVersion\":1}");
    return act::components::RecordingCommitPlan{
        staged_video,
        staged_analysis,
        root / L"evidence.mp4",
        root / L"evidence.analysis",
        overwrite,
    };
}

std::string read(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary);
    return std::string(
        std::istreambuf_iterator<char>(input),
        std::istreambuf_iterator<char>());
}

}  // namespace

int main() {
    const auto root =
        std::filesystem::temp_directory_path() /
        (L"act-recording-commit-" +
         std::to_wstring(GetCurrentProcessId()));
    std::error_code error;
    std::filesystem::remove_all(root, error);
    std::filesystem::create_directory(root, error);
    bool ok = !error;

    const std::string video_one =
        "0000ftypisomfirst-video";
    auto first = stage(
        root, "first", video_one, "first", false);
    const auto first_result =
        act::components::commit_recording_artifacts(first);
    ok = ok &&
        first_result.evidence.has_value() &&
        !first_result.evidence->replaced_video &&
        first_result.evidence->keyframe_files == 1U &&
        read(first.output_video) == video_one &&
        read(first.output_analysis / L"frame-01.png") ==
            "first" &&
        !std::filesystem::exists(first.staged_video) &&
        !std::filesystem::exists(first.staged_analysis);

    write(
        first.output_analysis / L"user-owned.txt",
        "preserve");
    const std::string video_two =
        "0000ftypisomsecond-video";
    auto refused = stage(
        root, "refused", video_two, "second", false);
    const auto refused_result =
        act::components::commit_recording_artifacts(refused);
    ok = ok &&
        !refused_result.evidence.has_value() &&
        refused_result.error_code ==
            "OVERWRITE_CONFIRMATION_REQUIRED" &&
        read(first.output_video) == video_one &&
        read(first.output_analysis / L"frame-01.png") ==
            "first" &&
        read(first.output_analysis / L"user-owned.txt") ==
            "preserve";
    std::filesystem::remove(refused.staged_video, error);
    std::filesystem::remove_all(
        refused.staged_analysis, error);

    auto replacement = stage(
        root, "replacement", video_two, "second", true);
    const auto replacement_result =
        act::components::commit_recording_artifacts(
            replacement);
    ok = ok &&
        replacement_result.evidence.has_value() &&
        replacement_result.evidence->replaced_video &&
        replacement_result.evidence
                ->replaced_analysis_files ==
            3U &&
        replacement_result.evidence
            ->preserved_unowned_analysis_files &&
        read(first.output_video) == video_two &&
        read(first.output_analysis / L"frame-01.png") ==
            "second" &&
        read(first.output_analysis / L"user-owned.txt") ==
            "preserve" &&
        !std::filesystem::exists(
            replacement.staged_video) &&
        !std::filesystem::exists(
            replacement.staged_analysis);

    for (const auto& entry :
         std::filesystem::directory_iterator(root)) {
        ok = ok &&
            !entry.path().filename().wstring().starts_with(
                L".act-recording-backup-");
    }
    std::filesystem::remove_all(root, error);
    if (!ok || error) {
        std::cerr
            << "Recording commit did not preserve rollback policy.\n";
        return 1;
    }
    return 0;
}
