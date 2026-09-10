use crate::{
    // 导入 generic operation 的强类型 action 单一来源。
    capabilities::CapabilityAction,
    catalog::{FieldDescriptor, FieldValueType, OperationDescriptor},
    // 导入动词与 capability-specific 占位执行域。
    domain::{ExecutionRealm, Verb},
};

const APP_SESSION_TARGET: &[FieldDescriptor] = &[
    // 所有 App capability 都绑定原领域目标。
    FieldDescriptor {
        // 使用稳定 sessionId 字段。
        name: "sessionId",
        // 使用封闭 opaque session ID 类型。
        value_type: FieldValueType::OpaqueSessionId,
        // 原目标始终必填。
        required: true,
    },
    // 只有四条严格隔离 mutation 可使用独立会话路由。
    FieldDescriptor {
        // 使用稳定 endpoint 选择字段。
        name: "interactiveSessionId",
        // 同样只接受 canonical opaque session。
        value_type: FieldValueType::OpaqueSessionId,
        // 普通同会话执行不得要求该字段。
        required: false,
    },
];

const APP_CAPABILITY_ARGS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "capability",
        // 使用封闭版本化 capability ID 类型。
        value_type: FieldValueType::VersionedCapabilityId,
        required: true,
    },
    FieldDescriptor {
        name: "input",
        // 使用封闭 JSON 对象类型。
        value_type: FieldValueType::Object,
        required: true,
    },
];

pub(super) const APP_VERBS: &[Verb] = &[Verb::Status, Verb::Sessions, Verb::Inspect, Verb::Run];

// 从强类型 action 与 catalog 自有描述构造全部 generic operation。
pub(super) const APP_OPERATIONS: &[OperationDescriptor] = &[
    // 发现绑定当前用户作用域，不要求尚未发现的目标；其余 v1 动作仍要求精确 session。
    #[cfg(target_os = "linux")]
    app_operation(
        "app.discover",
        CapabilityAction::Discover,
        "Discover bounded provider-neutral sessions through a versioned capability.",
        "guaranteed",
        &["app-api", "ipc"],
    ),
    // 登记统一只读 capability action。
    app_operation(
        // 保持稳定目录 ID。
        "app.read",
        // 从 registry 复用读取 action。
        CapabilityAction::Read,
        // 保持既有公开摘要。
        "Wait for or read provider-neutral state through a versioned capability on an exact session.",
        // 已认证 provider 保证后台执行。
        "guaranteed",
        // 当前只允许隔离可访问性读取方法。
        &["uia-read-isolated-worker"],
    ),
    // 登记领域对象创建 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.create",
        // 从 registry 复用创建 action。
        CapabilityAction::Create,
        // 保持既有公开摘要。
        "Create a domain object through a versioned capability on an exact application session.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &[
            "app-api",
            "command-line",
            "com-automation",
            // 固定浏览器会话 Broker 通过已发布 IPC 方法运行。
            "ipc",
            "file-automation",
        ],
    ),
    // 登记领域 patch action。
    app_operation(
        // 保持稳定目录 ID。
        "app.apply",
        // 从 registry 复用变更 action。
        CapabilityAction::Apply,
        // 保持既有公开摘要。
        "Apply a provider-neutral domain patch to an exact session.",
        // 保持后台优先后显式同意策略。
        "prefer-background-then-consent",
        // 保持既有方法集合。
        &[
            "app-api",
            "com-automation",
            "ipc",
            "uia",
            "win32-message",
            "foreground-input",
        ],
    ),
    // 登记制品保存 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.save",
        // 从 registry 复用保存 action。
        CapabilityAction::Save,
        // 保持既有公开摘要。
        "Save the editable source artifact owned by an exact document session.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &[
            "app-api",
            "command-line",
            "com-automation",
            "file-automation",
        ],
    ),
    // 登记派生制品导出 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.export",
        // 从 registry 复用导出 action。
        CapabilityAction::Export,
        // 保持既有公开摘要。
        "Export a derived artifact without changing the source session identity.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &[
            "app-api",
            "command-line",
            "com-automation",
            "file-automation",
        ],
    ),
    // 登记精确 session 关闭 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.close",
        // 从 registry 复用关闭 action。
        CapabilityAction::Close,
        // 保持既有公开摘要。
        "Close an exact session through a versioned lifecycle capability.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &["app-api", "com-automation", "ipc"],
    ),
    // 登记精确窗口截图 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.screenshot",
        // 从 registry 复用截图 action。
        CapabilityAction::Screenshot,
        // 保持既有公开摘要。
        "Capture the exact window represented by an opaque app session.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &[
            "windows-graphics-capture",
            "headless-browser",
            // Linux opt-in UIX 应用只读取自身 surface。
            "uix-application-surface",
        ],
    ),
    // 登记精确窗口录制 action。
    app_operation(
        // 保持稳定目录 ID。
        "app.record",
        // 从 registry 复用录制 action。
        CapabilityAction::Record,
        // 保持既有公开摘要。
        "Record an exact window and produce a bounded storyboard for AI analysis.",
        // 保持后台优先策略。
        "best-effort",
        // 保持既有方法集合。
        &["windows-graphics-capture", "media-foundation-h264"],
    ),
];

