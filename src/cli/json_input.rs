//! 把 CLI JSON 来源语法和 Component 错误映射到公开控制错误。

// 导入文件路径类型。
use std::path::Path;

// 导入 JSON 值与证据构造宏。
use serde_json::{Value, json};

// 导入有界读取 Component 和公开结果类型。
use crate::{
    // 只依赖 Component 的窄函数与封闭错误。
    components::bounded_json_input::{self, BoundedJsonInputError},
    // 使用 CLI 已有稳定结果 envelope。
    domain::AppResult,
};

// 导入父 CLI 边界拥有的封闭错误类型。
use super::error_code::CliErrorCode;

// 读取一个 CLI `--input` 来源并映射公开错误。
pub(super) fn read_json(source: &str) -> AppResult<Value> {
    // 兼容可选的单个 @ 文件来源前缀。
    let normalized = source.strip_prefix('@').unwrap_or(source);
    // CLI 边界决定单横线表示 stdin，其他值表示路径。
    let result = if normalized == "-" {
        // 标准输入仍由同一有界 Component 读取。
        bounded_json_input::read_stdin()
    } else {
        // 路径只在 CLI 边界转换为标准库类型。
        bounded_json_input::read_file(Path::new(normalized))
    };
    // 把 Component 私有错误映射为稳定公开错误。
    result.map_err(|error| match error {
        // I/O 失败保持现有稳定错误码。
        BoundedJsonInputError::Read(error) => {
            // 不隐藏操作系统提供的文件读取诊断。
            CliErrorCode::InputReadFailed.error(error.to_string())
        }
        // 超限输入映射为带边界证据的参数错误。
        BoundedJsonInputError::TooLarge {
            // 读取已经观察到的字节数。
            observed_bytes,
            // 读取统一最大字节数。
            maximum_bytes,
        } => {
            // 把有界读取证据映射为带安全详情的参数错误。
            CliErrorCode::InvalidArgument.with_details(
                // 返回不依赖来源类型的稳定消息。
                "JSON 输入超过 16 MiB 硬上限。",
                // 返回机器可判定的观察量和上限。
                json!({ "observedBytes": observed_bytes, "maximumBytes": maximum_bytes }),
            )
        }
        // UTF-8 与 JSON 语法错误保持参数错误语义。
        BoundedJsonInputError::Invalid(message) => {
            // 返回 Component 已经净化的诊断文本。
            CliErrorCode::InvalidArgument.error(message)
        }
    })
}
