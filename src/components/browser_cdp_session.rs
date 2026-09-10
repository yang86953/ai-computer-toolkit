//! 建立并持有工具自有的固定 CDP 页面 target。

// 导入单调期限。
use std::{
    // 导入私有元素 registry。
    collections::HashMap,
    // 导入等待轮询休眠。
    thread,
    // 导入单调期限。
    time::{Duration, Instant},
};

// 导入 JSON 构造和值类型。
use serde_json::{Value, json};

// 加载动作实现以保持单个代码文件有界。
#[path = "browser_cdp_session_actions.rs"]
mod actions;

// 导入私有传输和统一错误。
use crate::{
    // 导入封闭 CDP 方法与连接。
    components::{
        // 导入封闭 CDP 方法与连接。
        browser_cdp_transport::{CdpCancellationHandle, CdpConnection, CdpMethod},
        // 导入 provider-neutral 查询协议。
        browser_page_protocol::{BrowserElementSelector, BrowserWaitCondition},
    },
    // 导入统一失败类型。
    domain::{AppControlError, AppResult},
};

// 固定 wait 轮询间隔。
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
// 固定单会话私有元素 registry 上限。
const MAXIMUM_ELEMENT_RECORDS: usize = 10_000;

// 保存从 AX tree 提取的 provider-neutral 节点摘要。
struct SemanticNode {
    // 保存原生 backend node ID，仅进入私有 registry。
    backend_node_id: u64,
    // 保存可选语义 role。
    role: Option<String>,
    // 保存可选可访问名称。
    name: Option<String>,
    // 保存可选文本摘要。
    text: Option<String>,
    // 保存是否可用。
    enabled: bool,
}

// 保存已附着且 domains 已启用的私有 CDP 会话。
pub(crate) struct BrowserCdpSession {
    // 保存唯一连接并允许 Drop 显式关闭。
    connection: Option<CdpConnection>,
    // 保存当前导航代际的私有页面引用。
    page_ref: Option<String>,
    // 保存 worker 当前导航代际。
    navigation_generation: u64,
    // 保存当前查询签发的私有元素引用。
    elements: HashMap<String, u64>,
}

// 为私有 CDP 会话提供固定启动流程。
impl BrowserCdpSession {
    // 返回只允许中断在途 CDP I/O 的窄句柄。
    pub(crate) fn cancellation_handle(&self) -> AppResult<CdpCancellationHandle> {
        // 取得仍由会话持有的连接。
        let connection = self.connection.as_ref().ok_or_else(|| {
            // 缺失连接表示生命周期漂移。
            session_error(
                // 使用稳定断开码。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 输出安全诊断。
                "The browser protocol session is no longer connected.",
            )
        })?;
        // 委托传输复制窄取消句柄。
        connection.cancellation_handle()
    }

