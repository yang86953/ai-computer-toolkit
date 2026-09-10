#![cfg(target_os = "windows")]

// 引入错误边界供版本化契约测试传播读取失败。
use std::error::Error;
// 引入路径类型以定位仓库生产源码。
use std::path::PathBuf;

// 引入 JSON 值类型以核对策略清单。
use serde_json::Value;

// 验证 Chromium 外部进程只暴露认证动态槽和固定模板。
#[test]
// 联合核对策略字段集和 Rust/C++ 生产源码门禁。
fn external_processes_use_fixed_argument_templates() -> Result<(), Box<dyn Error>> {
    // 取得当前 Cargo 包根目录。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 读取版本化固定参数策略。
    let policy_text = std::fs::read_to_string(
        // 策略清单与测试一起进入仓库。
        root.join("tests/contracts/fixed-external-process-policy-v1.json"),
    )?;
    // 解析 JSON 策略对象。
    let policy: Value = serde_json::from_str(&policy_text)?;
    // 契约版本必须保持稳定。
    assert_eq!(
        policy["contractVersion"],
        "act/fixed-external-process-policy/v1"
    );
    // 外部进程绝不经过 shell。
    assert_eq!(policy["shellAllowed"], false);
    // 未知字段必须失败闭合。
    assert_eq!(policy["unknownFieldsRejected"], true);
    // Chromium flag 不能由调用方提供。
    assert_eq!(policy["browser"]["callerSuppliedFlagsAllowed"], false);

    // 读取所有明确禁止的输入字段。
    let forbidden = policy["forbiddenInputFields"]
        // 字段必须是数组。
        .as_array()
        // 非数组策略直接使测试失败。
        .ok_or("forbiddenInputFields must be an array")?;
    // 核对浏览器公开动态槽没有混入禁止字段。
    for field in policy["browser"]["allowedArgumentFields"]
        // 浏览器字段必须是数组。
        .as_array()
        // 非数组策略直接使测试失败。
        .ok_or("browser allowedArgumentFields must be an array")?
    {
        // 禁止字段不得出现在公开白名单。
        assert!(
            !forbidden.contains(field),
            "browser allowed forbidden field {field}"
        );
    }
    // 核对每个 Rust/C++ 生产入口仍包含认证门禁和固定模板标记。
    for assertion in policy["sourceAssertions"]
        // 源码断言必须是数组。
        .as_array()
        // 非数组策略直接使测试失败。
        .ok_or("sourceAssertions must be an array")?
    {
        // 取得仓库相对源码路径。
        let relative = assertion["path"]
            // 路径必须是字符串。
            .as_str()
            // 非字符串策略直接使测试失败。
            .ok_or("source assertion path must be a string")?;
        // 读取生产源码文本。
        let source = std::fs::read_to_string(root.join(relative))?;
        // 逐个核对门禁函数和固定模板标记。
        for token in assertion["requiredTokens"]
            // 必需标记必须是数组。
            .as_array()
            // 非数组策略直接使测试失败。
            .ok_or("requiredTokens must be an array")?
        {
            // 标记必须是字符串。
            let token = token.as_str().ok_or("required token must be a string")?;
            // 生产源码缺少任一标记都表示固定参数契约漂移。
            assert!(
                source.contains(token),
                "{relative} lost fixed-argument token {token}"
            );
        }
    }
    // 版本化策略与全部生产入口一致。
    Ok(())
}
