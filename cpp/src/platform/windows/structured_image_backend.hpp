#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace act::platform::windows {

// 从 provider 私有进程事实生成 canonical s2:a。
[[nodiscard]] std::string structured_image_application_session_id(
    // 接收当前 Photoshop PID。
    std::uint32_t process_id,
    // 接收同一进程实例的创建 FILETIME。
    std::uint64_t creation_time);

// 从 provider 私有进程与文档事实生成 canonical s2:d。
[[nodiscard]] std::string structured_image_document_session_id(
    // 接收当前 Photoshop PID。
    std::uint32_t process_id,
    // 接收同一进程实例的创建 FILETIME。
    std::uint64_t creation_time,
    // 接收 provider 原生文档 ID。
    std::int64_t native_document_id,
    // 接收当前文档名称。
    const std::string& name,
    // 接收当前源路径或空字符串哨兵。
    const std::string& source_path);

struct StructuredImageDocumentRecord {
    std::string session_id;
    std::string name;
    std::optional<std::string> source_path;
    double width_px;
    double height_px;
    double resolution_dpi;
    std::int64_t layer_count;
    bool saved;
    bool active;

    // Private provider identities; never serialize these fields.
    std::uint32_t native_process_id;
    std::int64_t native_document_id;
};

struct StructuredImageStatus {
    bool installed;
    bool connected;
    std::optional<std::string> version;
    std::optional<std::string> error_code;
    std::optional<std::string> error_message;
};

struct StructuredImageInventory {
    std::vector<StructuredImageDocumentRecord> documents;
    std::optional<std::string> application_session_id;
    std::optional<std::string> error_code;
    std::optional<std::string> error_message;
};

struct StructuredImageWriteEvidence {
    bool dispatched;
    bool verified;
    bool outcome_unknown;
    std::optional<std::string> current_session_id;
    std::optional<std::string> error_code;
    std::optional<std::string> error_message;
};

class StructuredImageBackend final {
public:
    [[nodiscard]] StructuredImageStatus status() const;
    [[nodiscard]] StructuredImageInventory inventory() const;
    [[nodiscard]] StructuredImageWriteEvidence save(
        const StructuredImageDocumentRecord& document,
        const std::filesystem::path& path) const;
    [[nodiscard]] StructuredImageWriteEvidence export_png(
        const StructuredImageDocumentRecord& document,
        const std::filesystem::path& path) const;
};

}  // namespace act::platform::windows
