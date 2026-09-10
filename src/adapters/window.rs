//! 只读顶层窗口发现与精确元数据观察 adapter。

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
// 引入只读 Window Adapter 私有封闭错误码实现。
#[path = "window_error.rs"]
mod error_code;

// 导入只读 Window Adapter 私有封闭错误码。
use error_code::WindowAdapterErrorCode;

// 导入窗口观察所需的 facade、registry、opaque 匹配与领域类型。
use crate::{
    // 导入公共 adapter trait 与 Win32 只读窗口事实。
    adapters::{
        // 导入公共 adapter trait。
        AppAdapter,
        // 导入窗口枚举、前景快照与 canonical 身份生成器。
        windows::{WindowRecord, enumerate_windows, foreground_hwnd, opaque_window_session_id},
    },
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入 fail-closed opaque 目标匹配组件。
    components::{
        opaque_id::{OpaqueTargetMatch, match_opaque_target},
        window_target_identity,
    },
    // 导入统一结果与请求类型。
    domain::{AppResult, CommandRequest},
};

// 声明只读窗口 surface adapter。
pub struct WindowAdapter;

// 限制一次公开快照最多保留的可见有标题窗口数量。
const MAXIMUM_WINDOWS: usize = 4096;

// 列出禁止跨越公开窗口边界的原生目标字段。
const PRIVATE_TARGET_FIELDS: &[&str] = &[
    // 禁止原生窗口句柄。
    "hwnd",
    // 禁止原生进程 ID。
    "processId",
    // 禁止 Win32 窗口类名。
    "className",
    // 禁止原生矩形边界。
    "bounds",
    // 禁止可执行文件路径。
    "path",
    // 禁止 provider 私有身份。
    "providerId",
];

// 从 registry 取得精确 surface 拥有的 capability 定义。
fn registered_capability(
    // 接收能力应归属的公开 surface。
    surface: capabilities::CapabilitySurface,
    // 接收 registry 常量提供的稳定 ID。
    id: &'static str,
) -> &'static capabilities::CapabilityDefinition {
    // surface 错配或漏登记属于不可恢复的内部目录漂移。
    let Some(definition) = capabilities::definition_for_surface(surface, id) else {
        // 禁止用手写回退描述掩盖 registry 漂移。
        unreachable!("window capability projection must use a registered surface definition");
    };
    // 返回只读静态定义供兼容投影使用。
    definition
}

// 构造 C++ window_json 使用的只读 capability 描述。
fn read_capability(
    // 接收能力应归属的公开 surface。
    surface: capabilities::CapabilitySurface,
    // 接收 registry 常量提供的稳定 ID。
    id: &'static str,
    // 保留 window observation 兼容契约拥有的风险文本。
    risk: &'static str,
    // 保留敏感读取是否需要确认的兼容事实。
    requires_confirmation: bool,
    // 保留 window observation 兼容约束文本。
    constraint: &'static str,
) -> Value {
    // 从精确 surface registry 解析稳定 ID 与执行域。
    let definition = registered_capability(surface, id);
    // 返回语言中立且不含原生目标的 capability 对象。
    json!({
        // 从 registry 输出稳定版本化 ID。
        "id": definition.id,
        // 输出兼容层拥有的风险分类。
        "risk": risk,
        // 从 registry 输出强类型执行域。
        "executionDomain": definition.execution_realm,
        // 当前 launcher 可提供该兼容能力。
        "availability": "available",
        // 输出兼容层明确拥有的确认事实。
        "requiresConfirmation": requires_confirmation,
        // 输出公开约束。
        "constraint": constraint,
    })
}

