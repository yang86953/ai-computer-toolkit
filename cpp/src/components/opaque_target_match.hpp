// 防止头文件被重复包含。
#pragma once

// 将唯一匹配组件限制在公共组件命名空间内。
namespace act::components {

// 描述 opaque 目标在当前清单中的唯一匹配状态。
enum class OpaqueTargetMatchState {
    // 表示当前清单中没有匹配目标。
    missing,
    // 表示当前清单中只有一个匹配目标。
    unique,
    // 表示当前清单中存在多个匹配目标。
    ambiguous,
// 结束唯一匹配状态定义。
};

// 保存唯一匹配状态以及仅在唯一命中时有效的位置。
template <typename Iterator>
// 定义不复制领域记录的唯一匹配结果。
struct OpaqueTargetMatch {
    // 保存 missing、unique 或 ambiguous 状态。
    OpaqueTargetMatchState state;
    // 保存唯一命中的迭代器；其他状态下等于结束迭代器。
    Iterator position;
// 结束唯一匹配结果定义。
};

// 扫描整个清单并拒绝任取第一个匹配目标。
template <typename Iterator, typename Predicate>
// 返回零命中、唯一命中或多命中的稳定结果。
[[nodiscard]] OpaqueTargetMatch<Iterator> match_opaque_target(Iterator begin, Iterator end, Predicate predicate) {
    // 初始位置表示尚未找到目标。
    Iterator position = end;
    // 遍历完整清单以检测第二个匹配目标。
    for (Iterator current = begin; current != end; ++current) {
        // 跳过不满足 opaque 身份谓词的记录。
        if (!predicate(*current)) {
            // 继续检查后续记录。
            continue;
        }
        // 第二次命中必须关闭失败，不能返回任意记录。
        if (position != end) {
            // 多命中时不暴露任何候选位置。
            return OpaqueTargetMatch<Iterator>{OpaqueTargetMatchState::ambiguous, end};
        }
        // 记录首个候选并继续扫描碰撞。
        position = current;
    }
    // 根据是否存在候选返回 missing 或 unique。
    return OpaqueTargetMatch<Iterator>{position == end ? OpaqueTargetMatchState::missing : OpaqueTargetMatchState::unique, position};
// 结束完整清单扫描。
}

// 结束公共组件命名空间。
}  // namespace act::components
