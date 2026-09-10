//! 仓库自有的固定 headless browser 测试替身。

// 导入路径与等待时长。
use std::{path::PathBuf, time::Duration};

// 导入 PNG 图像类型。
use image::{Rgba, RgbaImage};

// 执行只理解固定 Chromium 截图参数的测试替身。
fn main() {
    // 挂起模式用于证明 Job timeout 会回收完整进程。
    if std::env::var_os("ACT_BROWSER_FIXTURE_MODE").as_deref() == Some("hang".as_ref()) {
        // 永久等待直到父 Job 终止进程。
        loop {
            // 使用短休眠避免占用 CPU。
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // 初始化可选截图路径。
    let mut output = None;
    // 初始化可选视口尺寸。
    let mut dimensions = None;
    // 遍历固定进程参数。
    for argument in std::env::args().skip(1) {
        // 解析固定 screenshot 参数。
        if let Some(value) = argument.strip_prefix("--screenshot=") {
            // 保存输出路径。
            output = Some(PathBuf::from(value));
        }
        // 解析固定 window-size 参数。
        if let Some(value) = argument.strip_prefix("--window-size=") {
            // 按单个逗号切分尺寸。
            let mut parts = value.split(',');
            // 解析宽度。
            let width = parts.next().and_then(|value| value.parse::<u32>().ok());
            // 解析高度。
            let height = parts.next().and_then(|value| value.parse::<u32>().ok());
            // 只接受恰好两个字段。
            if parts.next().is_none() {
                // 保存完整尺寸。
                dimensions = width.zip(height);
            }
        }
    }
    // 缺失固定输出参数时返回失败。
    let Some(output) = output else {
        // 使用非零退出码。
        std::process::exit(2);
    };
    // 缺失固定尺寸参数时返回失败。
    let Some((width, height)) = dimensions else {
        // 使用非零退出码。
        std::process::exit(2);
    };
    // 创建确定性 RGBA 图像。
    let image = RgbaImage::from_pixel(width, height, Rgba([42, 96, 168, 255]));
    // 写入 PNG，失败时返回非零。
    if image.save(output).is_err() {
        // 使用非零退出码。
        std::process::exit(2);
    }
}
