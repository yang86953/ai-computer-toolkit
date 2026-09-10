# `window.activate.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.activate.transition@1` 是 UIX Agent 的精确窗口激活与焦点观察输入契约。
输入是严格封闭对象，只允许可选的 `pollIntervalMs`（20–500 毫秒，默认 50）与
`timeoutMs`（100–30000 毫秒，默认 30000）。未知字段、`null`、浮点和越界值均拒绝。

确认与前景同意必须先于 input 读取、opaque target 解析、Agent 连接和任何 provider I/O。
target 只接受精确的 `s2:w:<16 位小写十六进制>` 窗口 generation；Adapter 只 resolve 一次，
随后在同一认证操作连接上先提交一次 `activate_window`，再有界轮询同连接
`list_windows` 的 `focused=true` 观察。该流程只要求 Agent 协商的 `focused` 字段，不使用
Agent `wait` 协议。

成功只证明激活请求已被接受以及返回了焦点观察事实，不声明动作因果、焦点持久最终态或应用
是否消费请求。dispatch 后失去可信观察返回 `OUTCOME_UNKNOWN`，固定禁止自动重试。

公开结果不得包含桌面输入、native/transport 身份或 X11/XWayland/fallback；该能力不提供
compositor 全局窗口控制。当前 `uix-app` 由其他任务修改，本批不写入该项目。