// 构造 window observation 中由 App facade 承接的 capability 描述。
fn app_capability(id: &'static str, risk: &'static str) -> Value {
    // 只允许 App surface 定义进入 generic verb 兼容形状。
    let definition = registered_capability(capabilities::CapabilitySurface::App, id);
    // 从 registry 与兼容层自有 risk 组合稳定公开对象。
    json!({
        // 从 registry 输出稳定版本化 ID。
        "id": definition.id,
        // window observation 当前固定投影主版本一。
        "version": 1,
        // 从强类型 action 输出 generic verb。
        "verb": definition.action.as_str(),
        // 保留兼容层拥有的风险分类。
        "risk": risk,
        // 从 registry 输出强类型执行域。
        "executionDomain": definition.execution_realm,
        // 当前统一 facade 可用。
        "availability": "available",
        // App 变更/敏感制品动作按 action 要求确认。
        "requiresConfirmation": definition.action.mutates(),
        // 从 registry 输出前台许可策略。
        "requiresForegroundConsent": definition.requires_foreground_consent,
        // 从 registry 输出稳定输入 schema。
        "inputSchema": definition.input_schema,
    })
}

// 返回与 C++ 对照实现相同的窗口公开 capability 列表。
fn public_window_capabilities() -> Vec<Value> {
    // 使用固定顺序维持跨语言 golden 稳定。
    vec![
        // 公布运行中应用发现关系。
        read_capability(
            // 核对应用发现 surface 所有权。
            capabilities::CapabilitySurface::ApplicationDiscovery,
            // 从 Rust registry 使用稳定 ID。
            capabilities::APPLICATION_DISCOVER,
            // 保持普通读取风险。
            "read",
            // 普通观察无需确认。
            false,
            // 只覆盖运行中应用。
            "running-applications-only",
        ),
        // 公布本批迁回的窗口发现能力。
        read_capability(
            // 核对窗口 surface 所有权。
            capabilities::CapabilitySurface::Window,
            // 从 Rust registry 读取稳定 ID。
            capabilities::WINDOW_DISCOVER,
            // 保持普通读取风险。
            "read",
            // 普通观察无需确认。
            false,
            // 只发布可见有标题顶层窗口。
            "visible-titled-top-level-windows",
        ),
        // 公布隔离的只读可访问性树能力。
        read_capability(
            // 核对 Accessibility surface 所有权。
            capabilities::CapabilitySurface::Accessibility,
            // 从 Rust registry 使用稳定 ID。
            capabilities::ACCESSIBILITY_TREE_READ,
            // 保持普通读取风险。
            "read",
            // 普通观察无需确认。
            false,
            // 禁止 Value、Text 与 bounds 内容。
            "same-session-bounded-tree-without-value-text-or-bounds",
        ),
        // 公布只返回帧元数据的敏感读取探针。
        read_capability(
            // 核对窗口 surface 所有权。
            capabilities::CapabilitySurface::Window,
            // 从 Rust registry 使用稳定 ID。
            capabilities::WINDOW_CAPTURE_FRAME_PROBE,
            // 捕获元数据保持敏感读取风险。
            "read-sensitive",
            // 敏感读取需要确认。
            true,
            // 明确不读取 surface、不写文件也不激活窗口。
            "frame-metadata-only-no-surface-read-no-file-no-activation",
        ),
        // 公布已由 Rust app facade 承接的窗口截图能力。
        app_capability(
            // 从 registry 使用窗口截图 ID。
            capabilities::WINDOW_SCREENSHOT,
            // 截图保持敏感读取风险。
            "read-sensitive",
        ),
        // 公布已由 Rust app facade 承接的窗口关闭能力。
        app_capability(
            // 从 registry 使用窗口关闭 ID。
            capabilities::WINDOW_CLOSE,
            // 关闭窗口保持 mutation 风险。
            "mutation",
        ),
    ]
}

// 把私有 WindowRecord 投影为 C++ 等价的安全公开对象。
pub(crate) fn public_window_observation(window: &WindowRecord) -> Value {
    // 进程名不可读时保持 C++ 的非空 unavailable 哨兵。
    let application_name = window.process_name.as_deref().unwrap_or("unavailable");
    // 只序列化 provider-neutral 窗口事实。
    json!({
        // 从当前私有事实重新生成 canonical s2:w。
        "sessionId": opaque_window_session_id(window),
        // 保持 C++ golden 的通用类别。
        "kind": "window",
        // 输出稳定目标类别。
        "targetKind": "application-window",
        // 只输出进程文件名或 unavailable 哨兵。
        "applicationName": application_name,
        // 输出当前非空窗口标题。
        "title": window.title,
        // 本 surface 只会投影可见窗口。
        "visible": true,
        // 公开当前 opaque 窗口目标可证明的身份强度。
        "targetIdentityStrength": window_target_identity::public_assurance(),
        // 输出安全公开 capability 列表。
        "capabilities": public_window_capabilities(),
    })
}

