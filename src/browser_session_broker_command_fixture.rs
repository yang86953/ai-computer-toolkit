//! 为集成测试运行严格、无 native 泄漏的 browser-session open/close/inspect fixture。

// 导入唯一 stdout 输出。
use std::io::Write;

// 导入 JSON 输入和值构造类型。
use serde_json::{Value, json};

// 导入固定 broker Adapter、有界 stdin Component 与安全错误投影。
use crate::{
    // 只调用无 path、endpoint、argv 或环境覆盖的固定 transport Adapter。
    adapters::browser_session_broker_windows,
    // 只读取单个有界 JSON stdin 文档。
    components::{
        // 导入有界 stdin 文档读取。
        bounded_json_input,
        // 导入 broker provider-neutral selector 与等待条件。
        browser_session_broker_protocol::{BrowserSemanticSelector, BrowserWaitCondition},
    },
    // 构造并序列化不泄漏内部事实的错误 envelope。
    domain::{AppControlError, AppResult, error_json},
};

// 固定 fixture 唯一接受的 timeout 上限。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
// 固定公开 browser session identity 的前缀。
const SESSION_ID_PREFIX: &str = "s2:bs:";
// 固定公开 browser session identity 的随机后缀长度。
const SESSION_ID_SUFFIX_LENGTH: usize = 32;

// 表示 fixture 唯一允许的两条命令与一条只读查询。
enum FixtureCommand {
    // 表示无额外输入的会话打开。
    Open {
        // 保存已经类型和范围校验的总预算。
        timeout_ms: u32,
    },
    // 表示只绑定公开 opaque session 的会话关闭。
    Close {
        // 保存未暴露 native 路由的公开 session ID。
        session_id: String,
        // 保存已经类型和范围校验的总预算。
        timeout_ms: u32,
    },
    // 表示只绑定公开 opaque session 的只读存活查询。
    Inspect {
        // 保存未暴露 native 路由的公开 session ID。
        session_id: String,
        // 保存已经类型和范围校验的总预算。
        timeout_ms: u32,
    },
    // 表示以固定语义条件执行完整页面读取纵切。
    PageReadRoundtrip {
        // 保存已通过最小 URL 门禁的目标地址。
        url: String,
        // 保存已经类型和范围校验的总预算。
        timeout_ms: u32,
    },
    // 表示以固定文本执行完整页面动作与截图纵切。
    PageActionRoundtrip {
        // 保存已通过最小 URL 门禁的目标地址。
        url: String,
        // 保存已经类型和范围校验的总预算。
        timeout_ms: u32,
    },
}

// 构造 fixture 输入无效时不携带 JSON、路径或解析器细节的安全错误。
fn invalid_input() -> AppControlError {
    // 返回稳定公开参数错误。
    AppControlError::new(
        // 使用稳定参数代码。
        "INVALID_ARGUMENT",
        // 不回显输入字段或文本。
        "The browser session broker fixture input is invalid.",
    )
}

// 读取并校验 fixture 的唯一 timeoutMs 字段。
fn timeout_ms(object: &serde_json::Map<String, Value>) -> AppResult<u32> {
    // 只接受 JSON 无符号整数且拒绝负数、浮点数与字符串。
    let value = object
        // 读取唯一 timeout 字段。
        .get("timeoutMs")
        // 取得 JSON 无符号整数。
        .and_then(Value::as_u64)
        // 确保可无损窄化为协议 u32。
        .and_then(|value| u32::try_from(value).ok())
        // 其余类型或溢出统一视为无效输入。
        .ok_or_else(invalid_input)?;
    // 只允许协议冻结的正值总预算。
    if !(1..=MAXIMUM_TIMEOUT_MS).contains(&value) {
        // 范围越界不得启动或连接 broker。
        return Err(invalid_input());
    }
    // 返回严格范围内的 timeout。
    Ok(value)
}

// 判断 JSON 对象是否仅包含唯一允许的字段集合。
fn exact_keys(object: &serde_json::Map<String, Value>, expected: &[&str]) -> bool {
    // 键数量必须精确匹配。
    object.len() == expected.len()
        // 每个实际键都必须位于冻结集合。
        && object.keys().all(|key| expected.contains(&key.as_str()))
}

