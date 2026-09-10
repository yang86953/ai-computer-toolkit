#![cfg(target_os = "windows")]

// 引入有序集合以稳定展示契约差集。
use std::collections::BTreeSet;
// 引入通用错误边界以便测试传播读取和解析失败。
use std::error::Error;
// 引入文件系统接口以遍历生产 Rust 源码。
use std::fs;
// 引入输入输出错误以构造结构化契约错误。
use std::io;
// 引入路径类型以定位仓库文件。
use std::path::{Path, PathBuf};

// 定义仓库根目录以避免测试依赖启动目录。
const REPOSITORY_ROOT: &str = env!("CARGO_MANIFEST_DIR");
// 定义公开错误 envelope schema 的仓库内位置。
const ERROR_SCHEMA_PATH: &str = "contracts/v1/error-envelope.schema.json";
// 定义唯一生产 launcher 的仓库内位置。
const PRODUCTION_LAUNCHER_PATH: &str = "tools/windows/Invoke-ComputerControl.ps1";

// 验证生产源码中的错误码全部进入封闭公开 schema。
#[test]
// 允许测试传播文件系统和 JSON 解析错误。
fn production_error_codes_are_declared_by_public_schema() -> Result<(), Box<dyn Error>> {
    // 将编译期仓库根目录转换为路径。
    let repository_root = Path::new(REPOSITORY_ROOT);
    // 从公开 schema 读取允许的封闭错误码集合。
    let schema_codes = load_schema_codes(&repository_root.join(ERROR_SCHEMA_PATH))?;
    // 从非测试生产源码收集形似公开错误码的字符串。
    let production_codes = collect_production_error_codes(repository_root)?;
    // 计算生产源码中尚未登记到 schema 的错误码。
    let missing_codes = production_codes
        // 使用集合差集避免顺序和重复值影响结果。
        .difference(&schema_codes)
        // 复制差集值以便失败信息拥有独立数据。
        .cloned()
        // 收集为稳定排序的列表。
        .collect::<Vec<_>>();

    // 确认扫描器覆盖浏览器错误域的代表值。
    assert!(production_codes.contains("BROWSER_TIMEOUT"));
    // 确认扫描器覆盖捕获错误域的代表值。
    assert!(production_codes.contains("CAPTURE_TARGET_INELIGIBLE"));
    // 确认扫描器覆盖视频错误域的代表值。
    assert!(production_codes.contains("VIDEO_ENCODER_FAILED"));
    // 确认扫描器覆盖 worker 协议错误域的代表值。
    assert!(production_codes.contains("WORKER_PROTOCOL_VIOLATION"));
    // 确认扫描器覆盖唯一生产 launcher 的结构化失败值。
    assert!(production_codes.contains("COMPATIBILITY_RUNTIME_UNAVAILABLE"));
    // 要求所有生产错误码均受封闭 schema 管理。
    assert!(
        missing_codes.is_empty(),
        "生产错误码未登记到 error-envelope schema: {missing_codes:?}"
    );
    // 返回成功并保留上游错误传播能力。
    Ok(())
}

// 从公开 schema 中读取 code enum 的字符串集合。
fn load_schema_codes(schema_path: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    // 读取完整 schema 文本。
    let schema_text = fs::read_to_string(schema_path)?;
    // 将 schema 解析为通用 JSON 树。
    let schema: serde_json::Value = serde_json::from_str(&schema_text)?;
    // 定位必须保持封闭枚举的错误码节点。
    let code_values = schema
        // 使用 JSON Pointer 明确绑定公开 envelope 结构。
        .pointer("/properties/error/properties/code/enum")
        // 要求节点仍为数组而不是开放字符串或正则模式。
        .and_then(serde_json::Value::as_array)
        // 将契约形状漂移转换为可读的测试错误。
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "错误码 schema 缺少封闭 enum"))?;
    // 初始化稳定排序的 schema 错误码集合。
    let mut schema_codes = BTreeSet::new();
    // 逐项验证枚举只包含字符串。
    for code_value in code_values {
        // 将当前枚举值读取为字符串。
        let code = code_value.as_str().ok_or_else(|| {
            // 报告非字符串值会破坏稳定 JSON 协议。
            io::Error::new(
                io::ErrorKind::InvalidData,
                "错误码 schema enum 包含非字符串值",
            )
        })?;
        // 登记当前公开错误码。
        schema_codes.insert(code.to_owned());
    }
    // 返回封闭错误码集合。
    Ok(schema_codes)
}

