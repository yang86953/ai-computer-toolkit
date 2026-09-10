// 导入 CLI 与 System 入口。
use ai_computer_toolkit::{AppControlService, cli};
// 导入通用 JSON 值。
use serde_json::Value;

// Rust build-info 必须保持版本化字段形状和独立运行元数据边界。
#[test]
fn rust_build_info_matches_the_versioned_shape() -> Result<(), Box<dyn std::error::Error>> {
    // 解析版本化公开 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/v1/build-info.schema.json"
    ))?;
    // 通过 System 取得 Rust 构建信息。
    let result = AppControlService::new().build_info();
    // 顶层必须成功。
    assert_eq!(result["ok"], true);
    // 固定 control contract 版本。
    assert_eq!(result["contractVersion"], "act/control/v1");
    // 直接 Rust 调用必须诚实声明实现。
    assert_eq!(result["implementation"], "rust");
    // 语言必须来自 Rust edition 决策。
    assert_eq!(result["data"]["mainLanguage"], "Rust 2024");
    // 兼容版本必须来自 Cargo package 版本。
    assert_eq!(
        result["data"]["compatibilityVersion"],
        env!("CARGO_PKG_VERSION")
    );
    // schema 必须只接受 Rust 正式实现。
    assert_eq!(schema["properties"]["implementation"]["const"], "rust");
    // schema 必须固定当前兼容版本。
    assert_eq!(
        schema["properties"]["data"]["properties"]["compatibilityVersion"]["const"],
        env!("CARGO_PKG_VERSION")
    );
    // 返回形状一致成功。
    Ok(())
}

// build-info 必须拒绝位置参数并保留结构化错误。
#[test]
fn build_info_rejects_positional_arguments() {
    // 调用带多余位置参数的 CLI。
    let result = cli::run(vec![
        // 指定 build-info 命令。
        "build-info".to_owned(),
        // 故意加入非法参数。
        "unexpected".to_owned(),
    ]);
    // 提取预期错误。
    let error = match result {
        // 成功表示封闭命令被放宽。
        Ok(_) => panic!("build-info must reject positional arguments"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 必须使用稳定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}
