# Semantic Action Worker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ai-computer-toolkit-semantic-action-worker.exe` 是 Semantic Action Module 唯一允许启动的
固定 sibling companion。它使用单行 `act/semantic-action-worker/v1` JSON over stdio，
由 `worker_process` 在首条指令前绑定 `KILL_ON_JOB_CLOSE` Job；不接受命令行参数，不写
stderr，不成为独立公共控制面。

请求固定为 `operation=semantic-element-action`，并包含原 canonical `s2:w:*`、
provider-neutral selector、五种封闭 action、`maximumDepth`、`maximumItems` 和
`view=control|raw`。parent deadline 不传给 provider；由 Job 生命周期在 worker 外控制。
请求上限 128 KiB，输出上限 64 KiB。

worker 在 MTA 中重新枚举可见有标题窗口并唯一解析 opaque ID，再以有界 TreeWalker 完整
搜索 selector。两个匹配立即返回 `AMBIGUOUS_TARGET`；零或唯一结论若遇到属性缺口、深度/
数量截断或遍历失败则返回 `SEARCH_INCOMPLETE`。只有完整唯一匹配可继续检查 enabled 和
对应 pattern。模式不存在或 Value 只读返回 `ACTION_UNSUPPORTED`，不查询 ClickablePoint、
不发键鼠、不调用 SetFocus，也不接收或输出 snapshot element ID。

worker 对 Invoke、SetValue、Toggle、Select、Scroll 中恰好一个方法发起一次调用。方法
返回成功才输出 completed data；方法调用后的任何 provider 错误都返回带固定不可重试
details 的 `OUTCOME_UNKNOWN`。不得按 HRESULT 推断未发生，不得 retry，不得 fallback。

成功 envelope 只有 `ok`、`contractVersion`、`data`；失败 envelope 只有 `ok`、
`contractVersion`、`error={code,message,details}`。公开前由 parent 逐字段白名单验证；
任何未知字段、原生身份、未知错误码、退出码冲突或协议漂移都按可能已 dispatch 处理为
`OUTCOME_UNKNOWN`。
