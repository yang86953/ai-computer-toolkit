#![cfg(target_os = "windows")]

// 导入仓库路径类型。
use std::path::PathBuf;

// 验证前景 API 只存在于已受统一前景同意保护的 Adapter。
#[test]
fn foreground_apis_are_isolated_to_consent_gated_input_adapters()
-> Result<(), Box<dyn std::error::Error>> {
    // 定位 Adapter 源码根目录。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        // 进入源码目录。
        .join("src")
        // 进入 Adapter 目录。
        .join("adapters");
    // 允许 legacy 桌面前台 Adapter。
    let allowed_desktop_adapter = root.join("desktop.rs");
    // Pointer Input Module 只通过该窄平台 Adapter 调用前台 API。
    let allowed_pointer_adapter = root.join("pointer_input_windows.rs");
    // Keyboard Input Module 只通过该窄平台 Adapter 发送键盘输入。
    let allowed_keyboard_adapter = root.join("keyboard_input_windows.rs");
    // 共享前景 Component 是唯一承载窗口恢复与激活 API 的边界。
    let allowed_foreground_adapter = root.join("foreground_input_windows.rs");
    // 固定禁止扩散的平台符号。
    let forbidden = [
        // 前景窗口切换。
        "SetForegroundWindow",
        // 统一前台输入。
        "SendInput",
        // 原生焦点切换。
        "SetFocus",
        // 原生剪贴板写入。
        "SetClipboardData",
    ];
    // 从 Adapter 根目录开始递归扫描。
    let mut pending = vec![root.clone()];
    // 逐目录处理。
    while let Some(directory) = pending.pop() {
        // 枚举当前目录项。
        for entry in std::fs::read_dir(directory)? {
            // 取得仓库内路径。
            let path = entry?.path();
            // 子目录继续递归。
            if path.is_dir() {
                // 加入待处理目录。
                pending.push(path);
                // 跳过文件逻辑。
                continue;
            }
            // 只扫描 Rust 源码。
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                // 跳过非 Rust 文件。
                continue;
            }
            // 读取当前源码。
            let source = std::fs::read_to_string(&path)?;
            // legacy Adapter 继续要求本地前景同意门禁。
            if path == allowed_desktop_adapter {
                // 核对同意字段存在。
                assert!(source.contains("foreground_consent"));
                // 核对恢复只在该边界内发生。
                assert!(source.contains("ShowWindowAsync"));
                // 允许继续扫描。
                continue;
            }
            // 共享前景 Component 的授权由统一 app provider 在调用前完成。
            if path == allowed_foreground_adapter {
                // 平台文件必须保持 crate 私有边界。
                assert!(!source.contains("pub fn"));
                // 共享 Component 必须承载窗口激活。
                assert!(source.contains("SetForegroundWindow"));
                // 共享 Component 不得承载具体输入注入。
                assert!(!source.contains("SendInput"));
                // 读取统一 app provider。
                let provider = std::fs::read_to_string(root.join("app").join("desktop.rs"))?;
                // 核对统一前景同意门禁存在。
                assert!(provider.contains("ensure_capability_foreground_consent"));
                // 核对该门禁保护版本化键盘路由。
                assert!(provider.contains("UI_INPUT_KEY"));
                // 核对该门禁保护版本化指针路由。
                assert!(provider.contains("UI_INPUT_POINTER"));
                // 允许继续扫描。
                continue;
            }
            // 键鼠注入 Adapter 只能承载 SendInput，不得直接切换前景窗口。
            if path == allowed_pointer_adapter || path == allowed_keyboard_adapter {
                // 平台文件必须保持 crate 私有边界。
                assert!(!source.contains("pub fn"));
                // 窄 Adapter 必须承载统一输入注入。
                assert!(source.contains("SendInput"));
                // 窄 Adapter 不得越过共享前景 Component。
                assert!(!source.contains("SetForegroundWindow"));
                // 允许继续扫描。
                continue;
            }
            // 其他 Adapter 不得引用任何前景写 API。
            for symbol in forbidden {
                // 报告精确文件与符号。
                assert!(
                    !source.contains(symbol),
                    "{} must not call forbidden API {}",
                    path.display(),
                    symbol
                );
            }
        }
    }
    // 完成策略扫描。
    Ok(())
}
