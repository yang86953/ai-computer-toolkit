#include "components/output_path_policy.hpp"
#include "modules/structured_image_module.hpp"
// 导入可独立验证的 structured-image 纯身份生成器。
#include "platform/windows/structured_image_backend.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>

int main(int argc, char** argv) {
    if (argc != 2) {
        return 2;
    }
    const std::filesystem::path root(argv[1]);
    std::filesystem::create_directories(root);
    const std::filesystem::path psd = root / "candidate.psd";
    const std::filesystem::path png = root / "candidate.png";
    // 生成与 Rust 测试共用的应用 golden。
    const std::string application_session =
        // 传入稳定 PID 与创建 FILETIME 夹具。
        act::platform::windows::structured_image_application_session_id(
            // 传入夹具 PID。
            10U,
            // 传入夹具创建 FILETIME。
            123U);
    // 生成与 Rust 测试共用的文档 golden。
    const std::string document_session =
        // 传入稳定进程与文档事实夹具。
        act::platform::windows::structured_image_document_session_id(
            // 传入夹具 PID。
            10U,
            // 传入夹具创建 FILETIME。
            123U,
            // 传入夹具原生文档 ID。
            7,
            // 传入夹具文档名称。
            "poster.psd",
            // 传入夹具源路径。
            "C:\\a\\poster.psd");
    // 要求 C++ 与 Rust 共用逐字节相同的应用和文档 golden。
    if (application_session != "s2:a:df092f0662f4b7a5" ||
        // 拒绝文档身份字节布局的任何分歧。
        document_session != "s2:d:8566c36650cec9c1") {
        // 输出可定位的跨实现 golden 分歧错误。
        std::cerr << "Structured image identity golden diverged.\n";
        // 以失败状态结束候选门禁。
        return 1;
    // 结束跨实现身份 golden 检查。
    }
    {
        std::ofstream existing(psd);
        existing << "owned-fixture";
    }

    const act::modules::StructuredImageModule module;
    const auto unconfirmed =
        module.save({}, {}, false, false);
    const auto legacy =
        module.save("s1:c3:0000000000000000", psd, true, true);
    const auto stale =
        module.export_png(
            "s2:d:0000000000000000", png, false, true);
    if (unconfirmed.ok ||
        unconfirmed.error_code != "CONFIRMATION_REQUIRED" ||
        legacy.ok ||
        legacy.error_code !=
            "TARGET_ID_MIGRATION_REQUIRED" ||
        stale.ok ||
        (stale.error_code != "STALE_SESSION" &&
         stale.error_code != "APPLICATION_NOT_RUNNING")) {
        std::cerr << "Structured image policy ordering failed.\n";
        return 1;
    }

    const auto overwrite =
        act::components::validate_output_path(
            psd, "psd", false);
    const auto allowed =
        act::components::validate_output_path(
            psd, "psd", true);
    const auto wrong_extension =
        act::components::validate_output_path(
            png, "psd", false);
    const auto relative =
        act::components::validate_output_path(
            "candidate.psd", "psd", false);
    if (overwrite.ok ||
        overwrite.error_code !=
            "OVERWRITE_CONFIRMATION_REQUIRED" ||
        !allowed.ok ||
        wrong_extension.ok ||
        wrong_extension.error_code != "INVALID_ARGUMENT" ||
        relative.ok ||
        relative.error_code != "INVALID_ARGUMENT") {
        std::cerr << "Structured image output policy failed.\n";
        return 1;
    }

    const auto status = module.status();
    const auto sessions = module.sessions();
    if (!status.ok || !sessions.ok) {
        std::cerr << "Attach-only observation candidate failed.\n";
        return 1;
    }
    const std::string public_payload =
        status.data.dump() + sessions.data.dump();
    for (const char* forbidden : {
             "Photoshop.Application",
             "DoJavaScript",
             "native_document_id",
             "native_process_id",
         }) {
        if (public_payload.find(forbidden) !=
            std::string::npos) {
            std::cerr << "Native provider identity leaked.\n";
            return 1;
        }
    }
    // 禁止 sessions/status 公开 JSON 包含 provider 内部源路径字段。
    if (public_payload.find("\"path\"") != std::string::npos) {
        // 输出可定位的源文档路径泄漏错误。
        std::cerr << "Structured image source path leaked.\n";
        // 以失败状态结束候选门禁。
        return 1;
    // 结束公开源路径隐私检查。
    }
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    return 0;
}
