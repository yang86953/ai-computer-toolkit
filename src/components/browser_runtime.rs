//! 私下发现固定 Chromium runtime，不跨公共边界公开路径。

// 导入路径类型。
use std::path::{Path, PathBuf};

// 返回当前认证 Chromium runtime 的私有路径。
pub(crate) fn find() -> Option<PathBuf> {
    // 显式主机配置优先，供受控安装与仓库 fixture 使用。
    if let Some(path) = std::env::var_os("AI_COMPUTER_TOOLKIT_BROWSER_PATH").map(PathBuf::from) {
        // 只接受当前存在的普通文件。
        if path.is_file() {
            // 返回配置路径但不进入公开 JSON。
            return Some(path);
        }
    }
    // 固定只检查系统级 Program Files 根。
    let roots = ["ProgramFiles", "ProgramFiles(x86)"];
    // 固定认证 Chrome 与 Edge 安装相对路径。
    let relative_paths = [
        // 构造 Google Chrome 默认安装路径。
        Path::new("Google")
            // 进入产品目录。
            .join("Chrome")
            // 进入应用目录。
            .join("Application")
            // 选择固定可执行文件。
            .join("chrome.exe"),
        // 构造 Microsoft Edge 默认安装路径。
        Path::new("Microsoft")
            // 进入产品目录。
            .join("Edge")
            // 进入应用目录。
            .join("Application")
            // 选择固定可执行文件。
            .join("msedge.exe"),
    ];
    // 遍历固定环境根。
    roots
        // 转换为迭代器。
        .into_iter()
        // 跳过缺失环境变量。
        .filter_map(std::env::var_os)
        // 转换为路径。
        .map(PathBuf::from)
        // 组合固定相对候选。
        .flat_map(|root| {
            // 借用候选列表。
            relative_paths
                // 遍历相对路径。
                .iter()
                // 与当前根组合。
                .map(move |relative| root.join(relative))
        })
        // 返回第一个真实文件。
        .find(|path| path.is_file())
}
