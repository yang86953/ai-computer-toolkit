//! 编解码 browser-session broker v1 的 operation-specific success data。

// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 导入父 wire Component 的严格 JSON helper。
use super::super::{exact_keys, invalid, require_bool, required_string, required_u64};
// 导入协议 request、operation 与失败类型。
use super::super::super::{
    // 导入封闭 operation 与协议失败。
    BrowserSessionBrokerOperation,
    BrowserSessionBrokerProtocolFailure,
    // 导入严格 request 投影。
    BrowserSessionBrokerRequest,
};
// 导入封闭成功投影。
use super::super::super::response::BrowserSessionBrokerSuccess;

// 将 success 投影编码为 operation-specific data。
pub(super) fn success_value(
    // 借用可选成功投影。
    success: Option<&BrowserSessionBrokerSuccess>,
) -> Result<Value, BrowserSessionBrokerProtocolFailure> {
    // 每个变体编码唯一 data 对象。
    match success {
        // 无成功数据编码为 null。
        None => Ok(Value::Null),
        // open 只回显 session ID。
        Some(BrowserSessionBrokerSuccess::Open { session_id }) => {
            // 构造精确 open data。
            Ok(json!({ "sessionId": session_id }))
        }
        // close 只回显 closed=true。
        Some(BrowserSessionBrokerSuccess::Close) => Ok(json!({ "closed": true })),
        // session.inspect 回显已校验的 session 与 live=true。
        Some(BrowserSessionBrokerSuccess::SessionInspect { session_id, live }) => {
            // 构造精确 session.inspect data。
            Ok(json!({ "sessionId": session_id, "live": live }))
        }
        // navigate 回显 page 与代际。
        Some(BrowserSessionBrokerSuccess::Navigate {
            page_id,
            generation,
        }) => {
            // 构造精确 navigate data。
            Ok(json!({ "pageId": page_id, "navigationGeneration": generation }))
        }
        // wait 回显当前页面、代际与满足事实。
        Some(BrowserSessionBrokerSuccess::Wait {
            // 借用当前公开页面 identity。
            page_id,
            // 借用正导航代际。
            generation,
        }) => Ok(json!({
            // 编码当前公开页面 identity。
            "pageId": page_id,
            // 编码正导航代际。
            "navigationGeneration": generation,
            // 固定条件满足事实。
            "conditionMet": true
        })),
        // query Value 已由 response request-bound validator 验证。
        Some(BrowserSessionBrokerSuccess::Query(value)) => Ok(value.clone()),
        // click 回显 request-bound identity、代际与点击事实。
        Some(BrowserSessionBrokerSuccess::Click {
            page_id,
            element_id,
            generation,
        }) => Ok(json!({
            // 回显当前公开页面。
            "pageId": page_id,
            // 回显当前公开元素。
            "elementId": element_id,
            // 回显正导航代际。
            "navigationGeneration": generation,
            // 固定点击完成事实。
            "clicked": true
        })),
        // type 回显 request-bound identity、代际与字节计数。
        Some(BrowserSessionBrokerSuccess::Type {
            page_id,
            element_id,
            generation,
            utf8_bytes,
        }) => {
            // 构造不含原始文本的精确 type data。
            Ok(json!({
                // 回显当前公开页面。
                "pageId": page_id,
                // 回显当前公开元素。
                "elementId": element_id,
                // 回显正导航代际。
                "navigationGeneration": generation,
                // 固定输入完成事实。
                "typed": true,
                // 只回显调用方已知文本的 UTF-8 字节数。
                "utf8Bytes": utf8_bytes
            }))
        }
        // screenshot Value 已由 PNG validator 验证。
        Some(BrowserSessionBrokerSuccess::Screenshot(value)) => Ok(value.clone()),
    }
}