    // 建立 browser WebSocket、工具 target 与 flatten session。
    pub(crate) fn bootstrap(port: u16, path: &str, deadline: Instant) -> AppResult<Self> {
        // 取得尚未耗尽的总预算。
        let remaining = remaining(deadline)?;
        // 单次连接遵守传输层三十秒上限。
        let connect_timeout = remaining.min(Duration::from_secs(30));
        // 建立严格回环 WebSocket 连接。
        let mut connection = CdpConnection::connect(port, path, connect_timeout)?;
        // 创建唯一空白工具 target。
        let created = connection.call(
            // 使用封闭创建方法。
            CdpMethod::TargetCreateTarget,
            // 不接受调用方 URL 或额外参数。
            json!({ "url": "about:blank" }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 提取并验证私有 target token。
        let target_id = required_token(&created, "targetId")?;
        // 以 flatten 模式附着工具 target。
        let attached = connection.call(
            // 使用封闭附着方法。
            CdpMethod::TargetAttachToTarget,
            // 只传入刚创建的原生 target ID。
            json!({ "targetId": target_id, "flatten": true }),
            // 复用唯一总期限。
            deadline,
        )?;
        // 提取并验证私有 session token。
        let session_id = required_token(&attached, "sessionId")?;
        // 后续页面调用全部绑定 flatten session。
        connection.set_session_id(session_id)?;
        // 启用固定 Page domain。
        connection.call(CdpMethod::PageEnable, json!({}), deadline)?;
        // 启用固定 DOM domain。
        connection.call(CdpMethod::DomEnable, json!({}), deadline)?;
        // 启用固定 Accessibility domain。
        connection.call(CdpMethod::AccessibilityEnable, json!({}), deadline)?;
        // 返回不暴露任何原生身份的所有者。
        Ok(Self {
            // 保存唯一连接。
            connection: Some(connection),
            // target 尚未经过页面导航。
            page_ref: None,
            // 初始代际固定为零。
            navigation_generation: 0,
            // 初始没有已解析元素。
            elements: HashMap::new(),
        })
    }

    // 返回当前导航代际。
    pub(crate) const fn navigation_generation(&self) -> u64 {
        // 复制有界整数。
        self.navigation_generation
    }

    // 在任何 CDP dispatch 前验证导航身份。
    pub(crate) fn validate_navigation(
        &self,
        // 借用调用方观察的可选页面引用。
        page_ref: Option<&str>,
        // 接收调用方观察的导航代际。
        navigation_generation: u64,
    ) -> AppResult<()> {
        // 代际必须逐字匹配 worker 当前事实。
        if navigation_generation != self.navigation_generation {
            // 返回未派发 stale。
            return Err(session_error(
                // 使用页面 stale 类别。
                "STALE_PAGE",
                // 不回显私有引用。
                "The browser page navigation generation is stale.",
            ));
        }
        // 初始导航只允许无页面引用。
        if self.page_ref.is_none() && page_ref.is_some() {
            // 返回未派发 stale。
            return Err(session_error(
                // 使用页面 stale 类别。
                "STALE_PAGE",
                // 不回显私有引用。
                "The browser page reference is stale.",
            ));
        }
        // 后续导航必须绑定当前页面引用。
        if self.page_ref.as_deref().is_some()
            // 核对逐字身份。
            && self.page_ref.as_deref() != page_ref
        {
            // 返回未派发 stale。
            return Err(session_error(
                // 使用页面 stale 类别。
                "STALE_PAGE",
                // 不回显私有引用。
                "The browser page reference is stale.",
            ));
        }
        // 身份门禁通过。
        Ok(())
    }

    // 执行已完成身份门禁的固定页面导航。
    pub(crate) fn navigate(&mut self, url: &str, deadline: Instant) -> AppResult<Value> {
        // 取得仍由会话持有的 CDP 连接。
        let connection = self.connection.as_mut().ok_or_else(|| {
            // 缺失连接表示生命周期漂移。
            session_error(
                // 使用稳定断开码。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 不泄漏连接细节。
                "The browser protocol session is no longer connected.",
            )
        })?;
        // 派发唯一封闭导航方法。
        let result = connection.call(
            // 使用 Page.navigate。
            CdpMethod::PageNavigate,
            // 只传入协议已验证 URL。
            json!({ "url": url }),
            // 复用命令绝对期限。
            deadline,
        )?;
        // Chromium errorText 表示确定导航拒绝。
        if result
            // 读取可选错误文本。
            .get("errorText")
            // 转换为字符串。
            .and_then(Value::as_str)
            // 非空错误才失败。
            .is_some_and(|value| !value.is_empty())
        {
            // 返回不回显 URL 或 provider 文本的确定失败。
            return Err(session_error(
                // 使用稳定导航码。
                "NAVIGATION_FAILED",
                // 输出安全诊断。
                "The isolated browser rejected the page navigation.",
            ));
        }
        // 推进导航代际并拒绝溢出。
        self.navigation_generation =
            self.navigation_generation.checked_add(1).ok_or_else(|| {
                // 代际空间耗尽是确定协议失败。
                session_error(
                    // 使用协议失败码。
                    "BROWSER_PROTOCOL_FAILED",
                    // 输出安全诊断。
                    "The browser page navigation generation was exhausted.",
                )
            })?;
        // 生成新的 worker 私有页面身份。
        let page_ref = format!(
            "w1:bp:{}",
            crate::components::secure_nonce_windows::random_nonce()?
        );
        // 保存当前代际页面引用。
        self.page_ref = Some(page_ref.clone());
        // 导航使旧元素引用全部 stale。
        self.elements.clear();
        // 返回 provider-neutral 成功数据。
        Ok(json!({
            // 标记导航结果种类。
            "kind": "navigate",
            // 返回新的私有页面引用。
            "pageRef": page_ref,
            // 声明导航已经完成。
            "navigated": true,
        }))
    }

    // 在任何页面读取前验证私有页面身份。
    pub(crate) fn validate_page(
        &self,
        // 借用必需页面引用。
        page_ref: Option<&str>,
        // 接收调用方观察代际。
        navigation_generation: u64,
    ) -> AppResult<()> {
        // 页面引用与代际必须同时匹配当前事实。
        if navigation_generation != self.navigation_generation
            // 核对页面引用。
            || self.page_ref.as_deref() != page_ref
        {
            // 返回确定未派发 stale。
            return Err(session_error(
                // 使用稳定页面 stale 码。
                "STALE_PAGE",
                // 不回显私有引用。
                "The browser page reference or navigation generation is stale.",
            ));
        }
        // 页面身份有效。
        Ok(())
    }

    // 等待一个封闭 provider-neutral 页面条件。
    pub(crate) fn wait(
        &mut self,
        // 借用强类型等待条件。
        condition: &BrowserWaitCondition,
        // 接收不重置绝对期限。
        deadline: Instant,
    ) -> AppResult<Value> {
        // 按封闭条件执行。
        match condition {
            // 文档条件轮询固定 readyState。
            BrowserWaitCondition::DocumentReady {} => {
                // 持续到 complete 或总期限耗尽。
                loop {
                    // 执行不接受调用方脚本的固定表达式。
                    let result = self.call(
                        // 使用封闭 Runtime.evaluate 方法。
                        CdpMethod::RuntimeEvaluate,
                        // 表达式由 worker 固定，不接收外部文本。
                        json!({ "expression": "document.readyState", "returnByValue": true }),
                        // 复用唯一总期限。
                        deadline,
                    )?;
                    // 只有逐字 complete 才建立就绪事实。
                    if result.pointer("/result/value").and_then(Value::as_str) == Some("complete") {
                        // 退出等待。
                        break;
                    }
                    // 在剩余预算内短暂休眠。
                    wait_before_retry(deadline)?;
                }
            }
            // 元素条件轮询可访问性树。
            BrowserWaitCondition::ElementPresent { selector } => {
                // 持续到命中或总期限耗尽。
                loop {
                    // 读取当前语义节点。
                    let nodes = self.read_semantic_nodes(deadline)?;
                    // 任一节点匹配即完成。
                    if nodes.iter().any(|node| selector_matches(selector, node)) {
                        // 退出等待。
                        break;
                    }
                    // 在剩余预算内短暂休眠。
                    wait_before_retry(deadline)?;
                }
            }
            // 文本条件轮询 provider-neutral 文本摘要。
            BrowserWaitCondition::TextPresent { text, exact } => {
                // 持续到命中或总期限耗尽。
                loop {
                    // 读取当前语义节点。
                    let nodes = self.read_semantic_nodes(deadline)?;
                    // 任一文本匹配即完成。
                    if nodes.iter().any(|node| {
                        // 节点文本或名称均可建立文本事实。
                        node.text
                            // 借用可选文本。
                            .as_deref()
                            // 执行固定匹配。
                            .is_some_and(|value| text_matches(value, text, *exact))
                            // 名称也属于 provider-neutral 文本。
                            || node
                                // 借用可选名称。
                                .name
                                // 转换为字符串切片。
                                .as_deref()
                                // 执行固定匹配。
                                .is_some_and(|value| text_matches(value, text, *exact))
                    }) {
                        // 退出等待。
                        break;
                    }
                    // 在剩余预算内短暂休眠。
                    wait_before_retry(deadline)?;
                }
            }
        }
        // 返回封闭完成数据。
        Ok(json!({ "kind": "wait", "conditionMet": true }))
    }

    // 查询并签发当前导航代际的私有元素引用。
    pub(crate) fn query(
        &mut self,
        // 借用强类型 selector。
        selector: &BrowserElementSelector,
        // 接收结果上限。
        max_results: u16,
        // 接收不重置绝对期限。
        deadline: Instant,
    ) -> AppResult<Value> {
        // 读取当前完整语义树。
        let nodes = self.read_semantic_nodes(deadline)?;
        // 过滤全部匹配节点。
        let matching = nodes
            // 消费当前快照。
            .into_iter()
            // 只保留 selector 命中。
            .filter(|node| selector_matches(selector, node))
            // 收集有界 fixture 快照。
            .collect::<Vec<_>>();
        // 计算总命中数并限制为协议整数。
        let match_count = u32::try_from(matching.len()).map_err(|_| {
            // 过多节点是资源边界失败。
            session_error(
                // 使用资源码。
                "BROWSER_QUERY_TOO_LARGE",
                // 输出安全诊断。
                "The browser element query exceeded its result boundary.",
            )
        })?;
        // 保存返回结果。
        let mut matches = Vec::new();
        // 只签发调用方请求上限内的元素。
        for node in matching.iter().take(usize::from(max_results)) {
            // 复用同一代际中已签发的私有引用。
            let element_ref = if let Some(existing) = self
                // 遍历受限 registry。
                .elements
                // 取得键值迭代器。
                .iter()
                // 查找同一 backend node。
                .find_map(|(element_ref, backend_node_id)| {
                    // 命中时借用现有引用。
                    (*backend_node_id == node.backend_node_id).then(|| element_ref.clone())
                }) {
                // 返回稳定的私有引用。
                existing
            } else {
                // registry 达到上限时拒绝继续增长。
                if self.elements.len() >= MAXIMUM_ELEMENT_RECORDS {
                    // 返回确定资源失败。
                    return Err(session_error(
                        // 使用稳定资源码。
                        "BROWSER_ELEMENT_REGISTRY_FULL",
                        // 输出安全诊断。
                        "The browser element registry reached its resource boundary.",
                    ));
                }
                // 生成随机私有 element ref。
                let element_ref = format!(
                    // 使用冻结前缀。
                    "w1:be:{}",
                    // 使用系统安全随机源。
                    crate::components::secure_nonce_windows::random_nonce()?
                );
                // 保存私有原生映射。
                self.elements.insert(
                    // 使用随机引用作为键。
                    element_ref.clone(),
                    // 只保存 backend node ID。
                    node.backend_node_id,
                );
                // 返回新引用。
                element_ref
            };
            // 追加 provider-neutral 摘要。
            matches.push(json!({
                // 返回私有元素引用。
                "elementRef": element_ref,
                // 返回可选 role。
                "role": node.role,
                // 返回可选名称。
                "name": node.name,
                // 返回可选文本。
                "text": node.text,
                // 返回 enabled 事实。
                "enabled": node.enabled,
            }));
        }
        // 标记是否截断。
        let truncated = matching.len() > matches.len();
        // 返回封闭查询数据。
        Ok(json!({
            // 标记查询种类。
            "kind": "query",
            // 返回有界命中摘要。
            "matches": matches,
            // 返回完整命中计数。
            "matchCount": match_count,
            // 返回截断事实。
            "truncated": truncated,
        }))
    }

    // 调用当前 flatten session 上的固定 CDP 方法。
    fn call(&mut self, method: CdpMethod, params: Value, deadline: Instant) -> AppResult<Value> {
        // 取得仍由会话持有的连接。
        let connection = self.connection.as_mut().ok_or_else(|| {
            // 缺失连接表示生命周期漂移。
            session_error(
                // 使用稳定断开码。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 输出安全诊断。
                "The browser protocol session is no longer connected.",
            )
        })?;
        // 委托窄传输执行。
        connection.call(method, params, deadline)
    }

    // 读取并净化 Accessibility 完整树。
    fn read_semantic_nodes(&mut self, deadline: Instant) -> AppResult<Vec<SemanticNode>> {
        // 调用封闭语义树方法。
        let result = self.call(CdpMethod::AccessibilityGetFullAxTree, json!({}), deadline)?;
        // 读取 nodes 数组。
        let nodes = result
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                // 响应漂移是协议失败。
                session_error(
                    // 使用稳定协议码。
                    "BROWSER_PROTOCOL_FAILED",
                    // 不回显响应。
                    "The browser accessibility response was invalid.",
                )
            })?;
        // 拒绝异常大的语义树。
        if nodes.len() > 100_000 {
            // 返回资源失败。
            return Err(session_error(
                // 使用查询资源码。
                "BROWSER_QUERY_TOO_LARGE",
                // 输出安全诊断。
                "The browser accessibility tree exceeded its resource boundary.",
            ));
        }
        // 转换可交互 provider-neutral 节点。
        Ok(nodes.iter().filter_map(parse_semantic_node).collect())
    }
}

