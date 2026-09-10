// 把错误码实现保留为当前 desktop provider Adapter 的普通私有类型。
#[path = "desktop_error.rs"]
mod error_code;
// 把窗口关闭成功投影保留为窄私有 Component。
#[path = "window_close_projection.rs"]
mod window_close_projection;

use serde_json::{Map, Value, json};

// 导入当前 desktop provider Adapter 私有封闭错误码。
use error_code::AppDesktopErrorCode;
// 导入窗口关闭成功事实的 provider-neutral 投影。
use window_close_projection::public_window_close_result;

use crate::{
    adapters::{
        AppAdapter,
        DesktopAdapter,
        app::CapabilityProvider,
        // 导入单一窗口快照与 canonical 身份生成器。
        windows::{filter_windows, opaque_window_session_id},
    },
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入统一 opaque 目标碰撞与过期匹配 Component。
    components::{
        opaque_id::{OpaqueTargetMatch, match_opaque_target},
        window_target_identity,
    },
    domain::{AppResult, CommandRequest},
    // 精确窗口的等待、关闭、截图与录制只通过领域 Module。
    modules::{
        // 导入 UIA 语义等待 Module。
        accessibility_wait,
        // 导入 provider-neutral UIA 元素定位 Module。
        element_location,
        // 导入通用请求内有界键盘输入 Module。
        keyboard_input,
        // 导入通用请求内有界指针输入 Module。
        pointer_input,
        // 导入 provider-neutral 语义元素动作 Module。
        semantic_action,
        // 导入精确窗口关闭 Module。
        window_close,
        // 导入精确窗口关闭等待 Module。
        window_closed_wait,
        // 导入精确窗口录制 Module。
        window_record,
        // 导入精确窗口截图 Module。
        window_screenshot,
    },
};

use super::{
    capability_descriptor, session::ensure_capability_foreground_consent,
    session::required_session_id, window_lifecycle as window_lifecycle_route,
};

pub(super) struct DesktopProvider;

const DESKTOP_CAPABILITIES: &[&str] = &[
    // 公布只读语义元素定位 capability。
    capabilities::UI_ELEMENT_LOCATE,
    // 公布已确认语义元素动作 capability。
    capabilities::UI_ELEMENT_ACTION,
    // 公布只读语义元素等待 capability。
    capabilities::UI_ELEMENT_WAIT,
    // 公布只读精确窗口关闭等待 capability。
    capabilities::WINDOW_CLOSED_WAIT,
    // 公布窗口截图 capability。
    capabilities::WINDOW_SCREENSHOT,
    // 公布窗口录制 capability。
    capabilities::WINDOW_RECORD,
    // 公布窗口关闭 capability。
    capabilities::WINDOW_CLOSE,
    // 公布通用窗口状态与几何生命周期 capability。
    capabilities::WINDOW_LIFECYCLE,
    // 公布前台键盘输入 capability。
    capabilities::UI_INPUT_KEY,
    // 公布前台指针输入 capability。
    capabilities::UI_INPUT_POINTER,
];

impl DesktopProvider {
    fn native_session(&self, opaque_id: &str) -> AppResult<String> {
        // 每次解析都重新枚举当前窗口，禁止复用旧快照中的原生目标。
        let records = filter_windows(&Map::new())?;
        // 使用统一 Component 区分零、唯一和多命中。
        match match_opaque_target(opaque_id, &records, |record| {
            // 只从当前 PID、HWND 和进程创建时间生成候选身份。
            Some(opaque_window_session_id(record))
            // 结束窗口候选身份生成。
        }) {
            // 唯一命中后才在内部委托中恢复旧直接 session。
            OpaqueTargetMatch::Unique(record) => Ok(record.session_id.clone()),
            // 零命中明确表示目标已在使用时过期。
            OpaqueTargetMatch::Missing => Err(
                // 使用 desktop provider 私有目标过期分类。
                AppDesktopErrorCode::StaleSession.error("The window session no longer exists."),
            ),
            // 两个或更多当前候选必须 fail closed。
            OpaqueTargetMatch::Ambiguous => Err(
                // 使用 desktop provider 私有目标歧义分类。
                AppDesktopErrorCode::AmbiguousTarget
                    // 保持既有 provider-neutral 公开消息。
                    .error("The opaque window session matched more than one window."),
            ),
            // 结束窗口重新解析分类。
        }
    }

