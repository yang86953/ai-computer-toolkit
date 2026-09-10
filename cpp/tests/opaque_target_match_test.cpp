// 引入待验证的唯一匹配组件。
#include "components/opaque_target_match.hpp"

// 提供测试清单容器。
#include <vector>
// 提供测试身份字符串。
#include <string>

// 将测试记录限制在当前翻译单元内。
namespace {

// 定义不含平台类型的最小测试记录。
struct Record {
    // 保存供谓词比较的 opaque 身份。
    std::string session_id;
// 结束最小测试记录定义。
};

// 结束当前翻译单元的私有定义。
}  // namespace

// 验证 Missing、Unique 与 Ambiguous 三态。
int main() {
    // 建立不包含目标的清单。
    const std::vector<Record> missing_records{{"other"}};
    // 完整扫描零命中清单。
    const auto missing = act::components::match_opaque_target(
        missing_records.begin(),
        missing_records.end(),
        [](const auto& record) {
            // 只接受固定测试目标。
            return record.session_id == "target";
        });
    // 零命中必须返回 Missing 且不暴露位置。
    if (missing.state != act::components::OpaqueTargetMatchState::missing ||
        missing.position != missing_records.end()) {
        // 用非零退出码报告零命中门禁失败。
        return 1;
    }

    // 建立只包含一个目标的清单。
    const std::vector<Record> unique_records{{"other"}, {"target"}};
    // 完整扫描唯一命中清单。
    const auto unique = act::components::match_opaque_target(
        unique_records.begin(),
        unique_records.end(),
        [](const auto& record) {
            // 只接受固定测试目标。
            return record.session_id == "target";
        });
    // 唯一命中必须返回可安全解引用的位置。
    if (unique.state != act::components::OpaqueTargetMatchState::unique ||
        unique.position == unique_records.end() ||
        unique.position->session_id != "target") {
        // 用非零退出码报告唯一命中门禁失败。
        return 2;
    }

    // 建立包含两个相同 opaque 身份的碰撞清单。
    const std::vector<Record> ambiguous_records{{"target"}, {"target"}};
    // 完整扫描多命中清单。
    const auto ambiguous = act::components::match_opaque_target(
        ambiguous_records.begin(),
        ambiguous_records.end(),
        [](const auto& record) {
            // 只接受固定测试目标。
            return record.session_id == "target";
        });
    // 多命中必须返回 Ambiguous 且不暴露任意候选。
    if (ambiguous.state !=
            act::components::OpaqueTargetMatchState::ambiguous ||
        ambiguous.position != ambiguous_records.end()) {
        // 用非零退出码报告碰撞门禁失败。
        return 3;
    }

    // 三态门禁全部满足时返回成功。
    return 0;
// 结束纯唯一匹配测试。
}
