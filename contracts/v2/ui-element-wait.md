# `ui.element.wait@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.read` surface，在一个总 deadline 和同一认证 UIX Agent 连接内等待
精确窗口 generation 的语义 selector 达到 `unique` 或 `missing`。它复用 `ui.element.locate@2` 的
大小写敏感 exact-AND selector，但不扩写隔离 UIA `ui.element.wait@1` 的轮询、稳定时间或取消语义。

## 输入与条件

输入由 `ui-element-wait-input.schema.json` 冻结。`selector` 至少指定
`automationId`、`role`、`name`、`focused`、`enabled`、`action` 之一；所有字段均为精确 AND
匹配且拒绝 null、空字符串、未知字段和超界 UTF-8 内容。`condition` 只接受：

- `unique`：完整 snapshot 中恰好一个元素匹配；
- `missing`：完整 snapshot 中没有元素匹配。

两个或更多匹配立即返回元素歧义错误，不等待任意候选消失，也不公开 selector 或候选。`timeoutMs`
为 100..30000，默认 30000；本版本没有 `pollIntervalMs`、`stableForMs` 或隐式稳定性声明。

## revision 驱动边界

`uix-app v0.0.2` 的语义 snapshot 变化会推进窗口 revision、更新 Agent 目录并唤醒协议 `wait`。
Adapter 因此只重新解析一次；解析完成后建立一个认证操作连接，先读取并完整验证 snapshot；条件未
满足时在该连接上执行 `revision-after`，收到 changed 后再读取 snapshot。每轮固定 window ID、generation 和 opaque target，
要求 revision/presentedRevision 单调、节点身份唯一、字段与资源边界完整。窗口关闭、代际变化、
协议倒退、超时或不完整 snapshot 均结构化失败。

结果以 `sampleCount` 和 `waitCount` 区分初始快照命中与实际等待；只有 `waitCount` 大于零时
`revisionWaitUsed` 才为 true。唯一匹配返回 snapshot-scoped opaque 元素身份与应用客户区 logical
geometry，missing 返回 null 元素。

成功只证明当前 UIX 语义 snapshot 的零/唯一结论，不证明 compositor 终态、业务效果或跨命令元素
授权。能力只读且可安全重试，不确认 mutation、不请求前景、不注入输入，不读取或公开 value、
selection、完整树、端点、token、PID 或原生身份；不使用 Portal、X11/XWayland、compositor 私有协议、
定时轮询或 fallback。用户真实应用观察验收暂缓。

```text
ai-computer-toolkit run app read --capability ui.element.wait@2 --target sessionId=<s2:w:opaque> --input <file|->
```
