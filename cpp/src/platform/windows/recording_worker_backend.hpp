#pragma once

#include "components/json.hpp"
#include "components/recording_config.hpp"
#include "platform/windows/discovery_backend.hpp"

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>

namespace act::platform::windows {

struct RecordingWorkerRun {
    std::optional<components::Json> data;
    std::optional<BackendError> error;
    bool job_terminated;
    bool staging_cleaned;
    std::optional<std::filesystem::path> staged_video =
        std::nullopt;
    std::optional<std::filesystem::path> staged_analysis =
        std::nullopt;
};

class RecordingWorkerBackend final {
public:
    [[nodiscard]] RecordingWorkerRun run_fixture_encode(
        std::uint32_t timeout_ms) const;
    [[nodiscard]] RecordingWorkerRun run_timeout_fixture(
        std::uint32_t timeout_ms) const;
    [[nodiscard]] RecordingWorkerRun run_exact_window_candidate(
        const std::string& session_id,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] RecordingWorkerRun run_exact_window_stage(
        const std::string& session_id,
        const std::filesystem::path& staging_root,
        const components::RecordingConfig& config,
        std::uint32_t timeout_ms) const;
    [[nodiscard]] bool cleanup_external_stage(
        const std::filesystem::path& staged_video,
        const std::filesystem::path& staged_analysis) const;
};

}  // namespace act::platform::windows
