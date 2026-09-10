#![cfg(target_os = "linux")]

use ai_computer_toolkit::cli;
use serde_json::Value;

#[test]
fn linux_catalog_exposes_only_two_explicit_versioned_termination_operations() {
    let descriptor = ai_computer_toolkit::catalog::app("process")
        .unwrap_or_else(|| panic!("process catalog 必须存在"));
    assert!(
        descriptor
            .public_verbs
            .contains(&ai_computer_toolkit::domain::Verb::Run)
    );
    assert_eq!(descriptor.operations.len(), 2);
    assert_eq!(descriptor.operations[0].id, "process.terminate.graceful@2");
    assert_eq!(descriptor.operations[0].operation, "terminate-graceful");
    assert!(descriptor.operations[0].requires_confirmation);
    assert_eq!(descriptor.operations[1].id, "process.terminate.force@2");
    assert_eq!(descriptor.operations[1].operation, "terminate-force");
    assert!(descriptor.operations[1].requires_confirmation);
}

#[test]
fn cli_confirmation_precedes_input_file_and_target_access() {
    let error = cli::run(vec![
        "run".to_owned(),
        "process".to_owned(),
        "terminate-graceful".to_owned(),
        "--target".to_owned(),
        "sessionId=s2:p:0000000000000000".to_owned(),
        "--input".to_owned(),
        "/path-that-must-not-be-read-before-confirmation".to_owned(),
    ])
    .err()
    .unwrap_or_else(|| panic!("未确认高风险请求必须失败"));
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");

    let force = cli::run(vec![
        "run".to_owned(),
        "process".to_owned(),
        "terminate-force".to_owned(),
        "--target".to_owned(),
        "sessionId=s2:p:0000000000000000".to_owned(),
        "--input".to_owned(),
        "/path-that-must-not-be-read-before-force-confirmation".to_owned(),
    ])
    .err()
    .unwrap_or_else(|| panic!("未确认 critical 请求必须失败"));
    assert_eq!(force.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn linux_security_manifests_match_production_boundaries() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (manifest_text, public_schema) in [
        (
            include_str!("../contracts/linux-process-termination-security-v2.json"),
            include_str!("../../contracts/v2/process-termination-graceful-result.schema.json"),
        ),
        (
            include_str!("../contracts/linux-process-force-termination-security-v2.json"),
            include_str!("../../contracts/v2/process-termination-force-result.schema.json"),
        ),
    ] {
        let manifest: Value = serde_json::from_str(manifest_text)
            .unwrap_or_else(|error| panic!("安全 manifest 应有效：{error}"));
        for assertion in manifest["sourceAssertions"]
            .as_array()
            .unwrap_or_else(|| panic!("sourceAssertions 应为数组"))
        {
            let path = assertion["path"]
                .as_str()
                .unwrap_or_else(|| panic!("source assertion path 应为字符串"));
            let source = std::fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("应能读取 {path}：{error}"));
            for token in assertion["requiredTokens"]
                .as_array()
                .unwrap_or_else(|| panic!("requiredTokens 应为数组"))
            {
                let token = token
                    .as_str()
                    .unwrap_or_else(|| panic!("required token 应为字符串"));
                assert!(source.contains(token), "{path} 缺少安全 token {token}");
            }
        }
        for field in manifest["forbiddenPublicFields"]
            .as_array()
            .unwrap_or_else(|| panic!("forbiddenPublicFields 应为数组"))
        {
            let field = field
                .as_str()
                .unwrap_or_else(|| panic!("forbidden field 应为字符串"));
            assert!(
                !public_schema.contains(field),
                "结果 schema 泄漏字段 {field}"
            );
        }
    }
}