// 用强类型 action 构造 App facade 统一 operation 描述。
const fn app_operation(
    // 接收 catalog 自有稳定 ID。
    id: &'static str,
    // 接收 capability registry 拥有的 action 语义。
    action: CapabilityAction,
    // 接收 catalog 自有公开摘要。
    summary: &'static str,
    // 接收 catalog 自有背景执行策略。
    background_policy: &'static str,
    // 接收 catalog 自有实现方法集合。
    methods: &'static [&'static str],
) -> OperationDescriptor {
    // 返回保持公开形状的静态描述。
    OperationDescriptor {
        // 输出调用方提供的稳定目录 ID。
        id,
        // generic verb 只从强类型 action 投影。
        operation: action.as_str(),
        // 输出 catalog 自有摘要。
        summary,
        // 只有只读 discover 是空目标，其余动作保留原精确 opaque session 门禁。
        target_fields: if matches!(action, CapabilityAction::Discover) {
            &[]
        } else {
            APP_SESSION_TARGET
        },
        // 所有 App generic operation 都复用 capability + input 包装。
        argument_fields: APP_CAPABILITY_ARGS,
        // 副作用事实只从强类型 action 推导。
        mutates: action.mutates(),
        // mutation 确认事实与同一 action 保持一致。
        requires_confirmation: action.mutates(),
        // 发现不能要求调用者预先持有被发现的 session。
        requires_session_id: !matches!(action, CapabilityAction::Discover),
        // 输出 catalog 自有背景策略。
        background_policy,
        // 实际 realm 必须在运行时按 args.capability 解析。
        execution_realm: ExecutionRealm::None,
        // 输出 catalog 自有方法集合。
        methods,
    }
}

// 覆盖 App catalog 与 capability registry 的单一来源门禁。
#[cfg(test)]
mod tests {
    // 导入父模块静态描述与构造事实。
    use super::*;
    // 导入 capability registry 与 surface。
    use crate::capabilities;
    // 导入 JSON 构造器以核对公开字段形状。
    use serde_json::json;

    // 验证 App registry action 与 catalog operation 双向完全覆盖。
    #[test]
    fn app_operations_match_registry_action_projection() {
        // 从 registry 取得规范 action verb 顺序。
        let registry_verbs = capabilities::action_verbs_for_surface(
            // 只投影 App surface。
            capabilities::CapabilitySurface::App,
        );
        // 从 catalog 收集实际 operation 顺序。
        let catalog_verbs = APP_OPERATIONS
            // 遍历静态 operation。
            .iter()
            // 只提取规范 verb。
            .map(|operation| operation.operation)
            // 收集为稳定列表。
            .collect::<Vec<_>>();
        // 双向集合和规范顺序必须完全相同。
        assert_eq!(catalog_verbs, registry_verbs);
        // 保存已见 operation 以拒绝重复。
        let mut seen = std::collections::BTreeSet::new();
        // 逐项核对 registry 分发定义与派生事实。
        for operation in APP_OPERATIONS {
            // 每个 catalog operation 必须唯一。
            assert!(seen.insert(operation.operation));
            // 收集使用当前 action verb 的精确 App capability 定义。
            let definitions = capabilities::ALL
                // 遍历全部 registry 定义。
                .iter()
                // 只保留 App surface。
                .filter(|definition| definition.surface == capabilities::CapabilitySurface::App)
                // 只保留当前规范 action。
                .filter(|definition| definition.action.as_str() == operation.operation)
                // 收集借用定义供多项核对。
                .collect::<Vec<_>>();
            // 每个 advertised operation 至少有一个真实 capability 分发定义。
            assert!(!definitions.is_empty());
            // 所有同 action 定义必须同意副作用事实。
            assert!(
                definitions
                    // 遍历同 action 定义。
                    .iter()
                    // 核对 catalog 派生值。
                    .all(|definition| definition.action.mutates() == operation.mutates)
            );
            // 逐操作确认必须与同一 action 副作用保持一致。
            assert_eq!(operation.requires_confirmation, operation.mutates);
            // 稳定 catalog ID 必须与规范 verb 一致。
            assert_eq!(operation.id, format!("app.{}", operation.operation));
        }
    }