// 拒绝原生窗口选择器跨越公开 JSON 边界。
fn reject_private_target_fields(request: &CommandRequest) -> AppResult<()> {
    // 查找第一个被禁止的字段。
    let private_field = PRIVATE_TARGET_FIELDS
        // 遍历稳定禁止列表。
        .iter()
        // 只保留调用方实际提供的字段。
        .find(|field| request.target.contains_key(**field));
    // 没有原生字段时允许继续。
    let Some(private_field) = private_field else {
        // 返回成功。
        return Ok(());
    };
    // 返回稳定参数错误并只公开字段名。
    Err(WindowAdapterErrorCode::InvalidArgument.with_details(
        // 指引调用方使用 opaque 目标。
        "Native window selectors are private; use title, applicationName, or an opaque sessionId.",
        // 只回显安全字段名，不回显原生值。
        json!({ "field": private_field }),
    ))
}

// 捕获本 capability 允许发布的可见有标题顶层窗口。
pub(crate) fn capture_visible_titled_windows() -> AppResult<Vec<WindowRecord>> {
    // 枚举当前顶层窗口快照。
    let mut windows = enumerate_windows()?;
    // 只保留 C++ 契约允许的事实。
    windows.retain(|window| window.visible && !window.title.is_empty());
    // 与 C++ backend 的 4096 项硬边界一致。
    windows.truncate(MAXIMUM_WINDOWS);
    // 返回私有快照供过滤或重新解析。
    Ok(windows)
}

// 在安全公开字段上应用兼容过滤。
fn filter_public_windows(
    // 接收当前私有窗口快照。
    windows: &mut Vec<WindowRecord>,
    // 接收只读请求。
    request: &CommandRequest,
) {
    // 读取可选精确标题。
    let title = request.target.get("title").and_then(Value::as_str);
    // 读取 applicationName 或 legacy name 别名。
    let application_name = request
        // 优先使用契约字段。
        .target
        // 读取 applicationName。
        .get("applicationName")
        // 要求字符串。
        .and_then(Value::as_str)
        // 缺失时接受 legacy name。
        .or_else(|| request.target.get("name").and_then(Value::as_str));
    // 读取可选 opaque sessionId 过滤。
    let session_id = request.target.get("sessionId").and_then(Value::as_str);
    // 就地保留全部安全过滤条件匹配的窗口。
    windows.retain(|window| {
        // 标题必须精确匹配。
        let title_matches = title.is_none_or(|expected| window.title == expected);
        // 应用名按 Windows 文件名语义不区分 ASCII 大小写。
        let application_matches = application_name.is_none_or(|expected| {
            // 使用 unavailable 哨兵参与安全比较。
            window
                // 读取可用进程名。
                .process_name
                // 转为借用字符串。
                .as_deref()
                // 缺失时使用非空哨兵。
                .unwrap_or("unavailable")
                // 执行不区分大小写比较。
                .eq_ignore_ascii_case(expected)
        });
        // opaque 目标必须从当前私有事实重新生成后匹配。
        let session_matches =
            session_id.is_none_or(|expected| opaque_window_session_id(window) == expected);
        // 全部安全条件同时满足才保留。
        title_matches && application_matches && session_matches
    });
}

// 从 inspect 请求中读取 canonical sessionId。
fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    // 读取非空字符串目标。
    request
        // 访问 target 对象。
        .target
        // 读取 sessionId。
        .get("sessionId")
        // 要求字符串类型。
        .and_then(Value::as_str)
        // 拒绝空目标。
        .filter(|value| !value.is_empty())
        // 缺失时返回稳定参数错误。
        .ok_or_else(|| {
            // 使用封闭参数错误码构造缺失目标错误。
            WindowAdapterErrorCode::InvalidArgument.error("target.sessionId is required.")
        })
}

