#include "components/recording_commit.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cwctype>
#include <string_view>
#include <vector>

namespace act::components {
namespace {

std::wstring commit_token() {
    static std::atomic<std::uint64_t> sequence{0U};
    const auto ticks = std::chrono::steady_clock::now()
                           .time_since_epoch()
                           .count();
    return std::to_wstring(ticks) + L"-" +
        std::to_wstring(sequence.fetch_add(
            1U, std::memory_order_relaxed));
}

struct AnalysisFile {
    std::filesystem::path staged;
    std::filesystem::path target;
    std::filesystem::path backup;
    bool target_existed;
    bool backup_created = false;
    bool committed = false;
};

RecordingCommitResult failure(
    const char* code,
    const std::string& message) {
    return RecordingCommitResult{
        std::nullopt, code, message};
}

bool real_directory(
    const std::filesystem::path& path) {
    std::error_code error;
    const auto status =
        std::filesystem::symlink_status(path, error);
    return !error &&
           !std::filesystem::is_symlink(status) &&
           std::filesystem::is_directory(status);
}

bool real_regular_file(
    const std::filesystem::path& path) {
    std::error_code error;
    const auto status =
        std::filesystem::symlink_status(path, error);
    return !error &&
           !std::filesystem::is_symlink(status) &&
           std::filesystem::is_regular_file(status);
}

bool owned_stage_name(
    const std::filesystem::path& path) {
    const auto name = path.filename().wstring();
    return name.starts_with(L".act-recording-stage-");
}

bool mp4_extension(
    const std::filesystem::path& path) {
    auto extension = path.extension().wstring();
    std::transform(
        extension.begin(),
        extension.end(),
        extension.begin(),
        [](const wchar_t value) {
            return static_cast<wchar_t>(
                std::towlower(value));
        });
    return extension == L".mp4";
}

bool owned_analysis_name(
    const std::filesystem::path& path,
    bool& keyframe) {
    const auto name = path.filename().wstring();
    keyframe =
        name.starts_with(L"frame-") &&
        name.ends_with(L".png");
    return keyframe ||
           name == L"storyboard.png" ||
           name == L"manifest.json";
}

bool same_parent(
    const std::filesystem::path& left,
    const std::filesystem::path& right) {
    std::error_code error;
    const auto left_parent = std::filesystem::weakly_canonical(
        left.parent_path(), error);
    if (error) {
        return false;
    }
    const auto right_parent = std::filesystem::weakly_canonical(
        right.parent_path(), error);
    return !error && left_parent == right_parent;
}

bool move_path(
    const std::filesystem::path& from,
    const std::filesystem::path& to) {
    std::error_code error;
    std::filesystem::rename(from, to, error);
    return !error;
}

bool rollback(
    std::vector<AnalysisFile>& files,
    const RecordingCommitPlan& plan,
    const std::filesystem::path& video_backup,
    const bool video_backed_up,
    const bool video_committed,
    const bool analysis_created) {
    bool ok = true;
    for (auto iterator = files.rbegin();
         iterator != files.rend();
         ++iterator) {
        if (iterator->committed) {
            if (!move_path(
                    iterator->target,
                    iterator->staged)) {
                ok = false;
            }
        }
        if (iterator->backup_created) {
            if (!move_path(
                    iterator->backup,
                    iterator->target)) {
                ok = false;
            }
        }
    }
    if (video_committed &&
        !move_path(
            plan.output_video,
            plan.staged_video)) {
        ok = false;
    }
    if (video_backed_up &&
        !move_path(video_backup, plan.output_video)) {
        ok = false;
    }
    if (analysis_created) {
        std::error_code error;
        if (std::filesystem::is_empty(
                plan.output_analysis, error) &&
            !error) {
            std::filesystem::remove(
                plan.output_analysis, error);
            if (error) {
                ok = false;
            }
        }
    }
    return ok;
}

}  // namespace

RecordingCommitResult commit_recording_artifacts(
    const RecordingCommitPlan& plan) {
    if (!owned_stage_name(plan.staged_video) ||
        !owned_stage_name(plan.staged_analysis) ||
        !mp4_extension(plan.output_video) ||
        !same_parent(
            plan.staged_video, plan.output_video) ||
        !same_parent(
            plan.staged_analysis, plan.output_analysis) ||
        !real_regular_file(plan.staged_video) ||
        !real_directory(plan.staged_analysis) ||
        !real_directory(plan.output_video.parent_path()) ||
        !real_directory(plan.output_analysis.parent_path())) {
        return failure(
            "INVALID_ARGUMENT",
            "Recording commit paths violate the owned same-parent "
            "staging policy.");
    }
    std::error_code error;
    const auto video_bytes =
        std::filesystem::file_size(
            plan.staged_video, error);
    if (error ||
        video_bytes < 12U ||
        video_bytes > 512ULL * 1024ULL * 1024ULL) {
        return failure(
            "VIDEO_OUTPUT_INVALID",
            "The staged recording video violates size bounds.");
    }

    const bool output_exists =
        std::filesystem::exists(
            plan.output_video, error) &&
        !error;
    if (output_exists &&
        (!plan.overwrite ||
         !real_regular_file(plan.output_video))) {
        return failure(
            plan.overwrite
                ? "INVALID_ARGUMENT"
                : "OVERWRITE_CONFIRMATION_REQUIRED",
            "The recording output cannot be replaced safely.");
    }
    const bool analysis_exists =
        std::filesystem::exists(
            plan.output_analysis, error) &&
        !error;
    if (analysis_exists &&
        !real_directory(plan.output_analysis)) {
        return failure(
            "INVALID_ARGUMENT",
            "The recording analysis target is not a real directory.");
    }

    const std::wstring token = commit_token();
    std::vector<AnalysisFile> files;
    std::uint32_t keyframe_count = 0U;
    bool storyboard_found = false;
    bool manifest_found = false;
    for (const auto& entry :
         std::filesystem::directory_iterator(
             plan.staged_analysis, error)) {
        if (error ||
            !entry.is_regular_file(error) ||
            error) {
            return failure(
                "INVALID_ARGUMENT",
                "The staged analysis contains a non-file entry.");
        }
        bool keyframe = false;
        if (!owned_analysis_name(entry.path(), keyframe)) {
            return failure(
                "INVALID_ARGUMENT",
                "The staged analysis contains an unowned filename.");
        }
        if (keyframe) {
            ++keyframe_count;
        } else if (
            entry.path().filename() ==
            L"storyboard.png") {
            storyboard_found = true;
        } else {
            manifest_found = true;
        }
        const auto target =
            plan.output_analysis /
            entry.path().filename();
        const bool target_exists =
            std::filesystem::exists(target, error) &&
            !error;
        if (target_exists &&
            (!plan.overwrite ||
             !real_regular_file(target))) {
            return failure(
                plan.overwrite
                    ? "INVALID_ARGUMENT"
                    : "OVERWRITE_CONFIRMATION_REQUIRED",
                "An analysis output cannot be replaced safely.");
        }
        const auto backup =
            plan.output_analysis /
            (L".act-recording-backup-" + token + L"-" +
             std::to_wstring(files.size()));
        if (std::filesystem::exists(backup, error)) {
            return failure(
                "OUTPUT_COLLISION",
                "A recording rollback path already exists.");
        }
        files.push_back(AnalysisFile{
            entry.path(),
            target,
            backup,
            target_exists,
        });
    }
    if (error ||
        files.size() < 3U ||
        files.size() > 22U ||
        keyframe_count == 0U ||
        keyframe_count > 20U ||
        !storyboard_found ||
        !manifest_found) {
        return failure(
            "VIDEO_ANALYSIS_FAILED",
            "The staged analysis artifact set is incomplete.");
    }
    const auto video_backup =
        plan.output_video.parent_path() /
        (L".act-recording-backup-" + token + L"-video");
    if (std::filesystem::exists(video_backup, error)) {
        return failure(
            "OUTPUT_COLLISION",
            "The recording video rollback path already exists.");
    }

    bool analysis_created = false;
    if (!analysis_exists) {
        if (!std::filesystem::create_directory(
                plan.output_analysis, error) ||
            error) {
            return failure(
                "VIDEO_WRITE_FAILED",
                "The recording analysis directory could not be created.");
        }
        analysis_created = true;
    }
    bool video_backed_up = false;
    bool video_committed = false;
    if (output_exists) {
        video_backed_up = move_path(
            plan.output_video, video_backup);
        if (!video_backed_up) {
            rollback(
                files,
                plan,
                video_backup,
                false,
                false,
                analysis_created);
            return failure(
                "VIDEO_WRITE_FAILED",
                "The existing video could not enter rollback staging.");
        }
    }
    video_committed = move_path(
        plan.staged_video, plan.output_video);
    if (!video_committed) {
        const bool restored = rollback(
            files,
            plan,
            video_backup,
            video_backed_up,
            false,
            analysis_created);
        return failure(
            restored
                ? "VIDEO_WRITE_FAILED"
                : "VIDEO_COMMIT_OUTCOME_UNKNOWN",
            "The staged video could not be committed.");
    }
    std::uint32_t replaced_analysis = 0U;
    for (auto& file : files) {
        if (file.target_existed) {
            file.backup_created = move_path(
                file.target, file.backup);
            if (!file.backup_created) {
                const bool restored = rollback(
                    files,
                    plan,
                    video_backup,
                    video_backed_up,
                    video_committed,
                    analysis_created);
                return failure(
                    restored
                        ? "VIDEO_WRITE_FAILED"
                        : "VIDEO_COMMIT_OUTCOME_UNKNOWN",
                    "An analysis file could not enter rollback staging.");
            }
            ++replaced_analysis;
        }
        file.committed =
            move_path(file.staged, file.target);
        if (!file.committed) {
            const bool restored = rollback(
                files,
                plan,
                video_backup,
                video_backed_up,
                video_committed,
                analysis_created);
            return failure(
                restored
                    ? "VIDEO_WRITE_FAILED"
                    : "VIDEO_COMMIT_OUTCOME_UNKNOWN",
                "A staged analysis file could not be committed.");
        }
    }

    bool cleanup_ok = true;
    if (video_backed_up) {
        std::filesystem::remove(video_backup, error);
        cleanup_ok = cleanup_ok && !error;
    }
    for (const auto& file : files) {
        if (file.backup_created) {
            error.clear();
            std::filesystem::remove(file.backup, error);
            cleanup_ok = cleanup_ok && !error;
        }
    }
    error.clear();
    std::filesystem::remove(plan.staged_analysis, error);
    cleanup_ok = cleanup_ok && !error;
    if (!cleanup_ok) {
        return failure(
            "VIDEO_COMMIT_CLEANUP_FAILED",
            "Recording outputs committed, but rollback staging cleanup "
            "was incomplete.");
    }
    return RecordingCommitResult{
        RecordingCommitEvidence{
            video_bytes,
            keyframe_count,
            output_exists,
            replaced_analysis,
            true,
            true,
            true,
        },
        {},
        {},
    };
}

}  // namespace act::components