// 严格解析 operation-specific success data。
pub(super) fn parse_success(
    // 绑定原始 request operation。
    request: &BrowserSessionBrokerRequest,
    // 借用不可信 data。
    data: &Value,
) -> Result<BrowserSessionBrokerSuccess, BrowserSessionBrokerProtocolFailure> {
    // data 必须是对象。
    let object = data.as_object().ok_or_else(invalid)?;
    // 按 request operation 选择唯一 schema。
    match request.operation() {
        // open 只允许 sessionId。
        BrowserSessionBrokerOperation::Open => {
            // 固定唯一键。
            exact_keys(object, &["sessionId"])?;
            // 构造后由 response validator 校验 opaque ID。
            Ok(BrowserSessionBrokerSuccess::Open {
                // 复制公开 session ID。
                session_id: required_string(object, "sessionId")?.to_owned(),
            })
        }
        // close 只允许 closed=true。
        BrowserSessionBrokerOperation::Close => {
            // 固定唯一键。
            exact_keys(object, &["closed"])?;
            // 固定关闭事实。
            require_bool(object, "closed", true)?;
            // 返回无 payload 变体。
            Ok(BrowserSessionBrokerSuccess::Close)
        }
        // session.inspect 只允许原 sessionId 与 live=true。
        BrowserSessionBrokerOperation::SessionInspect => {
            // 固定唯一字段集合。
            exact_keys(object, &["sessionId", "live"])?;
            // 存活成功事实必须为 true。
            require_bool(object, "live", true)?;
            // 构造后由 request-bound validator 核对原 session identity。
            Ok(BrowserSessionBrokerSuccess::SessionInspect {
                // 复制公开 session identity。
                session_id: required_string(object, "sessionId")?.to_owned(),
                // 成功数据只表示 live=true。
                live: true,
            })
        }
        // navigate 只允许 pageId 与正代际。
        BrowserSessionBrokerOperation::Navigate => {
            // 固定唯一键。
            exact_keys(object, &["pageId", "navigationGeneration"])?;
            // 代际必须可无损收窄为 u32 且从一开始。
            let generation = u32::try_from(required_u64(object, "navigationGeneration")?)
                // 拒绝超出 u32。
                .map_err(|_| invalid())?;
            // 零代际不属于冻结契约。
            if generation == 0 {
                // 拒绝零代际。
                return Err(invalid());
            }
            // 构造后由 response validator 校验 page ID。
            Ok(BrowserSessionBrokerSuccess::Navigate {
                // 复制公开 page ID。
                page_id: required_string(object, "pageId")?.to_owned(),
                // 保存正代际。
                generation,
            })
        }
        // wait 只允许当前 page、正代际与 conditionMet=true。
        BrowserSessionBrokerOperation::Wait => {
            // 固定唯一键。
            exact_keys(object, &["pageId", "navigationGeneration", "conditionMet"])?;
            // 固定满足事实。
            require_bool(object, "conditionMet", true)?;
            // 代际必须可无损收窄为正 u32。
            let generation = u32::try_from(required_u64(object, "navigationGeneration")?)
                // 拒绝超出 u32。
                .map_err(|_| invalid())?;
            // 零代际不属于冻结契约。
            if generation == 0 {
                // 拒绝零代际。
                return Err(invalid());
            }
            // 返回后由 request-bound validator 核对原 page identity。
            Ok(BrowserSessionBrokerSuccess::Wait {
                // 复制公开 page identity。
                page_id: required_string(object, "pageId")?.to_owned(),
                // 保存正代际。
                generation,
            })
        }
        // query 结构由 response request-bound validator 严格校验。
        BrowserSessionBrokerOperation::Query => {
            Ok(BrowserSessionBrokerSuccess::Query(data.clone()))
        }
        // click 只允许 clicked=true。
        BrowserSessionBrokerOperation::Click => {
            // 固定唯一键。
            exact_keys(
                object,
                &["pageId", "elementId", "navigationGeneration", "clicked"],
            )?;
            // 固定点击事实。
            require_bool(object, "clicked", true)?;
            // 读取正导航代际。
            let generation = u32::try_from(required_u64(object, "navigationGeneration")?)
                // 拒绝越界代际。
                .map_err(|_| invalid())?;
            // 正导航代际不得为零。
            if generation == 0 {
                // 拒绝未导航页面。
                return Err(invalid());
            }
            // 返回 request-bound 点击投影。
            Ok(BrowserSessionBrokerSuccess::Click {
                // 保存公开页面。
                page_id: required_string(object, "pageId")?.to_owned(),
                // 保存公开元素。
                element_id: required_string(object, "elementId")?.to_owned(),
                // 保存正导航代际。
                generation,
            })
        }
        // type 只允许 typed=true 与匹配字节数。
        BrowserSessionBrokerOperation::Type => {
            // 固定唯一键。
            exact_keys(
                object,
                &[
                    "pageId",
                    "elementId",
                    "navigationGeneration",
                    "typed",
                    "utf8Bytes",
                ],
            )?;
            // 固定输入事实。
            require_bool(object, "typed", true)?;
            // 读取正导航代际。
            let generation = u32::try_from(required_u64(object, "navigationGeneration")?)
                // 拒绝越界代际。
                .map_err(|_| invalid())?;
            // 正导航代际不得为零。
            if generation == 0 {
                // 拒绝未导航页面。
                return Err(invalid());
            }
            // 字节数必须可无损收窄为 u16。
            let utf8_bytes = u16::try_from(required_u64(object, "utf8Bytes")?)
                // 拒绝超出协议边界。
                .map_err(|_| invalid())?;
            // 返回后由 request-bound validator 对比原 text。
            Ok(BrowserSessionBrokerSuccess::Type {
                // 保存公开页面。
                page_id: required_string(object, "pageId")?.to_owned(),
                // 保存公开元素。
                element_id: required_string(object, "elementId")?.to_owned(),
                // 保存正导航代际。
                generation,
                // 保存字节数但不保留文本。
                utf8_bytes,
            })
        }
        // screenshot 结构由 PNG request-bound validator 严格校验。
        BrowserSessionBrokerOperation::Screenshot => {
            // 保留 JSON 值交由 PNG validator 验证。
            Ok(BrowserSessionBrokerSuccess::Screenshot(data.clone()))
        }
    }
}