// 在当前窗口快照中唯一重新解析 canonical s2:w。
pub(crate) fn resolve_window<'inventory>(
    // 接收调用方 opaque 目标。
    session_id: &str,
    // 接收当前私有窗口快照。
    windows: &'inventory [WindowRecord],
) -> AppResult<&'inventory WindowRecord> {
    // 对每个候选从私有 PID、HWND 与创建时间重新生成 canonical 身份。
    match match_opaque_target(session_id, windows, |window| {
        // 返回当前候选的 s2:w。
        Some(opaque_window_session_id(window))
    }) {
        // 唯一命中后才允许返回。
        OpaqueTargetMatch::Unique(window) => Ok(window),
        // 零命中表示窗口消失、代际变化或非 canonical 输入。
        OpaqueTargetMatch::Missing => Err(WindowAdapterErrorCode::StaleSession.error(
            // 明确目标已不再存在。
            "The visible titled window target no longer exists.",
        )),
        // 多命中表示指纹碰撞，必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(WindowAdapterErrorCode::AmbiguousTarget.error(
            // 明确不会任取一个窗口。
            "The opaque window session matched more than one window.",
        )),
    }
}

// 检查只读窗口观察期间前景目标未变化。
pub(crate) fn ensure_foreground_unchanged(before: isize, after: isize) -> AppResult<()> {
    // 前景相同表示观察没有干扰主机。
    if before == after {
        // 返回成功。
        return Ok(());
    }
    // 前景变化必须结构化失败。
    Err(WindowAdapterErrorCode::HostInterferenceDetected.error(
        // 明确观察不应改变前景。
        "Foreground changed during window observation.",
    ))
}

// 实现窗口观察 surface。
impl AppAdapter for WindowAdapter {
    // 返回 legacy CLI surface ID。
    fn app_id(&self) -> &'static str {
        // 保持 sessions window / inspect window 入口。
        "window"
    }

    // 报告已迁回 Rust 的窗口观察能力。
    fn status(&self) -> AppResult<Value> {
        // 输出后台与 capability 状态。
        Ok(json!({
            // 标记调用成功。
            "ok": true,
            // 保持 CLI surface ID。
            "app": self.app_id(),
            // 声明只读 Win32 backend。
            "backend": "Win32 visible titled top-level window snapshot",
            // 声明保证后台执行。
            "backgroundPolicy": "guaranteed",
            // 标记只读。
            "readOnly": true,
            // 公布本批迁回的版本化 capability。
            "capabilities": [
                // 窗口发现。
                capabilities::WINDOW_DISCOVER,
                // 精确窗口元数据读取。
                capabilities::WINDOW_METADATA_READ,
            ],
        }))
    }

    // 枚举版本化窗口观察结果。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 在枚举前拒绝原生选择器。
        reject_private_target_fields(request)?;
        // 记录观察前前景窗口。
        let before = foreground_hwnd();
        // 捕获可见有标题顶层窗口。
        let mut windows = capture_visible_titled_windows()?;
        // 只在安全公开字段上过滤。
        filter_public_windows(&mut windows, request);
        // 记录过滤后总数。
        let total = windows.len();
        // 按调用方公开边界截断。
        windows.truncate(request.max_items);
        // 投影隐私安全公开对象。
        let sessions = windows
            // 遍历私有窗口记录。
            .iter()
            // 转换为安全公开 JSON。
            .map(public_window_observation)
            // 收集结果数组。
            .collect::<Vec<_>>();
        // 记录观察后前景窗口。
        let after = foreground_hwnd();
        // 前景变化必须失败。
        ensure_foreground_unchanged(before, after)?;
        // 输出 window-observation schema 数据。
        Ok(json!({
            // 保持 C++ 兼容 surface。
            "surface": "app",
            // 标识版本化发现 capability。
            "capability": capabilities::WINDOW_DISCOVER,
            // 标记只读。
            "readOnly": true,
            // 成功路径保证前景不变。
            "foregroundUnchanged": true,
            // 声明只接受版本化 opaque 目标。
            "targetIdentity": "opaque-versioned-session-id",
            // 输出本次返回数量。
            "count": sessions.len(),
            // 输出过滤后总数。
            "total": total,
            // 标记公开输出是否截断。
            "truncated": total > sessions.len(),
            // 输出安全窗口观察。
            "sessions": sessions,
        }))
    }

    // 对 canonical s2:w 执行使用时重新发现的元数据读取。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 拒绝混入原生选择器。
        reject_private_target_fields(request)?;
        // 读取调用方 opaque 目标。
        let session_id = required_session_id(request)?;
        // 记录观察前前景窗口。
        let before = foreground_hwnd();
        // 每次使用都重新枚举当前窗口快照。
        let windows = capture_visible_titled_windows()?;
        // 要求当前快照唯一命中。
        let window = resolve_window(session_id, &windows)?;
        // 投影唯一命中窗口的公开元数据。
        let window = public_window_observation(window);
        // 记录观察后前景窗口。
        let after = foreground_hwnd();
        // 前景变化必须失败。
        ensure_foreground_unchanged(before, after)?;
        // 输出精确 metadata read 结果。
        Ok(json!({
            // 标识版本化读取 capability。
            "capability": capabilities::WINDOW_METADATA_READ,
            // 标记只读。
            "readOnly": true,
            // 声明 host-headless 执行域。
            "executionDomain": "host-headless",
            // 成功路径保证前景不变。
            "foregroundUnchanged": true,
            // 输出唯一当前窗口观察。
            "window": window,
        }))
    }

    // 窗口观察 surface 永不提供写操作。
    fn run(&self, _: &CommandRequest) -> AppResult<Value> {
        // 返回稳定后台能力缺口。
        Err(
            WindowAdapterErrorCode::BackgroundOperationUnavailable.error(
                // 明确该 surface 只读。
                "window 不提供后台写操作；请使用经过策略评估的统一 app facade。",
            ),
        )
    }
}

