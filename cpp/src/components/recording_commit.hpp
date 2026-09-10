#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>

namespace act::components {

struct RecordingCommitPlan {
    std::filesystem::path staged_video;
    std::filesystem::path staged_analysis;
    std::filesystem::path output_video;
    std::filesystem::path output_analysis;
    bool overwrite;
};

struct RecordingCommitEvidence {
    std::uint64_t video_bytes;
    std::uint32_t keyframe_files;
    bool replaced_video;
    std::uint32_t replaced_analysis_files;
    bool preserved_unowned_analysis_files;
    bool staging_removed;
    bool rollback_available_during_commit;
};

struct RecordingCommitResult {
    std::optional<RecordingCommitEvidence> evidence;
    std::string error_code;
    std::string error_message;
};

[[nodiscard]] RecordingCommitResult
commit_recording_artifacts(
    const RecordingCommitPlan& plan);

}  // namespace act::components