    // 验证 catalog 自有字段和值在改用构造 helper 后保持不变。
    #[test]
    fn app_operation_catalog_owned_values_remain_compatible() {
        // 定义改造前的 ID、摘要、背景策略和方法集合。
        let expected: &[(&str, &str, &str, &[&str])] = &[
            // 锁定只读 operation 自有事实。
            (
                "app.read",
                "Wait for or read provider-neutral state through a versioned capability on an exact session.",
                "guaranteed",
                &["uia-read-isolated-worker"],
            ),
            // 锁定创建 operation 自有事实。
            (
                "app.create",
                "Create a domain object through a versioned capability on an exact application session.",
                "best-effort",
                &[
                    "app-api",
                    "command-line",
                    "com-automation",
                    // 固定浏览器会话 Broker 通过已发布 IPC 方法运行。
                    "ipc",
                    "file-automation",
                ],
            ),
            // 锁定变更 operation 自有事实。
            (
                "app.apply",
                "Apply a provider-neutral domain patch to an exact session.",
                "prefer-background-then-consent",
                &[
                    "app-api",
                    "com-automation",
                    "ipc",
                    "uia",
                    "win32-message",
                    "foreground-input",
                ],
            ),
            // 锁定保存 operation 自有事实。
            (
                "app.save",
                "Save the editable source artifact owned by an exact document session.",
                "best-effort",
                &[
                    "app-api",
                    "command-line",
                    "com-automation",
                    "file-automation",
                ],
            ),
            // 锁定导出 operation 自有事实。
            (
                "app.export",
                "Export a derived artifact without changing the source session identity.",
                "best-effort",
                &[
                    "app-api",
                    "command-line",
                    "com-automation",
                    "file-automation",
                ],
            ),
            // 锁定关闭 operation 自有事实。
            (
                "app.close",
                "Close an exact session through a versioned lifecycle capability.",
                "best-effort",
                &["app-api", "com-automation", "ipc"],
            ),
            // 锁定截图 operation 自有事实。
            (
                "app.screenshot",
                "Capture the exact window represented by an opaque app session.",
                "best-effort",
                &[
                    "windows-graphics-capture",
                    "headless-browser",
                    "uix-application-surface",
                ],
            ),
            // 锁定录制 operation 自有事实。
            (
                "app.record",
                "Record an exact window and produce a bounded storyboard for AI analysis.",
                "best-effort",
                &["windows-graphics-capture", "media-foundation-h264"],
            ),
        ];
        // 新 discovery 不改变既有 operation 的数量、顺序、字段或确认要求。
        let legacy = APP_OPERATIONS
            .iter()
            .filter(|operation| operation.operation != "discover")
            .collect::<Vec<_>>();
        assert_eq!(legacy.len(), expected.len());
        // 逐项核对 catalog 自有事实与共享字段形状。
        for (operation, (id, summary, background_policy, methods)) in
            legacy.into_iter().zip(expected)
        {
            // ID 保持不变。
            assert_eq!(operation.id, *id);
            // 摘要保持不变。
            assert_eq!(operation.summary, *summary);
            // 背景策略保持不变。
            assert_eq!(operation.background_policy, *background_policy);
            // 方法集合和顺序保持不变。
            assert_eq!(operation.methods, *methods);
            // 全部 generic operation 继续要求精确 session。
            assert!(operation.requires_session_id);
            // 实际执行域继续在运行时按 capability 解析。
            assert_eq!(operation.execution_realm, ExecutionRealm::None);
            // 目标字段保持原 opaque session 并增加可选独立会话路由。
            assert_eq!(
                json!(operation.target_fields),
                json!([
                    {
                        "name": "sessionId",
                        "value_type": "opaque-session-id",
                        "required": true
                    },
                    {
                        "name": "interactiveSessionId",
                        "value_type": "opaque-session-id",
                        "required": false
                    }
                ])
            );
            // 参数字段保持 capability + input 包装。
            assert_eq!(
                json!(operation.argument_fields),
                json!([
                    {
                        "name": "capability",
                        "value_type": "versioned-capability-id",
                        "required": true
                    },
                    {
                        "name": "input",
                        "value_type": "object",
                        "required": true
                    }
                ])
            );
        }
    }
}
