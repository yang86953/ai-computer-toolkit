// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};

// 返回与 C++ 对照实现同形状的 Rust 构建元数据。
pub(crate) fn render() -> Value {
    // 使用稳定 control envelope，构建信息不进入 doctor/status。
    json!({
        // 标记请求成功。
        "ok": true,
        // 固定公开控制契约版本。
        "contractVersion": "act/control/v1",
        // 声明当前直接调用实现。
        "implementation": "rust",
        // 输出 Rust 正式构建元数据对象。
        "data": {
            // 固定产品名称。
            "name": env!("CARGO_PKG_NAME"),
            // Rust 构建版本来自 Cargo package 单一来源。
            "version": env!("CARGO_PKG_VERSION"),
            // 声明 edition 对应的主实现语言。
            "mainLanguage": "Rust 2024",
            // 兼容 CLI 版本与 package version 保持一致。
            "compatibilityVersion": env!("CARGO_PKG_VERSION"),
        },
    })
}
