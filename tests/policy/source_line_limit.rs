#![cfg(target_os = "windows")]

// 导入文件系统、路径和通用测试错误类型。
use std::{error::Error, fs, path::Path};

// 固定项目规则要求的单个 Rust 文件最大行数。
const MAXIMUM_RUST_FILE_LINES: usize = 900;

// 递归收集一个目录中的 Rust 文件行数。
fn collect_rust_line_counts(
    // 接收 Cargo 固定的仓库根目录。
    repository: &Path,
    // 接收当前需要遍历的目录。
    directory: &Path,
    // 把结果追加到调用方拥有的集合。
    counts: &mut Vec<(String, usize)>,
) -> Result<(), Box<dyn Error>> {
    // 逐项读取当前目录并传播文件系统错误。
    for entry in fs::read_dir(directory)? {
        // 取得当前目录项。
        let entry = entry?;
        // 取得当前项路径。
        let path = entry.path();
        // 取得不跟随符号链接的文件类型。
        let file_type = entry.file_type()?;
        // 子目录使用同一规则递归扫描。
        if file_type.is_dir() {
            // 扫描子目录中的 Rust 文件。
            collect_rust_line_counts(repository, &path, counts)?;
            // 当前目录项已经处理完成。
            continue;
        }
        // 非普通文件不属于当前源码门禁。
        if !file_type.is_file() {
            // 跳过符号链接和特殊文件。
            continue;
        }
        // 只处理扩展名精确为 rs 的 Rust 源码。
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            // 跳过文档、契约和其他资产。
            continue;
        }
        // 读取完整 UTF-8 Rust 源码。
        let source = fs::read_to_string(&path)?;
        // 转换为不依赖工作目录的仓库相对路径。
        let relative = path
            // 移除 Cargo manifest 根目录前缀。
            .strip_prefix(repository)?
            // 使用可稳定格式化的字符串。
            .to_string_lossy()
            // 统一 Windows 与其他环境的路径分隔符。
            .replace('\\', "/");
        // 保存与文本行迭代器一致的实际行数。
        counts.push((relative, source.lines().count()));
    }
    // 当前目录扫描成功。
    Ok(())
}

// 验证项目中的所有手写 Rust 代码文件都满足 900 行硬上限。
#[test]
fn every_rust_source_file_stays_within_nine_hundred_lines() -> Result<(), Box<dyn Error>> {
    // 使用 Cargo manifest 目录固定仓库根路径。
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    // 保存全部受检文件和行数。
    let mut counts = Vec::new();
    // 扫描生产与库内测试源码。
    collect_rust_line_counts(repository, &repository.join("src"), &mut counts)?;
    // 扫描独立集成测试源码。
    collect_rust_line_counts(repository, &repository.join("tests"), &mut counts)?;
    // 使用仓库相对路径排序以稳定失败输出。
    counts.sort_by(|left, right| left.0.cmp(&right.0));
    // 门禁必须实际覆盖拆分后的关键生产文件。
    assert!(counts.iter().any(|(path, _)| path == "src/capabilities.rs"));
    // 门禁必须实际覆盖拆分后的 capability 测试文件。
    assert!(
        counts
            // 遍历全部稳定路径。
            .iter()
            // 查找拆分后的私有测试文件。
            .any(|(path, _)| path == "src/capabilities/tests.rs")
    );
    // 门禁必须实际覆盖拆分后的策略生产文件。
    assert!(counts.iter().any(|(path, _)| path == "src/policy.rs"));
    // 门禁必须实际覆盖拆分后的策略测试文件。
    assert!(
        counts
            // 遍历全部稳定路径。
            .iter()
            // 查找拆分后的私有测试文件。
            .any(|(path, _)| path == "src/policy/tests.rs")
    );
    // 收集全部超过硬上限的稳定诊断文本。
    let oversized = counts
        // 遍历已排序结果。
        .iter()
        // 只保留真正超限的文件。
        .filter(|(_, lines)| *lines > MAXIMUM_RUST_FILE_LINES)
        // 生成仓库相对路径与实际行数。
        .map(|(path, lines)| format!("{path}: {lines}"))
        // 收集为一次完整失败报告。
        .collect::<Vec<_>>();
    // 任一超限文件都必须使自动门禁失败。
    assert!(
        oversized.is_empty(),
        // 输出所有违规项，避免逐个修复后反复运行。
        "Rust files exceed {MAXIMUM_RUST_FILE_LINES} lines: {}",
        // 使用稳定分隔符连接诊断。
        oversized.join(", ")
    );
    // 门禁验证成功。
    Ok(())
}