// 收集生产 Rust 源码和唯一 launcher 中的候选错误码。
fn collect_production_error_codes(repository_root: &Path) -> io::Result<BTreeSet<String>> {
    // 初始化待扫描文件列表。
    let mut rust_files = Vec::new();
    // 递归收集 src 下的生产 Rust 文件。
    collect_rust_files(&repository_root.join("src"), &mut rust_files)?;
    // 固定扫描顺序以稳定诊断结果。
    rust_files.sort();
    // 初始化去重且稳定排序的候选集合。
    let mut production_codes = BTreeSet::new();
    // 逐个扫描生产 Rust 文件。
    for rust_file in rust_files {
        // 读取当前 Rust 源码。
        let source = fs::read_to_string(&rust_file)?;
        // 排除文件尾部的内联测试模块。
        let production_source = production_prefix(&source);
        // 从普通字符串字面量中收集候选错误码。
        for literal in ordinary_string_literals(production_source) {
            // 只保留错误码形状且不在非错误常量白名单中的值。
            if is_error_code_candidate(&literal) && !is_non_error_literal(&literal) {
                // 登记当前生产错误码候选值。
                production_codes.insert(literal);
            }
        }
    }
    // 读取唯一受支持的生产 PowerShell launcher。
    let launcher_source = fs::read_to_string(repository_root.join(PRODUCTION_LAUNCHER_PATH))?;
    // 收集 launcher 中单引号包围的结构化错误码。
    for literal in powershell_single_quoted_literals(&launcher_source) {
        // 复用与 Rust 源码相同的候选形状和精确白名单。
        if is_error_code_candidate(&literal) && !is_non_error_literal(&literal) {
            // 登记当前 launcher 错误码候选值。
            production_codes.insert(literal);
        }
    }
    // 返回完整生产错误码集合。
    Ok(production_codes)
}

// 递归收集非测试 Rust 文件。
fn collect_rust_files(directory: &Path, rust_files: &mut Vec<PathBuf>) -> io::Result<()> {
    // 遍历当前目录的直接子项。
    for entry in fs::read_dir(directory)? {
        // 读取目录项并传播访问错误。
        let entry = entry?;
        // 取得目录项路径用于后续分类。
        let path = entry.path();
        // 递归进入子目录。
        if path.is_dir() {
            // 继续收集嵌套 Module、Component 与 adapter 源码。
            collect_rust_files(&path, rust_files)?;
            // 当前目录项已经处理完成。
            continue;
        }
        // 读取文件名并在无有效 UTF-8 名称时跳过。
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // 排除独立私有测试文件。
        if file_name == "tests.rs" || file_name.ends_with("_tests.rs") {
            // 测试夹具错误码不属于生产公开协议。
            continue;
        }
        // 只扫描 Rust 源码文件。
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            // 将生产 Rust 文件加入稳定扫描列表。
            rust_files.push(path);
        }
    }
    // 当前目录收集成功。
    Ok(())
}

// 返回首次内联测试模块之前的生产源码。
fn production_prefix(source: &str) -> &str {
    // 定位约定的内联测试模块属性。
    if let Some(test_module_start) = source.find("#[cfg(test)]") {
        // 截断测试模块及其全部测试专用字面量。
        &source[..test_module_start]
    // 没有内联测试模块时保留完整源码。
    } else {
        // 返回完整生产源码。
        source
    }
}

// 提取普通双引号字符串字面量的未转义内容。
fn ordinary_string_literals(source: &str) -> Vec<String> {
    // 使用字节扫描精确识别 ASCII 引号与转义符。
    let bytes = source.as_bytes();
    // 初始化扫描游标。
    let mut index = 0;
    // 初始化字面量结果列表。
    let mut literals = Vec::new();
    // 扫描全部源码字节。
    while index < bytes.len() {
        // 跳过非双引号起点。
        if bytes[index] != b'"' {
            // 推进到下一个字节。
            index += 1;
            // 继续寻找普通字符串起点。
            continue;
        }
        // 越过字符串起始引号。
        index += 1;
        // 初始化当前字面量的原始 UTF-8 字节。
        let mut literal_bytes = Vec::new();
        // 记录前一个字节是否为转义符。
        let mut escaped = false;
        // 记录当前字符串是否找到闭合引号。
        let mut closed = false;
        // 扫描当前普通字符串内容。
        while index < bytes.len() {
            // 读取当前字节。
            let current = bytes[index];
            // 转义内容不可能形成当前约束的全大写错误码。
            if escaped {
                // 清除转义状态。
                escaped = false;
                // 推进到下一个字节。
                index += 1;
                // 继续扫描当前字符串。
                continue;
            }
            // 进入转义状态并忽略反斜杠。
            if current == b'\\' {
                // 标记下一个字节已转义。
                escaped = true;
                // 推进到转义内容。
                index += 1;
                // 继续扫描当前字符串。
                continue;
            }
            // 识别字符串闭合引号。
            if current == b'"' {
                // 标记当前字面量完整闭合。
                closed = true;
                // 越过闭合引号。
                index += 1;
                // 结束当前字符串扫描。
                break;
            }
            // 保存普通字符串的当前字节。
            literal_bytes.push(current);
            // 推进到下一个字符串字节。
            index += 1;
        }
        // 只处理闭合且保持有效 UTF-8 的普通字符串。
        if closed && let Ok(literal) = String::from_utf8(literal_bytes) {
            // 保存当前普通字符串内容。
            literals.push(literal);
        }
    }
    // 返回全部普通字符串内容。
    literals
}

