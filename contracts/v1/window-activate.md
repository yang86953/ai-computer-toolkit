# `window.activate@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，向显式启用 UIX Agent 的精确窗口
generation 提交一次前台激活请求。它只覆盖 opt-in UIX 应用，不声明 compositor 全局窗口、
任意第三方窗口或其他窗口管理器控制能力。

输入是严格封闭对象，只允许可选 `timeoutMs`，范围为 `100..30000` 毫秒，默认 `30000` 毫秒。
调用必须同时具备逐操作 `--confirm` 与 `--allow-foreground`；两项确认在读取 input、解析
opaque target、连接 Agent 或执行任何 provider 操作之前完成。

公开 target 必须是当前清单中的精确 `s2:w:<16 位小写十六进制>` 窗口 generation。Adapter 在同一
清单中重新解析该目标，并在后续同一认证连接上确认 Agent 发布 `activate_window`、只提交一次无 target
窗口动作，再尽力读取同一连接、同一 generation 的焦点观察。
`window.discover@3` / `window.metadata.read@3` 同步公开经 hello 协商的 nullable `focused`，供调用方
在不重复激活的前提下重新观察；未协商或缺失事实保持 `null`。

成功只证明 `activationRequestAccepted=true` 且 `outcome=request-accepted`，不证明最终焦点状态。
`focusObservedAfterDispatch` 与 `targetGenerationCurrentAfterDispatch` 都可以是 `true`、`false`
或 `null`；只有两者均为 `true` 时才允许 `focusConfirmed=true`。任一观察值为 `false` 或 `null`
都必须保持 `focusConfirmed=false`，不得把请求提交或缺失观察冒充为已聚焦。结果固定
`finalFocusStateGuaranteed=false`。

该 mutation 固定 `executionRealm=host-foreground` 与 `requiredExecutionRealm=host-foreground`，
结果中的确认和前景同意评估均为 true，`meta.foreground.activationRequested=true` 并包含
`focusConfirmed`。dispatch 后不确定结果不得自动重试，结果固定
`automaticRetryProhibited=true`、`retrySafe=false`。

公共结果只保留 opaque target、请求提交和焦点观察事实；不公开原生窗口身份、传输身份或其他
provider 句柄，不注入桌面输入，不使用 X11/XWayland，也不回退到其他窗口控制路径。用户视觉验收
暂缓是外部验收状态，不是运行时结果字段，也不构成此契约的成功证明。

```text
ai-computer-toolkit run app apply --capability window.activate@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground
```