// 从一个 CDP AXNode 提取窄语义摘要。
fn parse_semantic_node(value: &Value) -> Option<SemanticNode> {
    // ignored 节点不参与语义命中。
    if value.get("ignored").and_then(Value::as_bool) == Some(true) {
        // 丢弃隐藏节点。
        return None;
    }
    // backend node ID 是后续动作唯一私有定位事实。
    let backend_node_id = value.get("backendDOMNodeId").and_then(Value::as_u64)?;
    // 提取有界 role。
    let role = ax_text(value, "role");
    // 提取有界名称。
    let name = ax_text(value, "name");
    // 当前 CDP AXTree 没有独立可见文本时复用名称摘要。
    let text = name.clone();
    // disabled=true 表示不可用。
    let disabled = value
        // 读取属性数组。
        .get("properties")
        // 转换为数组。
        .and_then(Value::as_array)
        // 在属性中查找 disabled。
        .and_then(|properties| {
            // 遍历属性。
            properties.iter().find(|property| {
                // 核对属性名。
                property.get("name").and_then(Value::as_str) == Some("disabled")
            })
        })
        // 读取 typed value 内布尔值。
        .and_then(|property| property.pointer("/value/value"))
        // 转换布尔。
        .and_then(Value::as_bool)
        // 缺失 disabled 默认可用。
        .unwrap_or(false);
    // 返回净化摘要。
    Some(SemanticNode {
        // 保存私有 node ID。
        backend_node_id,
        // 保存 role。
        role,
        // 保存名称。
        name,
        // 保存文本。
        text,
        // 反转 disabled。
        enabled: !disabled,
    })
}

