//! 验证页面动作 Broker final 与原 request 的逐字段绑定。

// 导入标准 Base64 编码引擎。
use base64::Engine;
// 导入 JSON 构造宏。
use serde_json::json;

// 导入稳定字节摘要 Component。
use crate::components::byte_digest;
// 导入严格 request frame builder。
use super::super::wire::BrowserSessionBrokerRequestFrame;
// 导入 broker epoch。
use super::super::state::BrowserSessionBrokerEpoch;
// 导入封闭响应类型。
use super::{
    BrowserSessionBrokerOutcome, BrowserSessionBrokerResponse, BrowserSessionBrokerSuccess,
};

// 固定 canonical request nonce。
const NONCE: &str = "11111111111111111111111111111111";
// 固定 canonical broker epoch。
const EPOCH: &str = "22222222222222222222222222222222";
// 固定 canonical session identity。
const SESSION: &str = "s2:bs:33333333333333333333333333333333";
// 固定 canonical page identity。
const PAGE: &str = "s2:bp:44444444444444444444444444444444";
// 固定 canonical element identity。
const ELEMENT: &str = "s2:be:55555555555555555555555555555555";

// 构造 canonical broker epoch。
fn epoch() -> BrowserSessionBrokerEpoch {
    // 测试常量必须始终满足 epoch 形状。
    BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch should be canonical")
}

// 验证 click/type success 绑定原 page/element 且 type 不回显文本。
#[test]
fn click_and_type_success_are_request_bound_without_text_echo() {
    // 构造 confirmed-only click request。
    let click = BrowserSessionBrokerRequestFrame::click_confirmed(
        // 绑定 canonical nonce。
        NONCE,   // 绑定 canonical epoch。
        EPOCH,   // 使用合法剩余预算。
        5_000,   // 绑定公开 session。
        SESSION, // 绑定当前 page。
        PAGE,    // 绑定当前 element。
        ELEMENT,
    )
    // 同源 builder 必须成功。
    .expect("click request should be canonical");
    // 正确 target 与代际必须形成可信 completed final。
    let click_response = BrowserSessionBrokerResponse::finished(
        // 绑定原 click request。
        click.request(),
        // accepted 后 final 固定 revision 一。
        1,
        // 绑定 current epoch。
        &epoch(),
        // 使用 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 提供 request-bound success。
        Some(BrowserSessionBrokerSuccess::Click {
            // 回显原 page。
            page_id: PAGE.to_owned(),
            // 回显原 element。
            element_id: ELEMENT.to_owned(),
            // 使用正导航代际。
            generation: 7,
        }),
    );
    // 正确 success 必须被接受。
    assert!(click_response.is_some());
    // 跨 page click success 必须失败闭合。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定同一个 click request。
            click.request(),
            // 使用 final revision。
            1,
            // 绑定 current epoch。
            &epoch(),
            // 伪造 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 提供错误 page 的 success。
            Some(BrowserSessionBrokerSuccess::Click {
                // 使用另一个 canonical page。
                page_id: "s2:bp:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                // 保留原 element。
                element_id: ELEMENT.to_owned(),
                // 使用正代际也不能绕过 target 绑定。
                generation: 7,
            }),
        )
        // request-bound validator 必须拒绝。
        .is_none()
    );
    // 保存不得进入 response 的敏感测试文本。
    let secret = "credential-like-fixture";
    // 构造 confirmed-only type request。
    let type_text = BrowserSessionBrokerRequestFrame::type_confirmed(
        // 使用独立 nonce。
        "66666666666666666666666666666666",
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 绑定当前 page。
        PAGE,
        // 绑定当前 element。
        ELEMENT,
        // 绑定完整文本。
        secret,
        // 使用替换语义。
        true,
    )
    // 同源 builder 必须成功。
    .expect("type request should be canonical");
    // 构造 request-bound type completed final。
    let type_response = BrowserSessionBrokerResponse::finished(
        // 绑定原 type request。
        type_text.request(),
        // accepted 后 final 固定 revision 一。
        1,
        // 绑定 current epoch。
        &epoch(),
        // 使用 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 只保存 identity、代际与字节数。
        Some(BrowserSessionBrokerSuccess::Type {
            // 回显原 page。
            page_id: PAGE.to_owned(),
            // 回显原 element。
            element_id: ELEMENT.to_owned(),
            // 使用正导航代际。
            generation: 7,
            // 字节数必须逐字匹配原文本。
            utf8_bytes: u16::try_from(secret.len()).expect("fixture text should fit"),
        }),
    )
    // 正确 success 必须被接受。
    .expect("type success should match request");
    // 编码生产 final wire。
    let encoded = super::super::wire::encode_response(
        // 借用可信 completed response。
        &type_response,
        // 绑定原 type request。
        Some(type_text.request()),
        // 绑定 current epoch。
        &epoch(),
    )
    // encoder 必须成功。
    .expect("type response should encode");
    // response 不得回显调用方文本。
    assert!(!encoded.contains(secret));
    // response 必须保留安全字节数事实。
    assert!(encoded.contains("utf8Bytes"));
}