    fn delegated_request(
        &self,
        request: &CommandRequest,
        operation: &str,
    ) -> AppResult<CommandRequest> {
        let mut delegated = request.clone();
        delegated.app = "desktop".to_owned();
        delegated.operation = Some(operation.to_owned());
        delegated.target = Map::from_iter([(
            "sessionId".to_owned(),
            Value::String(self.native_session(required_session_id(request)?)?),
        )]);
        // 只读 inspect 不要求写操作的 input 包装对象。
        delegated.args = if operation == "inspect" {
            // 为底层只读 inspect 提供空参数对象。
            Map::new()
        // 写 capability 继续执行统一输入形状门禁。
        } else {
            // 读取统一 facade 的 capability input 对象。
            request
                // 访问调用方参数集合。
                .args
                // 读取固定 input 包装字段。
                .get("input")
                // 要求 input 是 JSON object。
                .and_then(Value::as_object)
                // 复制为底层 desktop operation 参数。
                .cloned()
                // 缺失或错误形状返回稳定结构化错误。
                .ok_or_else(|| {
                    // 构造统一 invalid argument 错误。
                    AppDesktopErrorCode::InvalidArgument.error("args.input must be an object.")
                    // 结束输入错误构造。
                })?
            // 结束只读与写入参数分流。
        };
        Ok(delegated)
    }
}