// 提取 PowerShell 单引号字符串字面量并处理成对单引号转义。
fn powershell_single_quoted_literals(source: &str) -> Vec<String> {
    // 使用字节扫描精确识别 ASCII 单引号。
    let bytes = source.as_bytes();
    // 初始化扫描游标。
    let mut index = 0;
    // 初始化字面量结果列表。
    let mut literals = Vec::new();
    // 扫描全部 launcher 字节。
    while index < bytes.len() {
        // 跳过非单引号起点。
        if bytes[index] != b'\'' {
            // 推进到下一个字节。
            index += 1;
            // 继续寻找 PowerShell 字符串起点。
            continue;
        }
        // 越过字符串起始单引号。
        index += 1;
        // 初始化当前字面量的原始 UTF-8 字节。
        let mut literal_bytes = Vec::new();
        // 记录当前字符串是否找到闭合单引号。
        let mut closed = false;
        // 扫描当前 PowerShell 单引号字符串。
        while index < bytes.len() {
            // 读取当前字节。
            let current = bytes[index];
            // 普通字节直接进入当前字符串。
            if current != b'\'' {
                // 保存当前字符串字节。
                literal_bytes.push(current);
                // 推进到下一个字节。
                index += 1;
                // 继续扫描当前字符串。
                continue;
            }
            // 成对单引号表示 PowerShell 字符串内的转义单引号。
            if index + 1 < bytes.len() && bytes[index + 1] == b'\'' {
                // 保存转义后的单引号。
                literal_bytes.push(b'\'');
                // 越过两个转义字节。
                index += 2;
                // 继续扫描当前字符串。
                continue;
            }
            // 标记当前字面量完整闭合。
            closed = true;
            // 越过闭合单引号。
            index += 1;
            // 结束当前字符串扫描。
            break;
        }
        // 只处理闭合且保持有效 UTF-8 的 PowerShell 字符串。
        if closed && let Ok(literal) = String::from_utf8(literal_bytes) {
            // 保存当前单引号字符串内容。
            literals.push(literal);
        }
    }
    // 返回全部 PowerShell 单引号字符串内容。
    literals
}

// 判断字符串是否符合稳定错误码形状。
fn is_error_code_candidate(literal: &str) -> bool {
    // 要求至少三个字符、包含分隔下划线且仅使用大写 ASCII、数字和下划线。
    literal.len() >= 3
        // 要求首字符为大写字母以排除双下划线模板占位符。
        && literal.as_bytes().first().is_some_and(|byte| byte.is_ascii_uppercase())
        // 排除没有领域分隔的普通大写单词。
        && literal.contains('_')
        // 检查每个字节均属于错误码封闭字符集。
        && literal.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

// 判断候选值是否属于已知非错误协议或构建常量。
fn is_non_error_literal(literal: &str) -> bool {
    // 白名单只接受逐项审计过的精确值，不允许前缀或正则放宽。
    matches!(
        literal,
        // 浏览器测试模式环境变量不是运行时错误码。
        "ACT_BROWSER_FIXTURE_MODE"
            // 浏览器会话 runtime 测试模式环境变量不是运行时错误码。
            | "ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE"
            // 浏览器可执行文件环境变量不是运行时错误码。
            | "AI_COMPUTER_TOOLKIT_BROWSER_PATH"
            // Cargo 包名编译期变量不是运行时错误码。
            | "CARGO_PKG_NAME"
            // Cargo 包版本编译期变量不是运行时错误码。
            | "CARGO_PKG_VERSION"
            // Serde 命名规则不是运行时错误码。
            | "SCREAMING_SNAKE_CASE"
            // Win32 消息常量不是运行时错误码。
            | "WM_CLOSE"
    )
}