// 为仍使用 legacy 形状的内部 adapter 保留前景快照辅助函数。
pub fn foreground_snapshot(before: isize, after: isize) -> Value {
    // 只报告布尔证据，禁止公开前后 HWND。
    json!({ "unchanged": before == after })
}

// 声明无需外部 fixture 的窗口观察契约测试。
#[cfg(test)]
mod tests {
    // 导入父模块全部纯辅助函数与类型。
    use super::*;
    // 导入请求 verb。
    use crate::domain::Verb;
    // 导入公共 catalog 策略以防 inspect 路由回归。
    use crate::policy;

    // 构造稳定窗口观察夹具。
    fn fixture_window() -> WindowRecord {
        // 返回与 C++ identity 顺序一致的记录。
        WindowRecord {
            // legacy 字段故意不可信，公开投影必须重新生成 canonical 身份。
            session_id: "window:4660".to_owned(),
            // 保存私有 HWND 十进制值。
            hwnd: 4660,
            // 保存公开标题。
            title: "Fixture Window".to_owned(),
            // 保存禁止公开的 Win32 类名。
            class_name: "FixtureClass".to_owned(),
            // 保存私有 PID。
            process_id: 42,
            // 保存公开应用名。
            process_name: Some("fixture.exe".to_owned()),
            // 标记窗口可见。
            visible: true,
            // 保存私有进程创建时间。
            process_creation_time: 123,
        }
    }

    // 递归判断 JSON 树中是否出现指定字段名。
    fn contains_key(value: &Value, forbidden: &str) -> bool {
        // 按 JSON 值类别递归扫描。
        match value {
            // 对象同时检查当前键与全部子值。
            Value::Object(object) => object
                // 遍历键值对。
                .iter()
                // 任一键或子树命中即返回真。
                .any(|(key, value)| key == forbidden || contains_key(value, forbidden)),
            // 数组递归扫描全部元素。
            Value::Array(items) => items
                // 遍历元素。
                .iter()
                // 任一子树命中即返回真。
                .any(|value| contains_key(value, forbidden)),
            // 标量没有字段名。
            _ => false,
        }
    }

