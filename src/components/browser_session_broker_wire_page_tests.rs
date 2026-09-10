// 导入页面 request builder 与协议类型。
use super::{
    // 导入 provider-neutral selector。
    BrowserSemanticSelector,
    // 导入严格 request frame。
    BrowserSessionBrokerRequestFrame,
    // 导入有限等待条件。
    BrowserWaitCondition,
};
// 导入 parser request 变体。
use super::super::super::BrowserSessionBrokerRequest;

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

// 验证 navigate builder 固化确认并与 parser canonical 指纹同源。
#[test]
fn navigate_builder_round_trips_confirmed_command() {
    // 构造严格导航 frame。
    let frame = BrowserSessionBrokerRequestFrame::navigate_confirmed(
        // 绑定 canonical nonce。
        NONCE,
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 使用合法 HTTPS URL。
        "https://example.test/page",
    )
    // 同源 builder 必须成功。
    .expect("navigate frame should be canonical");
    // parser 投影必须是 navigate 变体。
    assert!(matches!(
        // 借用 frame 对应 request。
        frame.request(),
        // 核对 session 与 URL。
        BrowserSessionBrokerRequest::Navigate(_, session, url)
            if session == SESSION && url == "https://example.test/page"
    ));
    // Command frame 必须显式包含 confirmed=true。
    assert_eq!(
        // 解析 builder 输出供字段断言。
        serde_json::from_str::<serde_json::Value>(frame.text())
            // builder 已保证严格 JSON。
            .expect("navigate frame should be json")["confirmed"],
        // 确认值必须为 true。
        true,
    );
}

// 验证 wait/query builder 不携带确认并保持 Query mutation 真值。
#[test]
fn page_read_builders_round_trip_as_queries() {
    // 构造有界文本等待条件。
    let condition = BrowserWaitCondition::TextPresent {
        // 保存测试文本。
        text: "fixture text".to_owned(),
        // 要求逐字匹配。
        exact: true,
    };
    // 构造严格 wait frame。
    let wait = BrowserSessionBrokerRequestFrame::wait(
        // 绑定 canonical nonce。
        NONCE,   // 绑定 canonical epoch。
        EPOCH,   // 使用合法剩余预算。
        5_000,   // 绑定公开 session。
        SESSION, // 绑定公开 page。
        PAGE,    // 绑定有限条件。
        &condition,
    )
    // 同源 builder 必须成功。
    .expect("wait frame should be canonical");
    // wait 必须保持 Query 角色。
    assert!(!wait.request().may_mutate_target());
    // wait JSON 不得出现 confirmed 字段。
    assert!(
        // 解析严格 JSON。
        serde_json::from_str::<serde_json::Value>(wait.text())
            // builder 已保证 JSON。
            .expect("wait frame should be json")
            // 读取根对象。
            .as_object()
            // 根必须为对象。
            .expect("wait frame should be object")
            // Query 不得携带 confirmed。
            .get("confirmed")
            // 字段必须缺失。
            .is_none()
    );
    // 构造 provider-neutral selector。
    let selector = BrowserSemanticSelector::new(
        // 不限制 role。
        None,
        // 不限制名称。
        None,
        // 按测试文本查询。
        Some("fixture text".to_owned()),
        // 要求逐字匹配。
        true,
    );
    // 构造严格 query frame。
    let query = BrowserSessionBrokerRequestFrame::query(
        // 绑定另一 canonical nonce 避免测试语义混淆。
        "55555555555555555555555555555555",
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 绑定公开 page。
        PAGE,
        // 绑定 selector。
        &selector,
        // 使用有界命中上限。
        10,
    )
    // 同源 builder 必须成功。
    .expect("query frame should be canonical");
    // parser 投影必须是 query 变体且保留上限。
    assert!(matches!(
        // 借用 frame 对应 request。
        query.request(),
        // 核对公开 target 与 maxResults。
        BrowserSessionBrokerRequest::Query(_, session, page, _, 10)
            if session == SESSION && page == PAGE
    ));
    // query 必须保持 Query 角色。
    assert!(!query.request().may_mutate_target());
}

