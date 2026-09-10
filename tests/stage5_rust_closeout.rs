#![cfg(target_os = "windows")]

// 导入只读文件系统与仓库路径类型。
use std::{fs, path::PathBuf};

// 导入语言中立 JSON 值。
use serde_json::Value;

// 返回当前测试所属仓库根目录。
fn repository_root() -> PathBuf {
    // 使用 Cargo 固定 manifest 目录避免依赖调用方工作目录。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// 读取并解析版本化 JSON 契约。
fn read_json(path: &PathBuf) -> Result<Value, Box<dyn std::error::Error>> {
    // 读取指定仓库文件。
    let bytes = fs::read(path)?;
    // 解析严格 JSON。
    Ok(serde_json::from_slice(&bytes)?)
}

// 阶段 5 旧入口必须只保留 Rust 生产路线。
#[test]
// 核对 descriptor、隔离认证与已退役 C++ 门禁资产。
fn stage5_routes_are_rust_only() -> Result<(), Box<dyn std::error::Error>> {
    // 定位仓库根目录。
    let root = repository_root();
    // 加载阶段 5 收口契约。
    let policy = read_json(&root.join("tests/contracts/stage5-rust-closeout-policy.json"))?;
    // 固定执行语言必须为 Rust。
    assert_eq!(policy["executionLanguage"], "rust");
    // 旧 C++ descriptor 必须明确退役。
    assert_eq!(policy["descriptorStatus"], "retired-rust-primary");
    // 不得继续授权 C++ 执行。
    assert_eq!(policy["cppExecutionEnabled"], false);
    // 不得再要求跨语言动态等价。
    assert_eq!(policy["cppDynamicParityRequired"], false);
    // 生产 launcher 不得保留阶段 5 回退。
    assert_eq!(policy["launcherFallback"], "none");
    // 固定 sequence worker 必须登记为认证 Rust 产物。
    assert!(
        // 读取认证 worker 集合并核对固定文件名。
        policy["certifiedWorkers"]
            // 字段必须是数组。
            .as_array()
            // 缺失字段视为未认证。
            .is_some_and(|workers| workers.contains(&Value::String(
                // 使用生产 runner 唯一允许的 sibling 文件名。
                "ai-computer-toolkit-sequence-step-worker.exe".to_owned()
            )))
    );

    // 逐个核对已退役的 C++ 验证脚本已从仓库删除。
    for script in policy["retiredValidationScripts"]
        // 字段必须是数组。
        .as_array()
        // 错误契约立即失败。
        .ok_or("retiredValidationScripts must be an array")?
    {
        // 取得脚本相对路径。
        let relative = script
            // 字段元素必须是字符串。
            .as_str()
            // 错误类型立即失败。
            .ok_or("retired validation script must be a string")?;
        // 退役脚本不得继续存在。
        assert!(
            !root.join(relative).exists(),
            "retired script remains: {relative}"
        );
    }

    // 加载严格隔离策略。
    let isolation = read_json(&root.join("tests/contracts/strict-isolation-policy-v1.json"))?;
    // 取得认证 Rust worker 路由集合。
    let certified = isolation["certifiedRustWorkerRoutes"]
        // 字段必须是数组。
        .as_array()
        // 错误契约立即失败。
        .ok_or("certifiedRustWorkerRoutes must be an array")?;
    // 核对五个公开或兼容入口都已完成隔离认证。
    for route in [
        // 浏览器截图旧入口。
        "browser.screenshot",
        // desktop 截图旧入口。
        "desktop.screenshot",
        // desktop 录制旧入口。
        "desktop.record",
        // provider-neutral 截图 capability。
        "window.screenshot@1",
        // provider-neutral 录制 capability。
        "window.record@1",
    ] {
        // 认证集合必须包含当前路线。
        assert!(
            // 查询语言中立字符串值。
            certified.contains(&Value::String(route.to_owned())),
            // 输出缺失路线。
            "route is not certified: {route}"
        );
    }

    // 加载 Rust capability metadata 实现。
    let metadata = fs::read_to_string(root.join("src/modules/capability_metadata.rs"))?;
    // 静态 descriptor 必须发布统一退役状态。
    assert!(metadata.contains("retired-rust-primary"));
    // 返回门禁成功。
    Ok(())
}
