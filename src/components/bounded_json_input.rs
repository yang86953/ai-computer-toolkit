//! 提供与命令和 provider 无关的有界 UTF-8 JSON 输入读取。

// 导入文件、流读取和路径类型。
use std::{fs::File, io::Read, path::Path};

// 导入公开 JSON 值类型。
use serde_json::Value;

// 固定所有 CLI JSON 输入的最大字节数为十六 MiB。
pub(crate) const MAX_JSON_INPUT_BYTES: usize = 16 * 1024 * 1024;

// 描述有界 JSON 输入 Component 的封闭失败集合。
#[derive(Debug)]
pub(crate) enum BoundedJsonInputError {
    // 表示打开、查询或读取来源失败。
    Read(std::io::Error),
    // 表示已经观察到输入超过硬上限。
    TooLarge {
        // 保存至少已经观察到的字节数。
        observed_bytes: u64,
        // 保存公开硬上限。
        maximum_bytes: usize,
    },
    // 表示字节不是有效 UTF-8 或文本不是有效 JSON。
    Invalid(String),
}

// 从标准输入读取一个有界 JSON 值。
pub(crate) fn read_stdin() -> Result<Value, BoundedJsonInputError> {
    // 取得标准输入句柄并限制锁的有效期。
    let input = std::io::stdin();
    // 锁定当前调用期，避免逐次读取重复获取锁。
    let locked = input.lock();
    // 使用与文件相同的流式硬上限。
    read_stream(locked)
}

// 从已经解析的文件路径读取一个有界 JSON 值。
pub(crate) fn read_file(path: &Path) -> Result<Value, BoundedJsonInputError> {
    // 先打开精确文件，使 metadata 与后续读取绑定同一对象。
    let file = File::open(path).map_err(BoundedJsonInputError::Read)?;
    // 读取已打开文件的当前元数据用于快速拒绝明显超限输入。
    let metadata = file.metadata().map_err(BoundedJsonInputError::Read)?;
    // 已知文件长度超过上限时不分配正文缓冲区。
    if metadata.len() > MAX_JSON_INPUT_BYTES as u64 {
        // 返回精确 metadata 长度和稳定最大值。
        return Err(BoundedJsonInputError::TooLarge {
            // 报告已打开文件的当前长度。
            observed_bytes: metadata.len(),
            // 报告统一硬上限。
            maximum_bytes: MAX_JSON_INPUT_BYTES,
        });
    }
    // 即使文件在 metadata 后增长，流式读取仍保持硬上限。
    read_stream(file)
}

// 从任意同步字节流读取并解析一个有界 JSON 值。
fn read_stream(mut reader: impl Read) -> Result<Value, BoundedJsonInputError> {
    // 只为常见小输入预留有限初始空间。
    let mut bytes = Vec::with_capacity(8 * 1024);
    // 最多观察上限加一个字节，以确定是否真正超限。
    let mut limited = reader.by_ref().take((MAX_JSON_INPUT_BYTES + 1) as u64);
    // 读取有界字节并保留底层 I/O 错误。
    limited
        // 把已读取字节追加到唯一缓冲区。
        .read_to_end(&mut bytes)
        // 映射为 Component 自有错误。
        .map_err(BoundedJsonInputError::Read)?;
    // 多出的一个观察字节证明输入超过硬上限。
    if bytes.len() > MAX_JSON_INPUT_BYTES {
        // 返回有界观察量而不继续读取来源。
        return Err(BoundedJsonInputError::TooLarge {
            // 最多只会报告上限加一。
            observed_bytes: bytes.len() as u64,
            // 报告统一硬上限。
            maximum_bytes: MAX_JSON_INPUT_BYTES,
        });
    }
    // 在不复制正文的情况下验证 UTF-8。
    let text = String::from_utf8(bytes).map_err(|error| {
        // 隐藏原始字节，只返回安全编码错误位置。
        BoundedJsonInputError::Invalid(format!(
            // 使用稳定中文前缀说明编码要求。
            "JSON 输入必须是有效 UTF-8：{}",
            // 仅公开标准 UTF-8 错误诊断。
            error.utf8_error()
        ))
    })?;
    // 兼容文件开头的单个 UTF-8 BOM 并解析完整 JSON。
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|error| {
        // JSON 语法错误不改变为 I/O 失败。
        BoundedJsonInputError::Invalid(error.to_string())
    })
}

