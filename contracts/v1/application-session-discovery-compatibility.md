# Application session discovery compatibility v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`application.session.discover@1` 是 `sessions app --max-items 1..4096` 的只读、
provider-neutral 聚合契约。Rust 是生产 launcher 的主实现；C++ 仅保留为反向迁移期的
直接等价对照，launcher 在 Rust runtime 缺失时必须结构化失败，不能回退到 C++。

## 单一聚合事实

Application Facade System 只组合既有 provider 的 `sessions` Module 入口，不建立第二套
provider、session 命名空间或原生目标解析。它必须：

1. 逐 provider 读取公开 session，并递归清除私有前景与原生字段；
2. 以 host、application、document、window、其他的固定类别顺序稳定投影；
3. 在聚合后计算 `total`，再按调用方 `max-items` 截断；
4. 将单 provider 失败降级为只含稳定 `code/message` 的 `warnings`；
5. 记录聚合前后前景事实，不执行任何写操作或前台激活。

成功结果由 `application-session-discovery.schema.json` 约束。顶层兼容字段和 `data`
必须从同一对象生成；`capability`、`readOnly`、`foregroundUnchanged`、`targetIdentity`、
`count`、`total`、`truncated`、`sessions` 与 `warnings` 必须逐字段相等。

## 身份和隐私

公开 session 只使用版本化 opaque `sessionId`。PID、HWND、AUMID、进程或文件路径、
provider ID、COM/UIA/WinRT 类型及 native handle 只能存在于 Component 私有重新发现边界，
不得进入 stdout JSON。每个 session 的 capability 数组仍由其原 provider 发布；聚合层
不能凭类别推断或新增写能力。

每个 `kind=window` session 必须携带 `act/window-target-identity/v1` 的
`targetIdentityStrength`。当前值明确报告完全相同 token 的同进程回收未获保证；该字段不得
由标题、class、style、UIA identity 或重新枚举结果推导为更强保证。

## Rust 契约门禁

Rust 在同一登录会话中使用 `max-items` 时，必须满足成功、只读、前景不变、
计数/截断不变量、顶层与 `data` 同源和禁止字段扫描，并发布唯一 canonical host。
Rust 聚合唯一发布的 host `sessionId` 必须等于两份同次 inventory 的 canonical
`hostTargetId`；该历史对应关系只用于兼容身份解释，不要求运行 C++ 或比较两份动态
session 数组。C++ 可执行文件与 `clang++` 不再参与现行门禁。
