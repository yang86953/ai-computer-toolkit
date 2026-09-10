# Protected desktop and permission boundary contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`GC-SEC-001` 将交互会话安全姿态与目标静态权限统一为失败闭合的
Permission Assessment。公开控制范围仍只包括同一用户、同一或更低完整性级别、活动且
未锁定的 Windows 交互会话；本契约不提供提权、跨会话、受保护桌面、反作弊、沙箱或
Session 0 绕过能力。

## 所有权与调用顺序

- ComputerControlSystem 只协调：先执行无目标副作用的 Policy，再请求 Permission
  Assessment；只有取得许可后才解析 provider 或读取、捕获、写入精确目标。
- Permission Boundary Module 拥有授权结论、稳定公开错误和零目标访问证据。
- Windows Security Context Adapter 只读取得当前进程会话状态、进程桌面与输入桌面的
  等价关系；原生 handle、桌面名称、SID、token、路径和安全描述符不得离开 Adapter。
- Static Permission Assessment Component 继续拥有目标进程 metadata 与相对完整性矩阵；
  领域 Module 必须在 dispatch 前消费其结论，不能自行放宽。

确认、前台同意和严格隔离门禁不能覆盖本安全结论。安全姿态在平台上不可探测时必须返回
不确定结果，不能假设当前桌面可写；错误后不得尝试另一 provider、前台输入、注入、调试
权限或软件专用协议。

## 主机安全姿态矩阵

所有精确目标读取和 mutation 都必须先通过下表。`sessions`、`status` 和不产生精确写目标
的有界发现可继续用于诊断，但它们不能授权后续控制。

| 私有事实 | 公开错误 | 稳定原因 |
| --- | --- | --- |
| 活动非零交互会话，进程桌面就是当前输入桌面 | 允许继续 | `active-interactive-default-desktop` |
| 当前进程位于 Session 0 | `PERMISSION_DENIED` | `session-zero` |
| 当前登录会话不是 active | `PERMISSION_DENIED` | `session-not-active` |
| 输入桌面不可访问，或进程桌面不是输入桌面 | `PERMISSION_DENIED` | `protected-or-non-input-desktop` |
| 会话状态无法确定 | `CAPABILITY_ASSESSMENT_UNAVAILABLE` | `session-state-indeterminate` |
| 桌面状态无法确定 | `CAPABILITY_ASSESSMENT_UNAVAILABLE` | `desktop-state-indeterminate` |

UAC 安全桌面、锁屏和工具自有非输入测试桌面统一落入
`protected-or-non-input-desktop`。契约故意不公开更细的原生桌面名称，避免泄漏安全上下文，
也避免把启发式识别误报成权限事实。

失败详情必须满足 `security-boundary-error.schema.json`：明确本次门禁发生在目标访问前，
`targetReadAttempted=false`、`targetWriteAttempted=false`，且 elevation、injection 和 fallback
均未尝试。前置失败没有 mutation，因此 `retrySafe=true`；但状态变化只能由调用方重新发起
并重新评估，`automaticRetryProhibited=true`。

## 目标静态权限矩阵

取得主机姿态许可不代表目标可写。精确目标重新解析后，领域 Module 仍必须执行无主动写
探针的静态权限矩阵：

| metadata | 相对完整性 | 执行结论 |
| --- | --- | --- |
| `available` | `lower` / `same` | 仅在 capability 自身确认和前台规则满足后继续 |
| `available` | `higher` | `PERMISSION_DENIED`，原因 `target-higher-integrity` |
| `available` | `unknown` | `CAPABILITY_ASSESSMENT_UNAVAILABLE` |
| `permission-blocked` | 任意 | `PERMISSION_DENIED`，原因 `target-metadata-permission-blocked` |
| `unavailable` | 任意 | `CAPABILITY_ASSESSMENT_UNAVAILABLE` |

进程终止还必须拒绝当前工具、PID 0/4、其他会话和 Windows critical process。其他领域不得
为了覆盖高完整性、反作弊、沙箱或独占输入而新增 `runas`、token 调整、调试附着、远程
线程、进程内存写入、全局 hook、驱动或任意消息/脚本入口。

## 验证边界

自动验证使用纯矩阵、合成 probe 和工具自有非输入桌面 fixture；不得触发真实 UAC、锁屏、
安全桌面或第三方反作弊。生产 launcher 必须在工具自有非输入桌面内返回上述结构化错误，
且不会读取或写入测试目标。真实机器视觉与交互验收仍由 `yang86` 独占，本契约和自动夹具
不能替代其结论。