// 从 AXValue 读取有界文本。
fn ax_text(value: &Value, field: &str) -> Option<String> {
    // 读取字段内 typed value。
    value
        // 读取固定字段。
        .get(field)
        // 读取 value 子字段。
        .and_then(|entry| entry.get("value"))
        // 转换为字符串。
        .and_then(Value::as_str)
        // 拒绝空值和超长文本。
        .filter(|text| !text.is_empty() && text.len() <= 512)
        // 取得所有权。
        .map(str::to_owned)
}

// 判断 selector 是否命中语义节点。
fn selector_matches(selector: &BrowserElementSelector, node: &SemanticNode) -> bool {
    // 所有已提供字段必须同时匹配。
    selector
        // 读取可选 role。
        .role()
        // 缺失 selector 字段视为通过。
        .is_none_or(|expected| {
            // 节点必须有 role 且匹配。
            node.role
                // 借用 role。
                .as_deref()
                // 应用固定文本匹配。
                .is_some_and(|actual| text_matches(actual, expected, selector.exact()))
        })
        // 名称条件必须同时成立。
        && selector.name().is_none_or(|expected| {
            // 节点名称必须匹配。
            node.name
                // 借用名称。
                .as_deref()
                // 应用固定文本匹配。
                .is_some_and(|actual| text_matches(actual, expected, selector.exact()))
        })
        // 文本条件必须同时成立。
        && selector.text().is_none_or(|expected| {
            // 节点文本必须匹配。
            node.text
                // 借用文本。
                .as_deref()
                // 应用固定文本匹配。
                .is_some_and(|actual| text_matches(actual, expected, selector.exact()))
        })
}

