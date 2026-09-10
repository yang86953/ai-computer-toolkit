// 导入 worker 私有纯函数。
use super::{normalize_line, read_devtools_endpoint};

// 验证输入 framing 只接受一条非空有界 UTF-8 行。
#[test]
// 覆盖 LF、CRLF、空行和非法 UTF-8。
fn input_line_normalization_is_strict() {
    // LF 被移除。
    assert_eq!(normalize_line(b"{}\n".to_vec()).as_deref(), Some("{}"));
    // CRLF 被移除。
    assert_eq!(normalize_line(b"{}\r\n".to_vec()).as_deref(), Some("{}"));
    // 空行被拒绝。
    assert!(normalize_line(b"\n".to_vec()).is_none());
    // 非 UTF-8 被拒绝。
    assert!(normalize_line(vec![0xff]).is_none());
}

// 验证调试端口事实不接受额外行或危险路径。
#[test]
// 使用工具自有临时目录保存固定事实。
fn devtools_active_port_shape_is_closed() {
    // 创建测试专用临时根。
    let root = std::env::temp_dir().join(format!(
        // 名称只使用进程身份。
        "act-browser-session-worker-test-{}",
        // 注入当前测试进程 ID。
        std::process::id()
    ));
    // 清理可能的陈旧同名目录。
    let _ = std::fs::remove_dir_all(&root);
    // 创建测试目录。
    std::fs::create_dir(&root)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("fixture directory failed: {error}"));
    // 写入合法两行事实。
    std::fs::write(
        // 使用固定 Chromium 文件名。
        root.join("DevToolsActivePort"),
        // 提供合法端口与 browser token。
        "9222\n/devtools/browser/fixture_token\n",
    )
    // 测试环境失败时给出明确诊断。
    .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    // 合法事实被解析。
    assert_eq!(
        // 读取私有 endpoint。
        read_devtools_endpoint(&root),
        // 核对端口与规范路径。
        Some((9222, "/devtools/browser/fixture_token".to_owned()))
    );
    // 写入含第三行的漂移事实。
    std::fs::write(
        // 覆盖同一测试文件。
        root.join("DevToolsActivePort"),
        // 添加非法第三行。
        "9222\n/devtools/browser/fixture\nextra\n",
    )
    // 测试环境失败时给出明确诊断。
    .unwrap_or_else(|error| panic!("fixture rewrite failed: {error}"));
    // 漂移事实被拒绝。
    assert!(read_devtools_endpoint(&root).is_none());
    // 回收精确测试目录。
    std::fs::remove_dir_all(&root)
        // 清理失败必须暴露测试污染。
        .unwrap_or_else(|error| panic!("fixture cleanup failed: {error}"));
}