// 验证 screenshot success 的 page、代际、字节数、IHDR 和摘要一致性。
#[test]
fn screenshot_success_validates_request_bound_png_facts() {
    // 构造不含确认的 screenshot request。
    let screenshot = BrowserSessionBrokerRequestFrame::screenshot(
        // 绑定 canonical nonce。
        NONCE,   // 绑定 canonical epoch。
        EPOCH,   // 使用合法剩余预算。
        5_000,   // 绑定公开 session。
        SESSION, // 绑定当前 page。
        PAGE,
    )
    // 同源 builder 必须成功。
    .expect("screenshot request should be canonical");
    // 构造最小受验证 PNG signature 与 IHDR 头。
    let png = [
        // PNG signature。
        137, 80, 78, 71, 13, 10, 26, 10, // IHDR 长度 13。
        0, 0, 0, 13, // IHDR 类型。
        73, 72, 68, 82, // 宽度 2。
        0, 0, 0, 2, // 高度 3。
        0, 0, 0, 3,
    ];
    // 编码标准 Base64。
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    // 计算同源稳定摘要。
    let digest = byte_digest::digest(&png);
    // 构造完整 request-bound screenshot data。
    let data = json!({
        // 回显原 page。
        "pageId": PAGE,
        // 使用正导航代际。
        "navigationGeneration": 7,
        // MIME 固定 PNG。
        "mimeType": "image/png",
        // 保存标准 Base64。
        "pngBase64": encoded,
        // 保存真实字节数。
        "pngBytes": png.len(),
        // 回显 IHDR 宽度。
        "width": 2,
        // 回显 IHDR 高度。
        "height": 3,
        // 回显同源摘要。
        "digest": digest
    });
    // 全部事实一致时必须形成可信 completed final。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定原 screenshot request。
            screenshot.request(),
            // accepted 后 final 固定 revision 一。
            1,
            // 绑定 current epoch。
            &epoch(),
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 提供封闭 PNG success。
            Some(BrowserSessionBrokerSuccess::Screenshot(data.clone())),
        )
        // 正确 success 必须被接受。
        .is_some()
    );
    // 复制数据以伪造字节数。
    let mut wrong_bytes = data;
    // 修改唯一字节数字段。
    wrong_bytes["pngBytes"] = json!(23);
    // 不一致字节数必须失败闭合。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定原 screenshot request。
            screenshot.request(),
            // accepted 后 final 固定 revision 一。
            1,
            // 绑定 current epoch。
            &epoch(),
            // 伪造 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 提供不一致 PNG success。
            Some(BrowserSessionBrokerSuccess::Screenshot(wrong_bytes)),
        )
        // validator 必须拒绝。
        .is_none()
    );
}