// 执行逐字或大小写不敏感包含匹配。
fn text_matches(actual: &str, expected: &str, exact: bool) -> bool {
    // exact 使用逐字相等。
    if exact {
        // 返回逐字结果。
        return actual == expected;
    }
    // 非 exact 使用 Unicode 小写后的包含关系。
    actual.to_lowercase().contains(&expected.to_lowercase())
}

// 在总期限内等待下一轮语义读取。
fn wait_before_retry(deadline: Instant) -> AppResult<()> {
    // 读取当前单调时刻。
    let now = Instant::now();
    // 期限耗尽时返回稳定错误。
    if now >= deadline {
        // 返回总 deadline。
        return Err(session_error(
            // 使用稳定超时码。
            "DEADLINE_EXCEEDED",
            // 输出安全诊断。
            "The browser page wait exceeded its deadline.",
        ));
    }
    // 休眠不超过剩余期限。
    thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    // 返回继续事实。
    Ok(())
}

// 在所有 worker 退出路径关闭 WebSocket。
impl Drop for BrowserCdpSession {
    // 消费内部连接并发送 close。
    fn drop(&mut self) {
        // 只关闭仍由本会话持有的连接。
        if let Some(connection) = self.connection.take() {
            // 尽力发送标准关闭并关闭 socket。
            connection.close();
        }
    }
}

