# `ui.input.pointer.drag@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证的 UIX 应用内对一个精确窗口
generation 提交一次固定左键原子拖拽。它运行在 `same-session-no-focus`，逐操作确认必需，
不需要也禁止前景同意：调用方必须提供 `--confirm`，不得提供 `--allow-foreground`。
它只接受 opt-in UIX Agent，不控制任意第三方窗口或宿主桌面。

## 输入契约

正式输入由 `uix-pointer-drag-input.schema.json` 冻结，严格封闭，只允许以下字段：

- `coordinateSpace` 必须为 `client-logical-px`；
- `start` 与 `end` 必须各含 finite 的数值 `x`、`y`，每个坐标在 `0..65535`；
- `samples` 为 `1..64`，默认 `12`；
- `durationMs` 为 `0..5000`，默认 `250`；
- `timeoutMs` 为 `100..30000`，默认 `30000`。

Rust 解析器还要求 `timeoutMs >= durationMs + 100`，并再次拒绝非 finite 坐标。未知字段、
独立 `button`、`pointer_down`、`pointer_move` 或 `pointer_up` 请求以及其他坐标空间均在
目标解析和 Agent 连接前失败。公开输入只表达一个固定左键拖拽，不公开可跨请求持有的按钮，
也不公开独立 down/up capability。

## Agent 认证与原子执行

Agent hello 必须在接受该 capability 前预先同时发布 `pointer_down`、`pointer_move`、
`pointer_up` 三个窗口动作；只发布其中一部分不得冒充支持拖拽。Adapter 在同一条已认证连接
上按 `pointer_down` → 有界插值 `pointer_move` 序列 → `pointer_up` 的顺序执行，插值数量受
`samples` 限制，持续时间受 `durationMs` 限制。按钮所有权只属于当前请求并在请求内配平。

`pointer_down` 一旦被接受，之后任何移动、响应、deadline 或连接失败都必须尽力发送同一请求
的 `pointer_up`。释放未被确认时返回 `OUTCOME_UNKNOWN`，固定 `automaticRetryProhibited=true`
与 `retrySafe=false`；调用方不得自动重试，也不能把未确认释放解释为已配平。down 后的任何
部分执行都不能返回成功的 completed 结果。

任何 dispatch 尚未开始前的失败都必须结构化失败，并明确 `acceptedMayHaveOccurred=false`；
不得把未 dispatch 的解析、目标 stale、策略拒绝、确认缺失或 Agent 能力缺口冒充为部分执行。
一旦 dispatch 已开始而最终状态无法可靠判断，则使用 `OUTCOME_UNKNOWN`，不得静默回滚或重试。

## 成功结果与安全边界

成功结果由 `uix-pointer-drag-result.schema.json` 冻结：envelope 固定为 `app.apply`、
`ui.input.pointer.drag@1`、精确 `s2:w:<16 位小写十六进制>` target、
`executionRealm=requiredExecutionRealm=same-session-no-focus`。data 固定
`outcome=completed`、`dispatchState=completed`、`accepted=true`，并要求 down、move、up
均已按本次请求完成：`pointerDownAccepted=true`、`pointerUpAccepted=true`、
`buttonReleaseConfirmed=true`、`requestScopedBalancedButtons=true`。结果同时携带
`samplesRequested`、`moveSamplesAccepted`、`durationMs` 以及最终 `revision`、
`presentedRevision`、`settled`。

成功只证明应用内拖拽请求已按协议提交并得到这些 dispatch 接受事实，不证明目标应用消费了
拖拽或产生了最终 UI 效果；因此 `effectConfirmed=false`、`finalStateReached=false`。结果中的
`windowReResolved`、`applicationPolicyEvaluated` 与确认评估均为 `true`，
`foregroundConsentRequired=false`、`hostForegroundActivationRequested=false`，并固定
`automaticRetryProhibited=true`、`retrySafe=false`。

安全字段固定声明 provider 为 `uix-agent-v1`，仅允许应用内部事件：
`applicationInternalEventsDispatched=true`、`desktopInputInjected=false`、
`desktopPointerMoved=false`、native/transport identity 均为 `false`、`x11Used=false`、
`fallback=none`。公开结果不得泄露原生窗口身份、传输连接身份或 provider 句柄，不注入桌面
输入，不使用 X11/XWayland，也不回退到其他输入路径。用户视觉验收暂缓属于外部验收状态，
不是运行时字段，也不是拖拽成功的证明。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer.drag@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