impl CapabilityProvider for DesktopProvider {
    fn provider_id(&self) -> &'static str {
        "windows-window"
    }

    fn capabilities(&self) -> &'static [&'static str] {
        DESKTOP_CAPABILITIES
    }

    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 通过实时重新发现确认该 s2 目标仍唯一存在。
        match self.native_session(session_id) {
            Ok(_) => Ok(true),
            Err(error) if AppDesktopErrorCode::StaleSession.matches(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn status(&self) -> AppResult<Value> {
        Ok(json!({
            "ok": true,
            "backend": self.provider_id(),
            "scope": "window",
            "capabilities": self.capabilities(),
            "result": DesktopAdapter.status()?,
        }))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        let mut discovery = request.clone();
        discovery
            .target
            .entry("visible".to_owned())
            .or_insert(Value::Bool(true));
        // 只使用一个当前窗口快照生成事实和精确身份。
        let mut records = filter_windows(&discovery.target)?;
        // 统一 app facade 只发布与 C++ 契约相同的可见有标题窗口。
        records.retain(|record| record.visible && !record.title.is_empty());
        // 在生成公共结果前应用调用方的有界条数。
        records.truncate(request.max_items);
        // 从同一记录生成公共事实和 s2 身份，避免跨快照错配。
        let sessions = records
            .into_iter()
            .filter_map(|record| {
                // 先计算只依赖私有字段的 canonical s2 ID。
                let session_id = opaque_window_session_id(&record);
                // 序列化旧兼容事实以复用稳定字段名称。
                let mut public = serde_json::to_value(&record).ok()?;
                // 取得可修改的公共事实对象。
                let object = public.as_object_mut()?;
                // 删除原生窗口句柄。
                object.remove("hwnd");
                // 删除原生进程标识。
                object.remove("processId");
                // 删除原生窗口类名。
                object.remove("className");
                // 写入可重新发现的 opaque session。
                object.insert("sessionId".to_owned(), json!(session_id));
                // 标记公共目标类型。
                object.insert("kind".to_owned(), json!("window"));
                // 公开当前 opaque 窗口目标可证明的身份强度。
                object.insert("targetIdentityStrength".to_owned(), window_target_identity::public_assurance());
                // 声明窗口只读能力。
                object.insert(
                    "capabilities".to_owned(),
                    json!([
                        capability_descriptor(
                            capabilities::UI_ELEMENT_LOCATE,
                            json!({
                                // 定位只接受精确 opaque 窗口。
                                "target": "exact-window",
                                // selector 只允许公共语义属性。
                                "selector": ["name", "automationId", "className", "frameworkId", "controlType"],
                                // 搜索深度保持有界。
                                "maximumDepth": { "min": 0, "max": 20, "default": 8 },
                                // 搜索节点数保持有界。
                                "maximumItems": { "min": 1, "max": 4_096, "default": 1_024 },
                                // 只允许既有 UIA view。
                                "view": ["control", "raw"],
                                // worker deadline 保持硬边界。
                                "timeoutMs": { "min": 1, "max": 30_000, "default": 2_000 },
                                // 坐标使用带符号虚拟桌面物理像素。
                                "coordinateUnit": "physical-screen-px",
                                // element ID 只属于当前定位快照。
                                "elementIdentity": "observation-only-snapshot"
                            }),
                        ),
                        capability_descriptor(
                            capabilities::UI_ELEMENT_ACTION,
                            json!({
                                // 动作只接受原 canonical 精确窗口。
                                "target": "exact-window",
                                // selector 只允许公共语义属性。
                                "selector": ["name", "automationId", "className", "frameworkId", "controlType"],
                                // 只发布五种 provider-neutral 动作。
                                "actions": ["invoke", "value", "toggle", "select", "scroll"],
                                // Value 允许清空并限制 UTF-8 字节数。
                                "valueBytes": { "min": 0, "max": 65_536 },
                                // Scroll 只允许封闭相对量且至少一个轴非 no-amount。
                                "scrollAmounts": ["no-amount", "large-decrement", "small-decrement", "large-increment", "small-increment"],
                                // 搜索深度保持有界。
                                "maximumDepth": { "min": 0, "max": 20, "default": 8 },
                                // 搜索节点数保持有界。
                                "maximumItems": { "min": 1, "max": 4_096, "default": 1_024 },
                                // 只允许既有 UIA view。
                                "view": ["control", "raw"],
                                // worker deadline 保持硬边界。
                                "timeoutMs": { "min": 1, "max": 30_000, "default": 2_000 },
                                // snapshot element ID 永不作为写目标。
                                "elementIdentity": "re-resolve-from-selector",
                                // 不支持时禁止静默指针降级。
                                "fallback": "none"
                            }),
                        ),
                        capability_descriptor(
                            capabilities::WINDOW_CLOSED_WAIT,
                            json!({
                                // 等待只接受精确 opaque 窗口。
                                "target": "exact-window",
                                // 总等待时间保持硬边界。
                                "timeoutMs": { "min": 50, "max": 30_000, "default": 5_000 },
                                // 轮询间隔避免忙循环和不可取消长暂停。
                                "pollIntervalMs": { "min": 25, "max": 1_000, "default": 100 },
                                // 连续缺失时间不得超过总超时。
                                "stableForMs": { "min": 0, "max": "timeoutMs", "default": 100 },
                                // 成功只证明目标无法从当前公开 inventory 解析。
                                "closureEvidence": "opaque-target-missing-from-visible-titled-window-inventory"
                            }),
                        ),
                        capability_descriptor(
                            capabilities::UI_ELEMENT_WAIT,
                            json!({
                                // 等待只接受精确 opaque 窗口。
                                "target": "exact-window",
                                // selector 只允许公开语义属性。
                                "selector": ["name", "automationId", "className", "frameworkId", "controlType"],
                                // 状态只允许可用性与可见性。
                                "state": ["enabled", "visible"],
                                // 总等待时间保持硬边界。
                                "timeoutMs": { "min": 50, "max": 30_000, "default": 5_000 },
                                // 轮询间隔避免忙循环和不可取消长暂停。
                                "pollIntervalMs": { "min": 25, "max": 1_000, "default": 100 },
                                // 连续稳定时间不得超过总超时。
                                "stableForMs": { "min": 0, "max": "timeoutMs", "default": 300 },
                                // 搜索深度与树 capability 共用边界。
                                "maximumDepth": { "min": 0, "max": 20, "default": 8 },
                                // 搜索节点数与树 capability 共用边界。
                                "maximumItems": { "min": 1, "max": 4_096, "default": 1_024 },
                                // 只允许既有 UIA view。
                                "view": ["control", "raw"]
                            }),
                        ),
                        capability_descriptor(
                            capabilities::WINDOW_SCREENSHOT,
                            json!({ "format": "png", "target": "exact-window" }),
                        ),
                        capability_descriptor(
                            capabilities::WINDOW_RECORD,
                            json!({
                                "format": "mp4/h264",
                                "target": "exact-window",
                                "defaults": {
                                    "fps": 2,
                                    "maxWidth": 960,
                                    "quality": 75,
                                    "maxKeyframes": 8
                                },
                                "analysis": "temporal-difference-storyboard"
                            }),
                        ),
                        capability_descriptor(
                            capabilities::WINDOW_CLOSE,
                            json!({ "target": "exact-window", "protocol": "WM_CLOSE" }),
                        ),
                        capability_descriptor(
                            // 公布稳定窗口生命周期 capability。
                            capabilities::WINDOW_LIFECYCLE,
                            // 只公开 provider-neutral 约束。
                            json!({
                                // 生命周期动作只接受精确 opaque 窗口。
                                "target": "exact-window",
                                // 公布五种通用状态与几何动作。
                                "actions": ["restore", "minimize", "maximize", "move", "resize"],
                                // 几何统一使用虚拟桌面物理像素。
                                "coordinateSpace": "screen-physical-px",
                                // 声明 Per-Monitor-V2 坐标上下文。
                                "dpiContext": "per-monitor-v2",
                                // 多显示器坐标允许负值。
                                "signedVirtualScreenCoordinates": true,
                                // 最小尺寸取目标当前 DPI 下的系统 tracking size。
                                "minimumSize": "current-dpi-system-tracking",
                                // 同步 deadline 保持硬边界。
                                "timeoutMs": { "min": 1, "max": 30_000, "default": 2_000 },
                                // 区分平台接受与最终状态读回。
                                "result": "accepted-and-final-state-readback",
                                // 能力缺失不得降级为键鼠或特定软件路径。
                                "fallback": "none"
                            }),
                        ),
                        capability_descriptor(
                            capabilities::UI_INPUT_KEY,
                            json!({
                                // 键盘只接受原 canonical 精确窗口。
                                "target": "exact-window",
                                // 公布正式三类步骤。
                                "steps": ["key", "chord", "text"],
                                // 公布成对和显式阶段。
                                "phase": ["press", "down", "up"],
                                // 按键所有权不得跨越一次短命 CLI 请求。
                                "keyOwnership": "request-scoped-balanced",
                                // 快捷键保持按下顺序并逆序释放。
                                "chordOrder": "ordered-down-reverse-up",
                                // 公布完整键集类别但不暴露平台键码。
                                "keySet": ["alphanumeric", "f1-f24", "navigation", "numpad", "left-right-modifiers", "system", "punctuation", "media"],
                                // Unicode 文本按 scalar 边界调度。
                                "unicodeText": true,
                                // 持续时间保持有界。
                                "holdMs": { "min": 0, "max": 5_000 },
                                // 重复次数保持有界。
                                "repeat": { "min": 1, "max": 100 },
                                // 动作序列保持有界。
                                "maximumSteps": 128,
                                // 同步 deadline 保持硬边界。
                                "timeoutMs": { "min": 1, "max": 30_000, "default": 2_000 },
                                // 旧单键与加号组合继续映射到安全 press。
                                "legacyKeyCompatibility": true
                            }),
                        ),
                        capability_descriptor(
                            capabilities::UI_INPUT_POINTER,
                            json!({
                                // 指针只接受原 canonical 精确窗口。
                                "target": "exact-window",
                                // 公布两种 provider-neutral 物理像素坐标空间。
                                "coordinateSpaces": ["screen-physical-px", "window-client-physical-px"],
                                // 公布五类通用指针步骤。
                                "steps": ["move", "button", "click", "scroll", "drag"],
                                // 公布三类按钮与显式上下阶段。
                                "buttons": ["left", "right", "middle"],
                                // 按钮所有权不得跨越一次短命 CLI 请求。
                                "buttonOwnership": "request-scoped-balanced",
                                // 单双击是封闭计数。
                                "clickCount": { "min": 1, "max": 2 },
                                // 同时支持垂直与水平滚轮。
                                "scrollAxes": ["vertical", "horizontal"],
                                // 动作序列保持有界。
                                "maximumSteps": 64,
                                // 同步 deadline 保持硬边界。
                                "timeoutMs": { "min": 1, "max": 30_000, "default": 2_000 },
                                // 旧 `{x,y}` 继续解释为屏幕物理像素左键单击。
                                "legacyClickCompatibility": true
                            }),
                        ),
                    ]),
                );
                // 返回清理后的公共窗口事实。
                Some(public)
            })
            .collect::<Vec<_>>();
        Ok(json!({ "ok": true, "sessions": sessions }))
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        let mut result = DesktopAdapter.inspect(&self.delegated_request(request, "inspect")?)?;
        sanitize_desktop_result(&mut result);
        Ok(result)
    }

    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // 生命周期 Module 自主保证确认先于前景同意、输入与目标解析。
        if capability == capabilities::WINDOW_LIFECYCLE {
            // 直接委托窄 app 路由，禁止恢复 legacy native session。
            return window_lifecycle_route::execute(request);
        }
        ensure_capability_foreground_consent(capability, request)?;
        // 通用键盘输入通过独立同步 Module，不再委托 legacy press-key Adapter。
        if capability == capabilities::UI_INPUT_KEY {
            // confirmation 必须先于 input、target 与 provider 解析。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确键盘动作是前景 mutation。
                    "Keyboard input requires explicit per-operation confirmation.",
                ));
            }
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for ui.input.key@1.",
                )
            })?;
            // 读取调用方原 canonical 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Keyboard Input Module 执行确认、权限、前景与状态机。
            return keyboard_input::perform(session_id, request.confirmed, input);
        }
        // 通用指针输入通过独立同步 Module，不再委托 legacy 单击 Adapter。
        if capability == capabilities::UI_INPUT_POINTER {
            // confirmation 必须先于 input、target 与 provider 解析。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确指针动作是前景 mutation。
                    "Pointer input requires explicit per-operation confirmation.",
                ));
            }
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for ui.input.pointer@1.",
                )
            })?;
            // 读取调用方原 canonical 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Pointer Input Module 执行确认、权限、前景与状态机。
            return pointer_input::perform(session_id, request.confirmed, input);
        }
        // 语义元素动作通过独立写 Module，不进入只读 observation worker。
        if capability == capabilities::UI_ELEMENT_ACTION {
            // confirmation 必须先于 input、target 与 provider 解析。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确语义动作是 mutation。
                    "Semantic element action requires explicit confirmation.",
                ));
            }
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for ui.element.action@1.",
                )
            })?;
            // 读取调用方原 canonical 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Semantic Action Module 执行门禁、重解析与一次 dispatch。
            return semantic_action::perform(session_id, request.confirmed, input);
        }
        // UI element locate 保持只读，不经过 legacy DesktopAdapter。
        if capability == capabilities::UI_ELEMENT_LOCATE {
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for ui.element.locate@1.",
                )
            })?;
            // 读取调用方精确 opaque 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Element Location Module 组合重解析、隔离搜索与安全投影。
            return element_location::locate(session_id, input);
        }
        // UI element wait 保持只读，不经过 legacy DesktopAdapter。
        if capability == capabilities::UI_ELEMENT_WAIT {
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for ui.element.wait@1.",
                )
            })?;
            // 读取调用方精确 opaque 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Accessibility Module 组合隔离采样与稳定状态。
            return accessibility_wait::wait_for_element(session_id, input);
        }
        // 精确窗口关闭等待保持只读，不经过 legacy DesktopAdapter。
        if capability == capabilities::WINDOW_CLOSED_WAIT {
            // 读取 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for window.closed.wait@1.",
                )
            })?;
            // 读取调用方精确 opaque 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Window Closed Wait Module 组合重新发现与稳定缺失语义。
            return window_closed_wait::wait_for_closed(session_id, input);
        }
        // 精确窗口截图不得转换为旧 native session 或委托 legacy DesktopAdapter。
        if capability == capabilities::WINDOW_SCREENSHOT {
            // confirmation 必须先于 input、target 和文件路径解析。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确截图是敏感读取。
                    "Exact window screenshot requires explicit confirmation.",
                ));
            }
            // app screenshot 必须携带 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需字段。
                    "args.input is required for window.screenshot@1.",
                )
            })?;
            // 读取调用方 opaque 精确窗口目标。
            let session_id = required_session_id(request)?;
            // 委托 Window Screenshot Module 完成隔离捕获与原子提交。
            return window_screenshot::screenshot(session_id, request.confirmed, input);
        }
        // 精确窗口录制不得转换为旧 native session 或委托 legacy adapter。
        if capability == capabilities::WINDOW_RECORD {
            // confirmation 必须先于 input、target 和文件路径解析。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确录制是敏感读取。
                    "Exact window recording requires explicit confirmation.",
                ));
            }
            // app record 必须携带 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需输入。
                    "args.input is required for window.record@1.",
                )
            })?;
            // 读取调用方 canonical opaque 窗口目标。
            let session_id = required_session_id(request)?;
            // 委托正式 Rust Module 完成隔离编码与多产物事务。
            return window_record::record(session_id, request.confirmed, input);
        }
        // 精确窗口关闭不得转换为旧 native session 或委托旧 DesktopAdapter。
        if capability == capabilities::WINDOW_CLOSE {
            // confirmation 必须先于 input 和 Module 内目标发现。
            if !request.confirmed {
                // 返回统一确认错误。
                return Err(AppDesktopErrorCode::ConfirmationRequired.error(
                    // 明确关闭是 mutation。
                    "Exact window close requires confirmation.",
                ));
            }
            // app.close 必须携带 provider-neutral input 对象。
            let input = request.args.get("input").ok_or_else(|| {
                // 返回参数错误。
                AppDesktopErrorCode::InvalidArgument.error(
                    // 说明必需字段。
                    "args.input is required for window.close@1.",
                )
            })?;
            // 读取并验证可选 timeoutMs。
            let timeout_ms = window_close::provider_input(input)?;
            // 读取调用方 opaque 精确窗口目标。
            let session_id = required_session_id(request)?;
            // 执行正式 Window Close Module。
            let result = window_close::close(
                // 传入精确 opaque 目标。
                session_id,
                // 传入逐操作确认。
                request.confirmed,
                // 传入已验证 deadline。
                timeout_ms,
            )?;
            // 返回安全 provider-neutral mapper。
            return public_window_close_result(&result, session_id);
        }
        // 未登记 capability 以 provider 私有缺口分类失败。
        Err(
            // 构造稳定 provider-neutral 公开错误。
            AppDesktopErrorCode::CapabilityUnsupported
                // 保持既有 capability 缺口消息。
                .error("The window session does not support this capability."),
        )
    }
}