// 返回总期限内的剩余预算。
fn remaining(deadline: Instant) -> AppResult<Duration> {
    // 读取单调当前时刻。
    let now = Instant::now();
    // 已耗尽期限时拒绝新 dispatch。
    if now >= deadline {
        // 返回稳定 deadline 失败。
        return Err(session_error(
            // 使用统一超时码。
            "DEADLINE_EXCEEDED",
            // 不泄漏原生端点。
            "The browser protocol session exceeded its opening deadline.",
        ));
    }
    // 返回不重置的剩余时长。
    Ok(deadline.saturating_duration_since(now))
}

// 从固定 CDP result 读取安全原生 token。
fn required_token(result: &Value, field: &str) -> AppResult<String> {
    // 只接受有界 ASCII token。
    let Some(token) = result
        // 读取固定字段。
        .get(field)
        // 转换为字符串。
        .and_then(Value::as_str)
        // 验证非空长度。
        .filter(|value| !value.is_empty() && value.len() <= 256)
        // 验证安全字符集。
        .filter(|value| {
            // 遍历原生 token 字节。
            value
                // 取得字节迭代器。
                .bytes()
                // 只允许 Chromium 身份的安全子集。
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
    else {
        // 响应漂移时返回固定协议失败。
        return Err(session_error(
            // 使用稳定协议码。
            "BROWSER_PROTOCOL_FAILED",
            // 不回显响应内容或字段值。
            "The browser protocol target response was invalid.",
        ));
    };
    // 返回私有所有权副本。
    Ok(token.to_owned())
}

// 构造私有会话的统一错误。
fn session_error(code: &'static str, message: &'static str) -> AppControlError {
    // 使用产品级错误 envelope。
    AppControlError::new(code, message)
}
