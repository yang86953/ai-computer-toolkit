//! MCP 工具清单与输入 schema。
//!
//! 清单是 agent 可见的公开契约：工具名、描述与 JSON Schema 在进程启动时即固定，
//! 枚举工具不连接桌面、不打开 Portal。

use serde_json::{Value, json};

use crate::components::desktop_interaction::MAXIMUM_INTERACTION_STEPS;

/// 授权三元组：这些布尔值表示用户已有授权，不是模型可自行批准的权限。
const AUTH: [&str; 3] = ["confirmed", "foregroundConsent", "strictIsolation"];

/// 会话标识与截图帧标识的公开形态。
const SESSION_ID_PATTERN: &str = "^s2:i:[0-9a-f]{16}$";
const FRAME_ID_PATTERN: &str = "^[0-9a-f]{32}$";

/// 省略确认字段的统一说明：只在 session 授权会话内允许继承。
const OMITTED_CONSENT: &str = " 连接时用 authorizationMode=session 可让整条会话继承一次确认，届时可省略这些字段；显式传 false 仍会被拒绝。";

/// 所有工具共用的前台语义提示，避免逐工具重复措辞。
const NOTICE: &str =
    " 操作真实前台桌面；不保证焦点、输入法或应用完成。确认标志仅表达用户已有授权。";

/// 构造带固定注解的工具条目。
fn tool(name: &str, description: impl Into<String>, input_schema: Value, read_only: bool) -> Value {
    json!({
        "name": format!("computer_{name}"),
        "description": format!("{}{NOTICE}", description.into()),
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": !read_only,
            "idempotentHint": false,
            "openWorldHint": true,
        },
    })
}

/// 有界整数属性。
fn integer(minimum: i64, maximum: i64, description: &str) -> Value {
    json!({ "type": "integer", "minimum": minimum, "maximum": maximum, "description": description })
}

/// 授权布尔属性。
fn consent(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

/// 构造只有必要说明的对象 schema，默认禁止未声明字段。
fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// 会话标识属性。
fn session_id_property() -> Value {
    json!({ "type": "string", "pattern": SESSION_ID_PATTERN })
}

/// 截图帧标识属性。
fn frame_id_property() -> Value {
    json!({ "type": "string", "pattern": FRAME_ID_PATTERN })
}

/// 单步交互动作的封闭集合。
fn interaction_step_schema() -> Value {
    json!({
        "oneOf": [
            object(json!({
                "type": { "const": "move" },
                "x": integer(0, 100_000, "observation-px 横坐标"),
                "y": integer(0, 100_000, "observation-px 纵坐标"),
            }), &["type", "x", "y"]),
            object(json!({
                "type": { "const": "click" },
                "x": integer(0, 100_000, "observation-px 横坐标"),
                "y": integer(0, 100_000, "observation-px 纵坐标"),
            }), &["type", "x", "y"]),
            object(json!({
                "type": { "const": "key" },
                "keys": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": 8,
                },
            }), &["type", "keys"]),
            object(json!({
                "type": { "const": "text" },
                "text": { "type": "string", "maxLength": 4096 },
            }), &["type", "text"]),
            object(json!({
                "type": { "const": "wait" },
                "ms": integer(1, 1_000, "该步等待毫秒；计入本批 wait 预算"),
            }), &["type", "ms"]),
        ],
    })
}

/// 相对指针动作的封闭集合；不提供跨请求的按钮持有。
fn pointer_step_schema() -> Value {
    let schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))
    .expect("embedded broker schema");
    fn resolve(value: &Value, root: &Value) -> Value {
        if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
            return resolve(
                root.pointer(reference.trim_start_matches('#'))
                    .expect("local schema reference"),
                root,
            );
        }
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|v| resolve(v, root)).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), resolve(v, root)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    resolve(&schema["$defs"]["pointerStep"], &schema)
}

pub(super) fn relative_coordinate_space() -> &'static str {
    "relative-logical-px"
}