fn sanitize_desktop_result(result: &mut Value) {
    if let Some(object) = result.as_object_mut() {
        object.remove("app");
        object.remove("operation");
        object.remove("captureMethod");
        object.remove("encoder");
        object.remove("deviceDriver");
        object.remove("systemCaptureIndicatorMayAppear");
        for key in ["target", "window"] {
            if let Some(target) = object.get_mut(key).and_then(Value::as_object_mut) {
                target.remove("hwnd");
                target.remove("processId");
                target.remove("sessionId");
                target.remove("className");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 导入不触发真实 provider 的请求 verb。
    use crate::domain::Verb;

    // 验证指针确认门禁先于 input 与 target 解析。
    #[test]
    fn pointer_confirmation_precedes_input_and_target_resolution() {
        // 构造不含 input 与 target 的直接 provider 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 提供前景影响同意以越过更早的统一策略门禁。
        request.foreground_consent = true;
        // 直接执行指针 capability 并显式区分结果。
        let error = match DesktopProvider.execute(capabilities::UI_INPUT_POINTER, &request) {
            // 成功表示确认门禁失效。
            Ok(_) => panic!("pointer input must require confirmation first"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对稳定确认错误而不是 input 或 target 错误。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    // 验证窗口生命周期路由保持确认优先策略。
    #[test]
    fn window_lifecycle_confirmation_precedes_input_and_target_resolution() {
        // 构造不含 input 与 target 的直接 provider 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 提供前景影响同意以隔离确认门禁。
        request.foreground_consent = true;
        // 直接执行生命周期 capability 并显式区分结果。
        let error = match DesktopProvider.execute(capabilities::WINDOW_LIFECYCLE, &request) {
            // 成功表示确认门禁失效。
            Ok(_) => panic!("window lifecycle must require confirmation first"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对稳定确认错误而不是 input 或 target 错误。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn facade_sanitizes_native_window_identifiers() {
        let mut value = json!({
            "app": "desktop",
            "operation": "screenshot",
            "captureMethod": "Windows.Graphics.Capture",
            "deviceDriver": "hardware",
            "target": {
                "sessionId": "window:42",
                "hwnd": 42,
                "processId": 7,
                "className": "NativeWindow",
                "title": "Example"
            }
        });
        sanitize_desktop_result(&mut value);
        assert_eq!(value["target"]["title"], "Example");
        assert!(value.get("app").is_none());
        assert!(value.get("captureMethod").is_none());
        assert!(value["target"].get("sessionId").is_none());
        assert!(value["target"].get("hwnd").is_none());
        assert!(value["target"].get("processId").is_none());
        assert!(value["target"].get("className").is_none());
    }

    #[test]
    fn exact_window_close_is_a_registered_background_capability() {
        assert!(
            DesktopProvider
                .capabilities()
                .contains(&capabilities::WINDOW_CLOSE)
        );
    }

    // 验证通用窗口生命周期由桌面 provider 公开。
    #[test]
    fn exact_window_lifecycle_is_a_registered_foreground_capability() {
        // provider 必须声明版本化生命周期 ID。
        assert!(
            // 读取桌面 provider 静态 capability 清单。
            DesktopProvider
                // 取得 capability 列表。
                .capabilities()
                // 核对通用窗口生命周期。
                .contains(&capabilities::WINDOW_LIFECYCLE)
        );
    }

    // 验证精确窗口关闭等待由桌面 provider 公开。
    #[test]
    fn exact_window_closed_wait_is_a_registered_background_capability() {
        // provider 必须声明版本化只读等待 ID。
        assert!(
            DesktopProvider
                // 读取静态 capability 清单。
                .capabilities()
                // 核对精确关闭等待。
                .contains(&capabilities::WINDOW_CLOSED_WAIT)
        );
    }

    #[test]
    fn exact_window_recording_is_a_registered_background_capability() {
        assert!(
            DesktopProvider
                .capabilities()
                .contains(&capabilities::WINDOW_RECORD)
        );
    }

    // 验证四种敏感窗口 capability 均在解析目标和输入前要求确认。
    #[test]
    fn sensitive_window_capabilities_require_confirmation_before_target_and_input() {
        // 固定 capability 与既有公开消息对照表。
        let cases = [
            // 锁定语义元素动作确认消息。
            (
                capabilities::UI_ELEMENT_ACTION,
                "Semantic element action requires explicit confirmation.",
            ),
            // 锁定精确窗口截图确认消息。
            (
                capabilities::WINDOW_SCREENSHOT,
                "Exact window screenshot requires explicit confirmation.",
            ),
            // 锁定精确窗口录制确认消息。
            (
                capabilities::WINDOW_RECORD,
                "Exact window recording requires explicit confirmation.",
            ),
            // 锁定精确窗口关闭确认消息。
            (
                capabilities::WINDOW_CLOSE,
                "Exact window close requires confirmation.",
            ),
        ];
        // 逐项验证缺失 target 和 input 时仍先返回确认错误。
        for (capability, message) in cases {
            // 构造未确认且不含目标或输入的请求。
            let request = CommandRequest::read(Verb::Run, "app");
            // 直接调用 provider 门禁且禁止进入真实窗口 Module。
            let error = DesktopProvider
                // 执行指定敏感 capability。
                .execute(capability, &request)
                // 未确认请求必须失败。
                .err()
                // 使用显式 panic 保留 capability 上下文。
                .unwrap_or_else(|| panic!("{capability} must require confirmation"));
            // 保持稳定确认错误码。
            assert_eq!(error.code, "CONFIRMATION_REQUIRED");
            // 保持既有逐 capability 消息。
            assert_eq!(error.message, message);
        }
    }

    // 验证关闭 Module 的不完整成功结果保持 provider 自有失败语义。
    #[test]
    fn window_close_projection_rejects_missing_success_evidence() {
        // 缺少 closed=true 必须失败闭合。
        let missing_closed = public_window_close_result(
            // 仅提供前景证据。
            &json!({ "foregroundUnchanged": true }),
            // 使用调用方已知 opaque ID。
            "s2:a:0000000000000000",
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing closure evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_closed.code, "OPERATION_FAILED");
        // 保持既有关闭证据消息。
        assert_eq!(
            missing_closed.message,
            "The window close module returned no closure evidence."
        );

        // 缺少 foregroundUnchanged=true 同样必须失败闭合。
        let missing_foreground = public_window_close_result(
            // 仅提供关闭证据。
            &json!({ "closed": true }),
            // 使用调用方已知 opaque ID。
            "s2:a:0000000000000000",
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing foreground evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_foreground.code, "OPERATION_FAILED");
        // 保持既有前景证据消息。
        assert_eq!(
            missing_foreground.message,
            "The window close module returned no foreground evidence."
        );

        // 缺少权限证据时必须失败闭合。
        let missing_permission = public_window_close_result(
            // 提供其他成功证据以隔离权限失败。
            &json!({
                // 提供关闭证据。
                "closed": true,
                // 提供前景证据。
                "foregroundUnchanged": true,
            }),
            // 使用调用方已知 opaque ID。
            "s2:w:0000000000000000",
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing permission evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_permission.code, "OPERATION_FAILED");
        // 核对稳定权限证据消息。
        assert_eq!(
            missing_permission.message,
            "The window close module returned no certified permission evidence."
        );

        // 主动写探针证据为 true 时必须失败闭合。
        let active_probe = public_window_close_result(
            // 提供表面完整但不安全的结果。
            &json!({
                // 提供关闭证据。
                "closed": true,
                // 提供前景证据。
                "foregroundUnchanged": true,
                // 提供成功权限关系。
                "permissionPreflight": "no-static-integrity-block-observed",
                // 故意声明执行过主动写探针。
                "activeWriteProbePerformed": true,
            }),
            // 使用调用方已知 opaque ID。
            "s2:w:0000000000000000",
        )
        // 不安全结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("active write probe evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(active_probe.code, "OPERATION_FAILED");
        // 核对稳定写探针消息。
        assert_eq!(
            active_probe.message,
            "The window close module returned invalid write-probe evidence."
        );
    }
}