// 验证 click/type/screenshot builder 与 parser 的完整语义逐字同源。
#[test]
fn page_action_builders_round_trip_strict_semantics() {
    // 构造 confirmed-only click frame。
    let click = BrowserSessionBrokerRequestFrame::click_confirmed(
        // 绑定 canonical nonce。
        NONCE,   // 绑定 canonical epoch。
        EPOCH,   // 使用合法剩余预算。
        5_000,   // 绑定公开 session。
        SESSION, // 绑定公开 page。
        PAGE,    // 绑定公开 element。
        ELEMENT,
    )
    // 同源 builder 必须成功。
    .expect("click frame should be canonical");
    // parser 投影必须保留三级公开 target。
    assert!(matches!(
        // 借用 click request。
        click.request(),
        // 核对完整 target。
        BrowserSessionBrokerRequest::Click(_, session, page, element)
            if session == SESSION && page == PAGE && element == ELEMENT
    ));
    // click 必须保持 mutation Command 角色。
    assert!(click.request().may_mutate_target());
    // 构造 confirmed-only type frame。
    let type_text = BrowserSessionBrokerRequestFrame::type_confirmed(
        // 使用独立 canonical nonce。
        "66666666666666666666666666666666",
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 绑定公开 page。
        PAGE,
        // 绑定公开 element。
        ELEMENT,
        // 绑定多字节文本。
        "输入 fixture",
        // 显式使用替换语义。
        true,
    )
    // 同源 builder 必须成功。
    .expect("type frame should be canonical");
    // parser 投影必须逐字保留文本与 replace。
    assert!(matches!(
        // 借用 type request。
        type_text.request(),
        // 核对三级 target、文本和 replace。
        BrowserSessionBrokerRequest::Type(_, session, page, element, text, true)
            if session == SESSION && page == PAGE && element == ELEMENT && text == "输入 fixture"
    ));
    // type 必须保持 mutation Command 角色。
    assert!(type_text.request().may_mutate_target());
    // 构造不含确认的 screenshot Query。
    let screenshot = BrowserSessionBrokerRequestFrame::screenshot(
        // 使用独立 canonical nonce。
        "77777777777777777777777777777777",
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 绑定公开 page。
        PAGE,
    )
    // 同源 builder 必须成功。
    .expect("screenshot frame should be canonical");
    // screenshot 必须保持 Query 角色。
    assert!(!screenshot.request().may_mutate_target());
    // screenshot wire 不得出现确认、路径、格式或 CDP 参数。
    let screenshot_value = serde_json::from_str::<serde_json::Value>(screenshot.text())
        // builder 已保证严格 JSON。
        .expect("screenshot frame should be json");
    // 读取根对象。
    let screenshot_object = screenshot_value
        // 转换对象。
        .as_object()
        // 根必须是对象。
        .expect("screenshot frame should be object");
    // Query 不得携带 confirmed。
    assert!(!screenshot_object.contains_key("confirmed"));
    // Query 不得携带路径或格式扩张。
    assert!(!screenshot_object.contains_key("path") && !screenshot_object.contains_key("format"));
    // Query 不得携带 CDP method。
    assert!(!screenshot_object.contains_key("cdpMethod"));
}

// 验证 navigate completed final 可被同一 request 与 epoch 无损解码。
#[test]
fn navigate_completed_response_round_trips() {
    // 构造严格导航 frame。
    let frame = BrowserSessionBrokerRequestFrame::navigate_confirmed(
        // 绑定 canonical nonce。
        NONCE,
        // 绑定 canonical epoch。
        EPOCH,
        // 使用合法剩余预算。
        5_000,
        // 绑定公开 session。
        SESSION,
        // 使用合法 HTTPS URL。
        "https://example.test/page",
    )
    // 同源 builder 必须成功。
    .expect("navigate frame should be canonical");
    // 构造认证 epoch 投影。
    let epoch = super::super::super::state::BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // canonical epoch 必须成功。
        .expect("epoch should be canonical");
    // 构造 request-bound completed final。
    let response = super::super::super::response::BrowserSessionBrokerResponse::finished(
        // 绑定原严格 request。
        frame.request(),
        // accepted 后 final 固定 revision 一。
        1,
        // 绑定认证 epoch。
        &epoch,
        // 使用 completed outcome。
        super::super::super::response::BrowserSessionBrokerOutcome::Completed,
        // 携带 navigate 专属成功数据。
        Some(
            super::super::super::response::BrowserSessionBrokerSuccess::Navigate {
                // 使用 canonical page identity。
                page_id: PAGE.to_owned(),
                // 使用首个正代际。
                generation: 1,
            },
        ),
    )
    // operation/result 组合必须被接受。
    .expect("navigate completed response should be valid");
    // 编码生产 final wire。
    let text = super::super::encode_response(
        // 借用 completed response。
        &response,
        // 绑定原严格 request。
        Some(frame.request()),
        // 绑定认证 epoch。
        &epoch,
    )
    // 生产 encoder 必须接受。
    .expect("navigate final should encode");
    // 用 Adapter 同源 decoder 解码。
    let decoded = super::super::decode_response(
        // 借用 final wire。
        &text,
        // 绑定原严格 request。
        frame.request(),
        // 绑定认证 epoch。
        &epoch,
    )
    // request-bound decoder 必须接受。
    .expect("navigate final should decode");
    // completed final 必须保留可信完成。
    assert!(decoded.completed());
}