/// 长流程执行中单个批次的封闭 schema：一批输入 + 该批自己的 wait 预算。
///
/// 批次只是把「同一次调用内的连续小步」组织起来，不改变单批的输入契约：
/// 每批仍绑定送出当时的最新帧，wait 合计仍须小于该批 timeoutMs。
fn run_batch_schema() -> Value {
    object(
        json!({
            "name": { "type": "string", "maxLength": 80 },
            "steps": {
                "type": "array",
                "items": interaction_step_schema(),
                "minItems": 1,
                "maxItems": MAXIMUM_INTERACTION_STEPS,
            },
            "timeoutMs": integer(1, 30_000, "该批 wait 预算上限，毫秒"),
        }),
        &["steps"],
    )
}

/// 返回全部工具定义；顺序即 `tools/list` 的公开顺序。
pub fn tool_catalog() -> Vec<Value> {
    vec![
        tool(
            "connect",
            "连接本客户端独占的桌面会话；不自动授权、不接管其他客户端。连接后先 observe。authorizationMode=session 表示 connect 处的一次确认覆盖整条会话，之后各工具可省略确认字段；rememberAuthorization=true（仅 Linux Portal）按系统原生 persist 机制记住授权，下次连接自动尝试恢复，可用 computer_authorization action=forget 撤销。",
            object(
                json!({
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "foregroundConsent": consent("用户已同意发送前台输入"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "authorizationMode": {
                        "type": "string",
                        "enum": ["operation", "session"],
                        "description": "operation（缺省）逐操作显式确认；session 一次确认覆盖整条会话"
                    },
                    "rememberAuthorization": consent("用户选择记住授权：跨连接复用同一授权并在下次连接自动尝试恢复（仅 Linux Portal 支持）"),
                    "timeoutMs": integer(10_000, 300_000, "连接超时，毫秒"),
                }),
                &AUTH,
            ),
            false,
        ),
        tool(
            "status",
            "读取当前客户端会话；不启动或授权桌面。",
            object(json!({}), &[]),
            true,
        ),
        tool(
            "observe",
            "截图并直接返回 PNG 图像、frameId 和 observation-px 坐标。返回图坐标可直接用于 interact；先看图确认目标。",
            object(
                json!({
                    "sessionId": session_id_property(),
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "maxDimension": integer(256, 2560, "返回图最长边像素"),
                }),
                &["sessionId"],
            ),
            true,
        ),
        tool(
            "interact",
            format!(
                "直接鼠标移动/点击、快捷键与小批次输入，执行后直接返回截图。键鼠只送达持有焦点的窗口：发键前先点一下目标窗口，别在别的窗口打字。frameId 必须为本会话最新 observe；输入一旦送出旧帧即失效，被预检拒绝则不消耗帧。steps 内 wait 合计必须小于 timeoutMs（默认 3000ms），按键与文本不计入该预算，超限整批拒收。text 是 ASCII 键盘输入，会受输入法影响。{OMITTED_CONSENT}"
            ),
            object(
                json!({
                    "sessionId": session_id_property(),
                    "frameId": frame_id_property(),
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "foregroundConsent": consent("用户已同意发送前台输入"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "steps": {
                        "type": "array",
                        "items": interaction_step_schema(),
                        "minItems": 1,
                        "maxItems": MAXIMUM_INTERACTION_STEPS,
                    },
                    "timeoutMs": integer(1, 30_000, "输入超时，毫秒"),
                    "maxDimension": integer(256, 2560, "返回图最长边像素"),
                }),
                &["sessionId", "frameId", "steps"],
            ),
            false,
        ),
        tool(
            "keys",
            format!(
                "发送完整按键或快捷键（例如 left-shift+f5、numpad-1），随后返回截图。按键只送达持有焦点的窗口，先用 interact 点一下目标窗口；依当前截图核对焦点，不自动切换输入法。{OMITTED_CONSENT}"
            ),
            object(
                json!({
                    "sessionId": session_id_property(),
                    "frameId": frame_id_property(),
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "foregroundConsent": consent("用户已同意发送前台输入"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 8,
                        "uniqueItems": true,
                    },
                    "maxDimension": integer(256, 2560, "返回图最长边像素"),
                }),
                &["sessionId", "frameId", "keys"],
            ),
            false,
        ),
        tool(
            "pointer",
            format!(
                "相对鼠标移动/拖拽/滚轮，随后返回截图。delta 为 relative-logical-px，不是预览像素；先用 interact move 定位起点。每次完整释放。{OMITTED_CONSENT}"
            ),
            object(
                json!({
                    "sessionId": session_id_property(),
                    "frameId": frame_id_property(),
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "foregroundConsent": consent("用户已同意发送前台输入"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "steps": {
                        "type": "array",
                        "items": pointer_step_schema(),
                        "minItems": 1,
                        "maxItems": 64,
                    },
                    "timeoutMs": integer(1, 30_000, "输入超时，毫秒"),
                    "maxDimension": integer(256, 2560, "返回图最长边像素"),
                }),
                &["sessionId", "frameId", "steps"],
            ),
            false,
        ),
        tool(
            "run",
            format!(
                "长流程批量执行：一次调用内由服务端自己完成「取新帧 → 送一批 → 读回新帧」的循环，可连续跑多批，不必每批回来一次。每批仍绑定送出当时的最新帧，绝不复用旧帧；调用方不需要先 observe，也不必逐批给 frameId。出错默认停止后续批次，并回读该批已发生的效果。返回逐批回执与有界关键帧；长流程用本工具，单批小步才用 interact。{OMITTED_CONSENT}"
            ),
            object(
                json!({
                    "sessionId": session_id_property(),
                    "confirmed": consent("用户已确认本次前台桌面操作"),
                    "foregroundConsent": consent("用户已同意发送前台输入"),
                    "strictIsolation": consent("必须为 false：此路线不提供后台隔离"),
                    "batches": {
                        "type": "array",
                        "items": run_batch_schema(),
                        "minItems": 1,
                        "maxItems": 64,
                    },
                    "captureEveryBatches": integer(0, 64, "每 N 批回读一张关键帧；0 表示只回读最后一批"),
                    "maxFrames": integer(1, 8, "返回图像张数上限"),
                    "stopOnError": { "type": "boolean", "description": "出错时停止后续批次，默认 true" },
                    "totalTimeoutMs": integer(1_000, 600_000, "整次调用总预算，毫秒；默认 45 秒"),
                    "maxDimension": integer(256, 2560, "返回图最长边像素"),
                }),
                &["sessionId", "batches"],
            ),
            false,
        ),
        tool(
            "disconnect",
            "关闭指定桌面会话并读回空 sessions，释放本客户端 broker；不关闭被操作的应用。",
            object(
                json!({ "sessionId": session_id_property() }),
                &["sessionId"],
            ),
            false,
        ),
        tool(
            "authorization",
            "查看或撤销本工具记住的桌面授权：action=status 返回是否已保存可恢复授权（不含任何凭据内容）；action=forget 清除本地保存的授权凭据并停止本客户端全部 live 会话。忘记只撤销本工具保存的凭据，系统 Portal 侧的授权记录需在桌面环境权限管理中单独撤销。",
            object(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["status", "forget"],
                        "description": "status 查看脱敏状态；forget 撤销本工具保存的授权"
                    },
                }),
                &["action"],
            ),
            false,
        ),
    ]
}

/// 工具名是否在公开清单内。
pub fn is_known_tool(name: &str) -> bool {
    tool_catalog()
        .iter()
        .any(|tool| tool["name"].as_str() == Some(name))
}

/// 返回工具输入 schema；未知工具返回 `None`。
pub fn input_schema(name: &str) -> Option<Value> {
    tool_catalog()
        .into_iter()
        .find(|tool| tool["name"].as_str() == Some(name))
        .map(|tool| tool["inputSchema"].clone())
}