    // 验证 window observation capability 顺序与 registry 投影一致。
    #[test]
    fn public_window_capabilities_match_registry_definitions() {
        // 取得稳定公开 capability 列表。
        let entries = public_window_capabilities();
        // 定义每个位置应使用的精确 surface 与稳定 ID。
        let expected = [
            // 应用发现属于独立发现 surface。
            (
                capabilities::CapabilitySurface::ApplicationDiscovery,
                capabilities::APPLICATION_DISCOVER,
            ),
            // 窗口发现属于 Window surface。
            (
                capabilities::CapabilitySurface::Window,
                capabilities::WINDOW_DISCOVER,
            ),
            // 可访问性树属于 Accessibility surface。
            (
                capabilities::CapabilitySurface::Accessibility,
                capabilities::ACCESSIBILITY_TREE_READ,
            ),
            // 帧元数据探针属于 Window surface。
            (
                capabilities::CapabilitySurface::Window,
                capabilities::WINDOW_CAPTURE_FRAME_PROBE,
            ),
            // 截图由 App facade 承接。
            (
                capabilities::CapabilitySurface::App,
                capabilities::WINDOW_SCREENSHOT,
            ),
            // 关闭由 App facade 承接。
            (
                capabilities::CapabilitySurface::App,
                capabilities::WINDOW_CLOSE,
            ),
        ];
        // 数量和顺序必须与兼容契约一致。
        assert_eq!(entries.len(), expected.len());
        // 保存已见 ID 以拒绝重复能力。
        let mut ids = std::collections::BTreeSet::new();
        // 逐项核对 registry 拥有的字段。
        for (entry, (surface, id)) in entries.iter().zip(expected) {
            // 只允许精确 surface 定义进入投影。
            let definition = capabilities::definition_for_surface(surface, id)
                // 静态测试缺项应立即失败。
                .unwrap_or_else(|| panic!("missing registry definition for {id}"));
            // ID 必须来自同一定义。
            assert_eq!(entry["id"], definition.id);
            // 公开列表中不得出现重复 ID。
            assert!(ids.insert(definition.id));
            // executionDomain 必须来自强类型执行域。
            assert_eq!(entry["executionDomain"], json!(definition.execution_realm));
            // 全部六项保持现有可用状态。
            assert_eq!(entry["availability"], "available");
            // App generic verb 只在兼容形状存在时核对 registry action。
            if entry.get("verb").is_some() {
                // verb 不得再独立手写。
                assert_eq!(entry["verb"], definition.action.as_str());
                // schema 不得再独立手写。
                assert_eq!(entry["inputSchema"], definition.input_schema);
                // 前台策略不得再独立手写。
                assert_eq!(
                    entry["requiresForegroundConsent"],
                    definition.requires_foreground_consent
                );
            }
        }
        // 兼容层自有的风险和确认值必须保持逐项不变。
        assert_eq!(
            // 提取不属于 registry 的兼容字段。
            entries
                // 遍历固定列表。
                .iter()
                // 组合风险和确认事实。
                .map(|entry| {
                    // 转换为无所有权稳定标量。
                    (
                        // 读取风险文本。
                        entry["risk"].as_str(),
                        // 读取确认布尔值。
                        entry["requiresConfirmation"].as_bool(),
                    )
                })
                // 收集为可比较列表。
                .collect::<Vec<_>>(),
            // 锁定四项读取、截图和关闭的现有值。
            vec![
                // 应用发现是普通读取。
                (Some("read"), Some(false)),
                // 窗口发现是普通读取。
                (Some("read"), Some(false)),
                // 可访问性树是普通读取。
                (Some("read"), Some(false)),
                // 帧探针是确认式敏感读取。
                (Some("read-sensitive"), Some(true)),
                // 截图是确认式敏感读取。
                (Some("read-sensitive"), Some(true)),
                // 关闭是确认式 mutation。
                (Some("mutation"), Some(true)),
            ]
        );
    }