// 验证 fixture 只接受当前 live epoch 签发的 canonical opaque session ID 形状。
fn canonical_session_id(value: &str) -> bool {
    // ID 必须拥有固定公开前缀。
    value
        // 取得不携带前缀的随机后缀。
        .strip_prefix(SESSION_ID_PREFIX)
        // 后缀必须是固定长度的小写十六进制。
        .is_some_and(|suffix| {
            // 核对精确 128 位十六进制长度。
            suffix.len() == SESSION_ID_SUFFIX_LENGTH
                // 拒绝大小写漂移、分隔符和非 ASCII 字符。
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

// 验证隐藏 fixture 只接受有界 HTTP(S) URL 文本。
fn fixture_url(value: &str) -> bool {
    // 完整网络边界仍由生产 wire parser 复核。
    !value.is_empty()
        // 保持与 broker URL 上限一致。
        && value.chars().count() <= 8_192
        // 只允许 HTTP(S) scheme。
        && (value.starts_with("http://") || value.starts_with("https://"))
        // 禁止控制字符。
        && !value.chars().any(char::is_control)
        // 禁止空白字符。
        && !value.chars().any(char::is_whitespace)
}

// 从单个 JSON 值解析唯一 fixture command。
fn parse_command(value: Value) -> AppResult<FixtureCommand> {
    // 顶层必须是 JSON 对象。
    let object = value.as_object().ok_or_else(invalid_input)?;
    // operation 必须是 JSON 字符串。
    let operation = object
        // 读取 operation 字段。
        .get("operation")
        // 只接受字符串类型。
        .and_then(Value::as_str)
        // 缺失或错类型不能进入 Adapter。
        .ok_or_else(invalid_input)?;
    // 只允许三条显式 operation 路径。
    match operation {
        // open 不得携带 session、nonce、epoch 或任何额外参数。
        "open" if exact_keys(object, &["operation", "timeoutMs"]) => {
            // 解析严格 timeout。
            let timeout_ms = timeout_ms(object)?;
            // 返回封闭 open 命令。
            Ok(FixtureCommand::Open { timeout_ms })
        }
        // close 必须且仅能携带一个公开 session target。
        "close" if exact_keys(object, &["operation", "sessionId", "timeoutMs"]) => {
            // sessionId 必须是字符串，opaque 形状由 Adapter strict builder 复核。
            let session_id = object
                // 读取唯一 session ID。
                .get("sessionId")
                // 拒绝 null、数字、数组与对象。
                .and_then(Value::as_str)
                // 复制唯一公开 target。
                .map(str::to_owned)
                // 不允许缺席或错类型。
                .ok_or_else(invalid_input)?;
            // session ID 必须在启动 broker 前满足公开 opaque 形状。
            if !canonical_session_id(&session_id) {
                // 不把无效 target 交给 Adapter 触发连接或启动。
                return Err(invalid_input());
            }
            // 解析严格 timeout。
            let timeout_ms = timeout_ms(object)?;
            // 返回封闭 close 命令。
            Ok(FixtureCommand::Close {
                // 保存公开 opaque session。
                session_id,
                // 保存总预算。
                timeout_ms,
            })
        }
        // inspect 必须且仅能携带一个公开 session target 与总预算。
        "inspect" if exact_keys(object, &["operation", "sessionId", "timeoutMs"]) => {
            // sessionId 必须是 canonical opaque browser-session identity。
            let session_id = object
                // 读取唯一公开 target。
                .get("sessionId")
                // 拒绝字符串以外的 JSON 类型。
                .and_then(Value::as_str)
                // 复制供物理 Adapter 查询。
                .map(str::to_owned)
                // 缺席或错类型统一失败闭合。
                .ok_or_else(invalid_input)?;
            // 无效 ID 不得触发 broker 启动或连接。
            if !canonical_session_id(&session_id) {
                // 复用不回显输入的固定参数错误。
                return Err(invalid_input());
            }
            // 解析协议范围内的完整总预算。
            let timeout_ms = timeout_ms(object)?;
            // 返回字段封闭的只读查询。
            Ok(FixtureCommand::Inspect {
                // 保存公开 opaque session。
                session_id,
                // 保存总预算。
                timeout_ms,
            })
        }
        // page-read-roundtrip 只接受 URL 与完整总预算。
        "page-read-roundtrip" if exact_keys(object, &["operation", "url", "timeoutMs"]) => {
            // URL 必须是字符串且不得成为 transport 或 native 选择器。
            let url = object
                // 读取唯一 URL。
                .get("url")
                // 拒绝字符串以外的 JSON 类型。
                .and_then(Value::as_str)
                // 复制供完整纵切使用。
                .map(str::to_owned)
                // 缺失或错类型统一失败闭合。
                .ok_or_else(invalid_input)?;
            // 在启动 broker 前实施最小 HTTP(S) 资源门禁。
            if !fixture_url(&url) {
                // 不把明显无效 URL 交给 Adapter。
                return Err(invalid_input());
            }
            // 解析协议范围内的完整总预算。
            let timeout_ms = timeout_ms(object)?;
            // 返回字段封闭的完整页面读取纵切。
            Ok(FixtureCommand::PageReadRoundtrip {
                // 保存目标 URL。
                url,
                // 保存总预算。
                timeout_ms,
            })
        }
        // page-action-roundtrip 只接受 URL 与完整总预算。
        "page-action-roundtrip" if exact_keys(object, &["operation", "url", "timeoutMs"]) => {
            // URL 必须是字符串且不得成为 transport 或 native 选择器。
            let url = object
                // 读取唯一 URL。
                .get("url")
                // 拒绝字符串以外的 JSON 类型。
                .and_then(Value::as_str)
                // 复制供完整纵切使用。
                .map(str::to_owned)
                // 缺失或错类型统一失败闭合。
                .ok_or_else(invalid_input)?;
            // 在启动 broker 前实施最小 HTTP(S) 资源门禁。
            if !fixture_url(&url) {
                // 不把明显无效 URL 交给 Adapter。
                return Err(invalid_input());
            }
            // 解析协议范围内的完整总预算。
            let timeout_ms = timeout_ms(object)?;
            // 返回字段封闭的完整页面动作纵切。
            Ok(FixtureCommand::PageActionRoundtrip {
                // 保存目标 URL。
                url,
                // 保存总预算。
                timeout_ms,
            })
        }
        // operation、键集合或类型漂移全部失败闭合。
        _ => Err(invalid_input()),
    }
}

// 经生产 Adapter 执行 open、navigate、wait、query 与 close 完整纵切。
fn run_page_read_roundtrip(url: &str, timeout_ms: u32) -> AppResult<Value> {
    // 先经真实 broker 打开隔离会话。
    let session_id = browser_session_broker_windows::open_confirmed(timeout_ms)
        // 只附加固定 open 阶段。
        .map_err(|error| stage_error(error, "open"))?;
    // 在同一 live session 内执行页面读取，失败后仍尝试回收。
    let page_result = (|| {
        // 执行 confirmed navigate 并取得公开 page identity。
        let navigation = browser_session_broker_windows::navigate_confirmed(
            // 绑定当前公开 session。
            &session_id,
            // 传递测试本地 HTTP(S) URL。
            url,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 navigate 阶段。
        .map_err(|error| stage_error(error, "navigate"))?;
        // 构造不含 provider 查询语言的文本等待条件。
        let condition = BrowserWaitCondition::TextPresent {
            // 等待固定测试文本。
            text: "fixture text".to_owned(),
            // 要求逐字匹配。
            exact: true,
        };
        // 经真实 broker 执行无副作用 wait Query。
        browser_session_broker_windows::wait(
            // 绑定当前公开 session。
            &session_id,
            // 绑定导航刚签发的 page。
            navigation.page_id(),
            // 绑定有限文本条件。
            &condition,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 wait 阶段。
        .map_err(|error| stage_error(error, "wait"))?;
        // 构造 provider-neutral 文本 selector。
        let selector = BrowserSemanticSelector::new(
            // 不限制 role。
            None,
            // 不限制名称。
            None,
            // 查询固定测试文本。
            Some("fixture text".to_owned()),
            // 要求逐字匹配。
            true,
        );
        // 经真实 broker 执行无副作用 query Query。
        let query = browser_session_broker_windows::query(
            // 绑定当前公开 session。
            &session_id,
            // 绑定导航刚签发的 page。
            navigation.page_id(),
            // 绑定 provider-neutral selector。
            &selector,
            // 固定最多十个命中。
            10,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 query 阶段。
        .map_err(|error| stage_error(error, "query"))?;
        // 返回不含 URL、worker 或 transport 事实的页面结果。
        Ok(json!({
            // 输出公开 page identity。
            "pageId": navigation.page_id(),
            // 输出正导航代际。
            "navigationGeneration": navigation.generation(),
            // wait 成功固定表示条件已满足。
            "conditionMet": true,
            // 输出 strict decoder 已验证的查询对象。
            "query": query
        }))
    })();
    // 无论页面动作是否成功都尝试经真实 broker 回收 session。
    let close_result = browser_session_broker_windows::close_confirmed(&session_id, timeout_ms);
    // 保留首个页面错误，同时要求成功路径完成回收。
    match page_result {
        // 页面失败时不让清理错误覆盖业务根因。
        Err(error) => {
            // 已尽力执行回收且不公开其私有诊断。
            let _ = close_result;
            // 返回原页面错误。
            Err(error)
        }
        // 页面成功时 close 也必须可信完成。
        Ok(mut value) => {
            // 要求 Module 已完成 worker 整树回收。
            close_result.map_err(|error| stage_error(error, "close"))?;
            // JSON 宏保证结果为对象。
            if let Some(object) = value.as_object_mut() {
                // 追加唯一公开关闭事实。
                object.insert("closed".to_owned(), Value::Bool(true));
            }
            // 返回完整页面读取结果。
            Ok(value)
        }
    }
}

// 经生产 Adapter 执行 open、navigate、query、click、type、screenshot 与 close 完整纵切。
fn run_page_action_roundtrip(url: &str, timeout_ms: u32) -> AppResult<Value> {
    // 先经真实 broker 打开隔离会话。
    let session_id = browser_session_broker_windows::open_confirmed(timeout_ms)
        // 只附加固定 open 阶段。
        .map_err(|error| stage_error(error, "open"))?;
    // 在同一 live session 内执行页面动作，失败后仍尝试回收。
    let page_result = (|| {
        // 执行 confirmed navigate 并取得公开 page identity。
        let navigation = browser_session_broker_windows::navigate_confirmed(
            // 绑定当前公开 session。
            &session_id,
            // 传递测试本地 HTTP(S) URL。
            url,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 navigate 阶段。
        .map_err(|error| stage_error(error, "navigate"))?;
        // 构造 provider-neutral 可访问名称 selector。
        let selector = BrowserSemanticSelector::new(
            // 不限制 role。
            None,
            // 查询 runtime fixture 的固定可访问名称。
            Some("Submit".to_owned()),
            // 不限制可见文本。
            None,
            // 要求逐字匹配。
            true,
        );
        // 经真实 broker 查询当前页面元素。
        let query = browser_session_broker_windows::query(
            // 绑定当前公开 session。
            &session_id,
            // 绑定导航刚签发的 page。
            navigation.page_id(),
            // 绑定 provider-neutral selector。
            &selector,
            // 固定只需要首个命中。
            1,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 query 阶段。
        .map_err(|error| stage_error(error, "query"))?;
        // 读取 strict decoder 验证过的首个公开 element identity。
        let element_id = query
            // 读取首个匹配的 elementId。
            .pointer("/matches/0/elementId")
            // 只接受字符串 identity。
            .and_then(Value::as_str)
            // 复制以结束对 query 的借用。
            .map(str::to_owned)
            // fixture 漂移不得伪造成功。
            .ok_or_else(|| fixture_stage_failure("query"))?;
        // 使用当前 page 与 canonical 未签发 element 验证 accepted 前拒绝。
        let stale_element = browser_session_broker_windows::click_confirmed(
            // 绑定当前公开 session。
            &session_id,
            // 绑定当前公开 page，确保错误优先级到达 element。
            navigation.page_id(),
            // 使用未由 Module 签发的 canonical element。
            "s2:be:00000000000000000000000000000000",
            // 复用协议范围内总预算。
            timeout_ms,
        );
        // stale element 必须在 business acceptance 前结构化拒绝。
        match stale_element {
            // 只接受冻结 STALE_ELEMENT 语义。
            Err(error) if error.code == "STALE_ELEMENT" => {}
            // 其他安全错误保留原错误码供定向诊断。
            Err(error) => return Err(stage_error(error, "stale-element")),
            // 意外成功表示预检纵切漂移。
            Ok(_) => return Err(fixture_stage_failure("stale-element")),
        }
        // 经真实 broker 执行 confirmation-first click。
        let click = browser_session_broker_windows::click_confirmed(
            // 绑定当前公开 session。
            &session_id,
            // 绑定当前公开 page。
            navigation.page_id(),
            // 绑定 query 刚签发的 element。
            &element_id,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 click 阶段。
        .map_err(|error| stage_error(error, "click"))?;
        // 经真实 broker 执行 confirmation-first type。
        let type_action = browser_session_broker_windows::type_confirmed(
            // 绑定当前公开 session。
            &session_id,
            // 绑定当前公开 page。
            navigation.page_id(),
            // 绑定 query 刚签发的 element。
            &element_id,
            // 使用 runtime fixture 唯一接受的固定文本。
            "fixture text",
            // 使用显式替换语义。
            true,
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 type 阶段。
        .map_err(|error| stage_error(error, "type"))?;
        // 经真实 broker 捕获当前页面有界 PNG。
        let screenshot = browser_session_broker_windows::screenshot(
            // 绑定当前公开 session。
            &session_id,
            // 绑定当前公开 page。
            navigation.page_id(),
            // 复用协议范围内总预算。
            timeout_ms,
        )
        // 只附加固定 screenshot 阶段。
        .map_err(|error| stage_error(error, "screenshot"))?;
        // 所有成功投影必须绑定同一当前页面、元素与代际。
        if click.page_id() != navigation.page_id()
            // click 必须回显 query element。
            || click.element_id() != element_id
            // click 必须回显当前代际。
            || click.generation() != navigation.generation()
            // type 必须回显同一 page。
            || type_action.action().page_id() != navigation.page_id()
            // type 必须回显同一 element。
            || type_action.action().element_id() != element_id
            // type 必须回显同一代际。
            || type_action.action().generation() != navigation.generation()
            // screenshot 必须回显同一 page。
            || screenshot.page_id() != navigation.page_id()
            // screenshot 必须回显同一代际。
            || screenshot.generation() != navigation.generation()
        {
            // request binding 漂移不得形成成功输出。
            return Err(fixture_stage_failure("request-binding"));
        }
        // 返回不含 URL、输入原文、worker 或 transport 事实的页面结果。
        Ok(json!({
            // 输出公开 page identity。
            "pageId": navigation.page_id(),
            // 输出公开 element identity。
            "elementId": element_id,
            // 输出正导航代际。
            "navigationGeneration": navigation.generation(),
            // click completed 固定表示点击可信完成。
            "clicked": true,
            // type completed 固定表示输入可信完成。
            "typed": true,
            // 只输出输入字节数，不输出原文。
            "utf8Bytes": type_action.utf8_bytes(),
            // 声明业务前 stale element 拒绝已验证。
            "staleElementRejected": true,
            // 输出 strict decoder 已验证的有界 PNG。
            "screenshot": {
                // MIME 固定为 PNG。
                "mimeType": screenshot.mime_type(),
                // 输出有界 Base64。
                "pngBase64": screenshot.png_base64(),
                // 输出真实 PNG 字节数。
                "pngBytes": screenshot.png_bytes(),
                // 输出 IHDR 宽度。
                "width": screenshot.width(),
                // 输出 IHDR 高度。
                "height": screenshot.height(),
                // 输出稳定摘要。
                "digest": screenshot.digest()
            }
        }))
    })();
    // 无论页面动作是否成功都尝试经真实 broker 回收 session。
    let close_result = browser_session_broker_windows::close_confirmed(&session_id, timeout_ms);
    // 保留首个页面错误，同时要求成功路径完成回收。
    match page_result {
        // 页面失败时不让清理错误覆盖业务根因。
        Err(error) => {
            // 已尽力执行回收且不公开其私有诊断。
            let _ = close_result;
            // 返回原页面错误。
            Err(error)
        }
        // 页面成功时 close 也必须可信完成。
        Ok(mut value) => {
            // 要求 Module 已完成 worker 整树回收。
            close_result.map_err(|error| stage_error(error, "close"))?;
            // JSON 宏保证结果为对象。
            if let Some(object) = value.as_object_mut() {
                // 追加唯一公开关闭事实。
                object.insert("closed".to_owned(), Value::Bool(true));
            }
            // 返回完整页面动作结果。
            Ok(value)
        }
    }
}

// 构造 fixture 内部阶段漂移的安全错误。
fn fixture_stage_failure(stage: &'static str) -> AppControlError {
    // 返回不含 target、文本或 transport 事实的固定错误。
    AppControlError::with_details(
        // 使用稳定内部协议错误码。
        "BROKER_UNAVAILABLE",
        // 使用固定安全说明。
        "The browser session broker fixture could not verify the page action result.",
        // 只附加固定阶段标签。
        json!({ "stage": stage }),
    )
}

// 为页面纵切测试失败附加不含目标或 transport 的固定阶段。
fn stage_error(error: AppControlError, stage: &'static str) -> AppControlError {
    // 保留 Adapter 已封闭的公共生命周期布尔事实。
    let mut details = error.details;
    // 在既有对象上追加固定阶段，不复制私有事实。
    if let Some(object) = details.as_object_mut() {
        // 阶段标签只来自 fixture 封闭常量。
        object.insert("stage".to_owned(), Value::String(stage.to_owned()));
    } else {
        // 没有公共事实时只创建固定阶段对象。
        details = json!({ "stage": stage });
    }
    // 只保留既有稳定错误码、安全消息与固定阶段标签。
    AppControlError::with_details(
        // 保留原稳定错误码。
        error.code,
        // 保留 Adapter 已过滤的安全消息。
        error.message,
        // 保存 Adapter 已安全投影的生命周期事实。
        details,
    )
}

// 写出唯一一行 JSON，忽略测试进程已经关闭 stdout 的 I/O 结果。
fn write_output(value: Value) {
    // 序列化固定成功或 error envelope。
    let text = value.to_string();
    // 只写入单行，不写入诊断侧通道。
    let _ = writeln!(std::io::stdout(), "{text}");
}

// 运行一次固定 open/close command fixture 并写出 provider-neutral 结果。
pub fn run_stdio() -> i32 {
    // 有界读取并严格解析唯一 stdin 文档。
    let command = bounded_json_input::read_stdin()
        // 输入读取、UTF-8 或 JSON 失败不公开来源细节。
        .map_err(|_| invalid_input())
        // 限制为唯一 fixture command。
        .and_then(parse_command);
    // 执行已经通过 exact-key 与类型门禁的命令。
    let result = match command {
        // open 只返回公开 session identity。
        Ok(FixtureCommand::Open { timeout_ms }) => {
            // 调用私有跨 launcher open exchange。
            browser_session_broker_windows::open_confirmed(timeout_ms).map(|session_id| {
                // 构造不含 epoch、nonce、PID 或路径的成功结果。
                json!({ "ok": true, "result": { "sessionId": session_id } })
            })
        }
        // close 只返回公开关闭事实。
        Ok(FixtureCommand::Close {
            // 解构公开 opaque session。
            session_id,
            // 解构已校验预算。
            timeout_ms,
        }) => {
            // 调用私有跨 launcher close exchange。
            browser_session_broker_windows::close_confirmed(&session_id, timeout_ms).map(|()| {
                // 构造不含 native 路由事实的成功结果。
                json!({ "ok": true, "result": { "closed": true } })
            })
        }
        // inspect 只返回当前 target 的 live 事实。
        Ok(FixtureCommand::Inspect {
            // 解构公开 opaque session。
            session_id,
            // 解构已校验预算。
            timeout_ms,
        }) => {
            // 调用固定 broker Adapter 的同 nonce/epoch 可恢复 Query。
            browser_session_broker_windows::inspect_session(&session_id, timeout_ms).map(|()| {
                // 结果只回显公开 session 与存活事实。
                json!({ "ok": true, "result": { "sessionId": session_id, "live": true } })
            })
        }
        // 页面读取纵切只返回公开 page、Query 数据与关闭事实。
        Ok(FixtureCommand::PageReadRoundtrip { url, timeout_ms }) => {
            // 执行真实 broker 的三项页面 operation。
            run_page_read_roundtrip(&url, timeout_ms)
                // 包装为 fixture 唯一成功 envelope。
                .map(|result| json!({ "ok": true, "result": result }))
        }
        // 页面动作纵切只返回公开 target、动作事实、PNG 与关闭事实。
        Ok(FixtureCommand::PageActionRoundtrip { url, timeout_ms }) => {
            // 执行真实 broker 的页面写动作与截图 Query。
            run_page_action_roundtrip(&url, timeout_ms)
                // 包装为 fixture 唯一成功 envelope。
                .map(|result| json!({ "ok": true, "result": result }))
        }
        // 输入失败保持安全 error envelope。
        Err(error) => Err(error),
    };
    // 输出唯一成功或错误 envelope 并返回稳定退出码。
    match result {
        // 成功只写入固定 result 对象。
        Ok(value) => {
            // 输出成功 JSON。
            write_output(value);
            // 返回成功退出码。
            0
        }
        // 失败使用统一安全 error_json。
        Err(error) => {
            // 输出不含 transport 私有信息的错误 JSON。
            write_output(error_json(&error));
            // 返回失败退出码。
            2
        }
    }
}

// 注册 fixture 输入边界的纯解析回归测试。
#[cfg(test)]
// 将测试保留在独立文件，避免运行时 fixture 承担测试细节。
#[path = "browser_session_broker_command_fixture_tests.rs"]
mod tests;
