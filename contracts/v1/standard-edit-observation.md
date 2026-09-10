# Standard Edit read-only observation

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`sessions win32-control` 只观察当前桌面会话中由系统公开窗口目录暴露的标准
`Edit` 控件。它不读取文本内容、不发送窗口消息、不激活窗口，也不返回 HWND、PID、
class name 或其他原生标识。目标统一为进程生命周期绑定的 `s2:c:<opaque>`。

`inspect win32-control --target sessionId=<opaque>` 必须重新枚举并精确解析目标；
目标消失或身份变化返回 `STALE_SESSION`。两条入口都验证操作前后前台不变。

公开结果的 capability 元数据描述 mutation 风险与已认证范围：
`availability` 固定为 `available`，`requiresConfirmation` 固定为 `true`。
`run win32-control set-text` 和 `run app apply` 只对重新解析成功、静态权限允许的
`s2:c:*` 精确目标执行；缺少确认、目标 stale、权限受阻或关系未知均结构化拒绝。

每个会话包含只读 `assessment`：

- 同级或较低完整性只报告 `requires-confirmation`，不声称已经可安全执行；
- 较高完整性目标报告 `permission-blocked`；
- 目标元数据本身因权限不可读也报告 `permission-blocked`；
- 元数据不可得时报告 `indeterminate`；
- `safeToExecuteNow:false` 与 `activeWriteProbePerformed:false` 固定成立。

该分类只读取公开进程令牌元数据，不以试发 `WM_SETTEXT` 探测权限。

纯 Component 的回归矩阵穷举三种元数据访问状态与四种完整性关系的十二个组合；公开
Rust 集成测试还会逐项核对 `sessions win32-control` assessment，并验证 `status` 的三类
计数之和等于 `controlCount`、`activeWriteProbes` 为零。测试只读取安全投影，不公开或
持久化 token、SID、完整性 RID、PID 或 HWND。

Rust 只读门禁比较安全归一化事实
`lowercase(applicationName/processName) + visible`，不公开 HWND/PID，也不以历史
C++ 集合 Jaccard 冒充正确性。写入行为由 Rust 自有 fixture 独立验证。