    // 验证 surface 错配不会退回任意 registry 定义。
    #[test]
    #[should_panic(
        expected = "window capability projection must use a registered surface definition"
    )]
    fn window_capability_projection_rejects_surface_mismatch() {
        // 窗口发现不得被当成 App surface 能力。
        let _ = registered_capability(
            // 故意提供错误 surface。
            capabilities::CapabilitySurface::App,
            // 使用真实但属于 Window 的 ID。
            capabilities::WINDOW_DISCOVER,
        );
    }

    // 验证 Rust 与 C++ 使用相同窗口身份布局。
    #[test]
    fn window_identity_matches_cpp_golden() {
        // 从 PID、HWND 与 FILETIME 生成 canonical 目标。
        let session_id = opaque_window_session_id(&fixture_window());
        // 对照独立计算的 FNV-1a golden。
        assert_eq!(session_id, "s2:w:d0150724920d7811");
        // 创建时间变化必须产生不同代际目标。
        let mut next_generation = fixture_window();
        // 模拟同 PID/HWND 的新进程代际。
        next_generation.process_creation_time = 124;
        // 验证目标变化。
        assert_ne!(session_id, opaque_window_session_id(&next_generation));
    }

    // 验证公开窗口对象与 C++ golden 完全一致。
    #[test]
    fn public_window_observation_matches_cpp_golden() {
        // 投影稳定夹具。
        let value = public_window_observation(&fixture_window());
        // 构造 C++ discovery_module.cpp 的预期对象。
        let golden = json!({
            // canonical 窗口身份。
            "sessionId": "s2:w:d0150724920d7811",
            // 通用窗口类别。
            "kind": "window",
            // 稳定目标类别。
            "targetKind": "application-window",
            // 安全应用名。
            "applicationName": "fixture.exe",
            // 当前窗口标题。
            "title": "Fixture Window",
            // 契约只发布可见窗口。
            "visible": true,
            // 复用实现生成的固定 capability golden。
            "capabilities": public_window_capabilities(),
        });
        // 全对象必须相等。
        assert_eq!(value, golden);
        // 收集公开字段名。
        let mut keys = value
            // 要求对象形状。
            .as_object()
            // 展开对象字段迭代器。
            .into_iter()
            // 遍历字段名。
            .flat_map(|object| object.keys())
            // 转为字符串切片。
            .map(String::as_str)
            // 收集规范比较列表。
            .collect::<Vec<_>>();
        // 排序消除 JSON map 实现差异。
        keys.sort_unstable();
        // 与 window-observation schema 字段集对齐。
        assert_eq!(
            keys,
            vec![
                "applicationName",
                "capabilities",
                "kind",
                "sessionId",
                "targetKind",
                "title",
                "visible",
            ]
        );
        // 递归扫描全部公开 JSON 字段名。
        for field in PRIVATE_TARGET_FIELDS {
            // 禁止任何原生事实字段。
            assert!(!contains_key(&value, field));
        }
        // 序列化以检查 legacy 原生目标值。
        let text = value.to_string();
        // 禁止 legacy HWND session 值。
        assert!(!text.contains("window:4660"));
    }

    // 验证窗口重新解析保留 stale 与 ambiguous 的 fail-closed 语义。
    #[test]
    fn window_resolution_is_missing_unique_or_ambiguous() {
        // 构造唯一当前记录。
        let window = fixture_window();
        // 唯一目标必须解析到夹具。
        let resolved = match resolve_window(
            // 使用 canonical 目标。
            "s2:w:d0150724920d7811",
            // 提供单项当前快照。
            std::slice::from_ref(&window),
        ) {
            // 保存唯一命中。
            Ok(record) => record,
            // 唯一夹具不得失败。
            Err(error) => panic!("unique window fixture failed: {}", error.message),
        };
        // 验证唯一命中标题。
        assert_eq!(resolved.title, "Fixture Window");
        // legacy 或过期目标必须视为 stale。
        let stale = resolve_window("window:4660", std::slice::from_ref(&window));
        // 验证 stale 错误码。
        assert_eq!(stale.err().map(|error| error.code), Some("STALE_SESSION"));
        // 构造两个生成同一 canonical 目标的碰撞候选。
        let duplicates = [window.clone(), window];
        // 多命中必须 fail closed。
        let ambiguous = resolve_window("s2:w:d0150724920d7811", &duplicates);
        // 验证歧义错误码。
        assert_eq!(
            ambiguous.err().map(|error| error.code),
            Some("AMBIGUOUS_TARGET")
        );
    }

    // 验证 public sessions 在触碰 Win32 前拒绝原生目标字段。
    #[test]
    fn window_sessions_reject_native_target_fields() {
        // 逐项核对全部禁止字段。
        for field in PRIVATE_TARGET_FIELDS {
            // 构造只读窗口发现请求。
            let mut request = CommandRequest::read(Verb::Sessions, "window");
            // 注入禁止字段。
            request.target.insert((*field).to_owned(), json!(42));
            // 执行前置边界检查。
            let error = match WindowAdapter.sessions(&request) {
                // 禁止误报成功。
                Ok(_) => panic!("native field {field} must be rejected"),
                // 保存结构化错误。
                Err(error) => error,
            };
            // 验证稳定参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }

    // 验证公共 catalog 允许 window inspect 到达 adapter。
    #[test]
    fn window_catalog_allows_opaque_metadata_inspect() -> AppResult<()> {
        // 构造精确只读 inspect 请求。
        let mut request = CommandRequest::read(Verb::Inspect, "window");
        // 提供 canonical 窗口目标。
        request.target.insert(
            // 写入公开 sessionId 字段。
            "sessionId".to_owned(),
            // 使用稳定夹具目标。
            json!("s2:w:d0150724920d7811"),
        );
        // catalog/policy 必须允许请求进入 adapter 的 stale/unique 门禁。
        policy::validate(&request)?;
        // 完成 catalog 回归测试。
        Ok(())
    }

    // 验证实时 Rust sessions 输出版本化且不泄漏原生字段。
    #[test]
    fn live_window_sessions_are_versioned_and_private() -> AppResult<()> {
        // 构造有界只读请求。
        let mut request = CommandRequest::read(Verb::Sessions, "window");
        // 限制测试输出规模。
        request.max_items = 8;
        // 执行实时 Win32 观察。
        let value = WindowAdapter.sessions(&request)?;
        // 收集顶层 schema 字段。
        let mut keys = value
            // 要求对象结果。
            .as_object()
            // 展开对象字段。
            .into_iter()
            // 遍历字段名。
            .flat_map(|object| object.keys())
            // 转为字符串切片。
            .map(String::as_str)
            // 收集规范比较列表。
            .collect::<Vec<_>>();
        // 排序保证稳定。
        keys.sort_unstable();
        // 与 window-observation schema 顶层字段集对齐。
        assert_eq!(
            keys,
            vec![
                "capability",
                "count",
                "foregroundUnchanged",
                "readOnly",
                "sessions",
                "surface",
                "targetIdentity",
                "total",
                "truncated",
            ]
        );
        // 验证 capability ID。
        assert_eq!(value["capability"], capabilities::WINDOW_DISCOVER);
        // 验证返回边界。
        assert!(value["count"].as_u64().is_some_and(|count| count <= 8));
        // 禁止所有原生目标字段名。
        for field in PRIVATE_TARGET_FIELDS {
            // 公开 JSON 树不得包含该字段。
            assert!(!contains_key(&value, field));
        }
        // 验证全部返回项可见、有标题且使用 canonical s2:w。
        for session in value["sessions"].as_array().into_iter().flatten() {
            // 读取 session ID。
            let session_id = session["sessionId"].as_str().unwrap_or_default();
            // 要求 canonical 窗口目标前缀。
            assert!(session_id.starts_with("s2:w:"));
            // 要求窗口可见。
            assert_eq!(session["visible"], true);
            // 要求标题非空。
            assert!(
                session["title"]
                    // 读取标题字符串。
                    .as_str()
                    // 检查非空。
                    .is_some_and(|title| !title.is_empty())
            );
        }
        // 完成实时观察测试。
        Ok(())
    }

    // 验证前景证据不会公开原生窗口句柄。
    #[test]
    fn foreground_snapshot_only_reports_invariance() {
        // 不同私有句柄只产生 false。
        let changed = foreground_snapshot(42, 84);
        // 精确核对唯一公开字段。
        assert_eq!(changed, json!({ "unchanged": false }));
        // 相同私有句柄只产生 true。
        let unchanged = foreground_snapshot(42, 42);
        // 禁止恢复 before 或 after。
        assert_eq!(unchanged, json!({ "unchanged": true }));
    }
}