// 验证精确上限、超限观察、BOM 与无效输入语义。
#[cfg(test)]
mod tests {
    // 导入内存字节流和自定义读取 trait。
    use std::io::{Cursor, Read};

    // 导入待测函数、错误与硬上限。
    use super::{BoundedJsonInputError, MAX_JSON_INPUT_BYTES, read_stream};
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 验证恰好达到硬上限的合法 JSON 可以完整解析。
    #[test]
    fn exact_limit_json_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        // 两个 JSON 引号之外全部使用单字节字符填满上限。
        let document = format!("\"{}\"", "x".repeat(MAX_JSON_INPUT_BYTES - 2));
        // 解析恰好等于上限的内存流。
        let value = read_stream(Cursor::new(document.as_bytes())).map_err(test_error)?;
        // 解析后的字符串长度等于正文字符数。
        assert_eq!(value.as_str().map(str::len), Some(MAX_JSON_INPUT_BYTES - 2));
        // 测试正常完成。
        Ok(())
    }

    // 验证上限加一只读取到足以证明超限的字节数。
    #[test]
    fn one_byte_over_limit_is_rejected_with_bounded_observation() {
        // 构造比硬上限多一个字节的合法 JSON 字符串。
        let document = format!("\"{}\"", "x".repeat(MAX_JSON_INPUT_BYTES - 1));
        // 执行有界读取并保存错误。
        let error = read_stream(Cursor::new(document.as_bytes())).err();
        // 只接受结构化超限错误。
        match error {
            // 核对观察量和最大值。
            Some(BoundedJsonInputError::TooLarge {
                // 读取有限观察量。
                observed_bytes,
                // 读取固定上限。
                maximum_bytes,
            }) => {
                // 流式读取最多观察上限加一。
                assert_eq!(observed_bytes, (MAX_JSON_INPUT_BYTES + 1) as u64);
                // 错误回报固定硬上限。
                assert_eq!(maximum_bytes, MAX_JSON_INPUT_BYTES);
            }
            // 任何其他结果都表示硬上限失效。
            _ => panic!("oversized JSON must return TooLarge"),
        }
    }

    // 验证 UTF-8 BOM 可以兼容且不改变 JSON 值。
    #[test]
    fn utf8_bom_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        // 构造带单个 BOM 的小型 JSON 对象。
        let document = "\u{feff}{\"ok\":true}";
        // 解析内存输入。
        let value = read_stream(Cursor::new(document.as_bytes())).map_err(test_error)?;
        // BOM 不得进入业务值。
        assert_eq!(value, json!({ "ok": true }));
        // 测试正常完成。
        Ok(())
    }

    // 验证无效 UTF-8 与无效 JSON 都属于输入错误。
    #[test]
    fn invalid_encoding_and_json_are_distinct_from_io_errors() {
        // 构造不合法的单字节 UTF-8。
        let invalid_utf8 = read_stream(Cursor::new([0xff])).err();
        // 编码错误必须映射为 Invalid。
        assert!(matches!(
            invalid_utf8,
            Some(BoundedJsonInputError::Invalid(_))
        ));
        // 构造合法 UTF-8 但不完整的 JSON。
        let invalid_json = read_stream(Cursor::new(b"{".as_slice())).err();
        // 语法错误同样必须映射为 Invalid 而不是 Read。
        assert!(matches!(
            invalid_json,
            Some(BoundedJsonInputError::Invalid(_))
        ));
    }

    // 验证底层读取失败保持为独立 I/O 错误。
    #[test]
    fn reader_failure_is_preserved() {
        // 执行固定失败读取器。
        let error = read_stream(FailingReader).err();
        // I/O 失败不能被误报为无效 JSON。
        assert!(matches!(error, Some(BoundedJsonInputError::Read(_))));
    }

    // 提供不读取任何字节就失败的测试流。
    struct FailingReader;

    // 实现标准同步读取契约。
    impl Read for FailingReader {
        // 每次读取都返回确定性测试错误。
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            // 使用标准 Other 错误模拟来源故障。
            Err(std::io::Error::other("fixture read failure"))
        }
    }

    // 把 Component 错误转换为测试框架可传播的普通错误。
    fn test_error(error: BoundedJsonInputError) -> Box<dyn std::error::Error> {
        // 保留 Debug 证据但不为生产错误引入 Display 实现。
        Box::new(std::io::Error::other(format!("{error:?}")))
    }
}
