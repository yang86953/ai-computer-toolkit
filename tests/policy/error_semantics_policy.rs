#![cfg(target_os = "windows")]

// 导入通用 JSON 值。
use serde_json::Value;

// 统一错误策略清单必须被封闭错误 envelope 完整接受。
#[test]
fn error_semantics_manifest_matches_public_error_envelope() -> Result<(), Box<dyn std::error::Error>>
{
    // 解析版本化错误策略清单。
    let policy: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/error-semantics-policy-v1.json"
    ))?;
    // 解析公开错误 envelope schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/error-envelope.schema.json"
    ))?;
    // 取得 schema 的封闭错误码集合。
    let codes = schema["properties"]["error"]["properties"]["code"]["enum"]
        // 要求数组形状。
        .as_array()
        // 将损坏 schema 转为测试错误。
        .ok_or_else(|| std::io::Error::other("error code enum must be an array"))?;
    // 每个任务要求的错误码都必须进入公开 envelope。
    for required in policy["requiredCodes"]
        // 要求数组形状。
        .as_array()
        // 将损坏清单转为测试错误。
        .ok_or_else(|| std::io::Error::other("requiredCodes must be an array"))?
    {
        // 禁止运行时错误码被封闭 schema 拒绝。
        assert!(codes.contains(required));
    }
    // outcome unknown 必须显式禁止自动重试。
    assert_eq!(policy["unknownOutcome"]["outcome"], "unknown");
    // retrySafe 必须保持 false。
    assert_eq!(policy["unknownOutcome"]["retrySafe"], false);
    // 兼容错误必须保留正式 provider timeout 证据。
    assert_eq!(policy["unknownOutcome"]["providerErrorCode"], "TIMEOUT");
    // 返回契约一致成功。
    Ok(())
}
