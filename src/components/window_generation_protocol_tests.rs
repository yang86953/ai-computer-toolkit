//! 验证窗口代际 broker 协议的严格字段、边界与隐私。

// 导入被测协议类型与常量。
use super::{
    // 导入固定协议元数据。
    BROKER_BUILD_ID,
    // 导入 epoch 与请求 parser。
    BrokerEpoch,
    CONTRACT_VERSION,
    WindowGenerationBrokerOperation,
    WindowGenerationBrokerRequest,
    // 导入稳定失败分类。
    WindowGenerationProtocolErrorCode,
};

// 固定合法请求 nonce。
const NONCE: &str = "00112233445566778899aabbccddeeff";
// 固定合法 broker epoch。
const EPOCH: &str = "ffeeddccbbaa99887766554433221100";

// 构造合法 resolve-snapshot 请求。
fn valid_resolve() -> String {
    // 返回严格字段对象。
    format!(
        // windows 只包含 provider 私有十六进制事实。
        "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"resolve-snapshot\",\"windows\":[{{\"processId\":42,\"currentWindowToken\":\"0000000000001234\",\"processGeneration\":\"0000000000005678\"}}]}}"
    )
}

// 验证 health 与 resolve 两种请求形状均严格可解析。
#[test]
fn parses_closed_health_and_resolve_requests() {
    // 解析不携带 windows 的 health。
    let health = WindowGenerationBrokerRequest::parse(&format!(
        // 使用完整固定 envelope。
        "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"health\"}}"
    ))
    // 合法 health 必须成功。
    .unwrap_or_else(|error| panic!("health must parse: {error:?}"));
    // 核对封闭操作。
    assert_eq!(health.operation(), WindowGenerationBrokerOperation::Health);
    // health 不产生候选。
    assert!(health.windows().is_empty());
    // 核对 nonce。
    assert_eq!(health.request_nonce(), NONCE);
    // 核对 epoch。
    assert_eq!(health.expected_broker_epoch().as_str(), EPOCH);
    // 解析 resolve。
    let resolve = WindowGenerationBrokerRequest::parse(&valid_resolve())
        // 合法快照必须成功。
        .unwrap_or_else(|error| panic!("resolve must parse: {error:?}"));
    // 核对操作。
    assert_eq!(
        resolve.operation(),
        // 固定 resolve-snapshot。
        WindowGenerationBrokerOperation::ResolveSnapshot
    );
    // 只包含一条事实。
    assert_eq!(resolve.windows().len(), 1);
    // 读取候选。
    let candidate = resolve.windows()[0];
    // 核对 PID。
    assert_eq!(candidate.process_id(), 42);
    // 核对 token。
    assert_eq!(candidate.current_window_token(), 0x1234);
    // 核对进程代际。
    assert_eq!(candidate.process_generation(), 0x5678);
}

// 验证未知字段、null、缺失数组、重复 token 与非 canonical 文本均拒绝。
#[test]
fn rejects_ambiguous_or_extensible_frames() {
    // 建立非法请求集合。
    let invalid = [
        // health 不接受 windows。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"health\",\"windows\":[]}}"
        ),
        // resolve 必须显式携带 windows。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"resolve-snapshot\"}}"
        ),
        // null 不冒充字段缺失。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"resolve-snapshot\",\"windows\":null}}"
        ),
        // 未知 root 字段必须拒绝。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"health\",\"fallback\":true}}"
        ),
        // 大写 epoch 不是 canonical。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"FFEEDDCCBBAA99887766554433221100\",\"operation\":\"health\"}}"
        ),
        // 重复 token 使快照歧义。
        format!(
            "{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"requestNonce\":\"{NONCE}\",\"expectedBrokerEpoch\":\"{EPOCH}\",\"operation\":\"resolve-snapshot\",\"windows\":[{{\"processId\":42,\"currentWindowToken\":\"0000000000001234\",\"processGeneration\":\"0000000000005678\"}},{{\"processId\":43,\"currentWindowToken\":\"0000000000001234\",\"processGeneration\":\"0000000000005679\"}}]}}"
        ),
    ];
    // 逐条验证失败闭合。
    for frame in invalid {
        // 解析必须返回同一安全分类。
        let failure = WindowGenerationBrokerRequest::parse(&frame)
            // 任一接受都表示边界漂移。
            .expect_err("invalid frame must fail");
        // 不细分或回显输入内容。
        assert_eq!(
            failure.code(),
            WindowGenerationProtocolErrorCode::InvalidArgument
        );
    }
}

// 验证稳定元数据与错误文本不含 native target 值。
#[test]
fn metadata_and_failures_are_stable_and_private() {
    // broker build ID 不得包含路径分隔符或用户名。
    assert_eq!(
        BROKER_BUILD_ID,
        "ai-computer-toolkit/0.0.1/window-generation-broker/v1"
    );
    // epoch parser 只接受 canonical 文本。
    assert_eq!(
        BrokerEpoch::parse(EPOCH)
            // 固定合法值必须存在。
            .unwrap_or_else(|| panic!("epoch must parse"))
            // 借用 canonical 文本。
            .as_str(),
        // 与输入逐字一致。
        EPOCH
    );
    // 枚举全部稳定失败。
    for code in [
        // 参数失败。
        WindowGenerationProtocolErrorCode::InvalidArgument,
        // epoch 失败。
        WindowGenerationProtocolErrorCode::StaleBrokerEpoch,
        // owner 失败。
        WindowGenerationProtocolErrorCode::OwnerUnavailable,
        // 容量失败。
        WindowGenerationProtocolErrorCode::OwnerCapacityExhausted,
    ] {
        // 组合公开 wire 文本供隐私扫描。
        let text = format!("{} {}", code.as_str(), code.message()).to_ascii_lowercase();
        // 禁止 HWND 类型名。
        assert!(!text.contains("hwnd"));
        // 禁止 pipe 名称。
        assert!(!text.contains("pipe"));
        // 禁止测试 token。
        assert!(!text.contains("1234"));
    }
}
