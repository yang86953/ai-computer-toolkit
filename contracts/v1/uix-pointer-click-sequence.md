# `ui.input.pointer.click.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证的 UIX 应用内对一个精确窗口
generation 提交一次请求级普通左键点击序列。它运行在 `same-session-no-focus`，逐操作确认
必需，且不需要也禁止前景同意：调用方必须提供 `--confirm`，不得提供
`--allow-foreground`。它只接受 opt-in UIX Agent，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-pointer-click-sequence-input.schema.json` 冻结，严格封闭，只接受
`coordinateSpace`、`clicks`、`intervalMs` 与 `timeoutMs`：

- `coordinateSpace` 必须为 `client-logical-px`；
- `clicks` 必须包含 `1..64` 个只含 finite 数值 `x`、`y` 的客户区 logical px 点，每个坐标
  在 `0..65535`；
- `intervalMs` 为 `0..500` 毫秒，默认 `0`；
- `timeoutMs` 为 `100..30000` 毫秒，默认 `30000`。

计划时长固定为 `(clicks.length - 1) * intervalMs`，必须不超过 `5000` 毫秒；Rust 解析器
还要求 `timeoutMs >= plannedDurationMs + 100`。未知字段、空序列、非法点、其他坐标空间和
越界参数都在目标解析及 Agent 连接前失败。每个 click 只表示一次普通左键配对，不公开独立
按钮所有权、`PointerDoubleClick`、click count、时间聚合、事务或回滚语义。

## Agent 预检与有序执行

UIX Agent v0.0.2 的 `click_at` 只派发普通左键 `PointerDown` + `PointerUp`，不声明双击或
点击计数。所有点击必须在首个 dispatch 前预检 hello 的 `click_at` 能力；Adapter 在同一条
已认证连接上固定 window generation，并沿当前 revision 链按顺序发送每个 click，点击间隔
只受有界 `intervalMs` 约束。该顺序不是时间聚合、事务或可回滚操作。

只要任一 dispatch 已开始，后续发生传输失败、点击未被完整接受、revision 链断裂、deadline
超时、连接丢失或终态不可信，都必须返回 `OUTCOME_UNKNOWN`，并标记
`acceptedMayHaveOccurred=true`、`automaticRetryProhibited=true` 与 `retrySafe=false`。
调用方不得自动重试，也不得声称点击已回滚或全部被消费。任何首个 dispatch 尚未开始前的确认、
解析、目标 stale、策略拒绝或 Agent 能力缺口，都必须结构化失败并保持 `accepted=false`，
同时不得冒充部分执行。

## 成功结果与安全边界

成功结果由 `uix-pointer-click-sequence-result.schema.json` 冻结：envelope 固定为 `app.apply`、
`ui.input.pointer.click.sequence@1`、精确 `s2:w:<16 位小写十六进制>` target、
`executionRealm=requiredExecutionRealm=same-session-no-focus`。data 固定
`action=click-sequence`、provider-neutral `clicks`、`clicksRequested`、`clicksAccepted`、
`intervalMs`、`plannedDurationMs`、`outcome=completed`、`dispatchState=completed`、
`accepted=true`、`allClicksBalanced=true`、`doubleClickSemantics=false`、
`clickCountSemantics=false`，并携带最终 `revision`、`presentedRevision`、`settled`。

成功只证明这些普通左键 click_at 请求按 revision 链提交并获得 dispatch 接受事实，不证明目标
应用消费了点击或达到最终 UI 效果；因此 `effectConfirmed=false`、`finalStateReached=false`。
结果中的 `windowReResolved`、`applicationPolicyEvaluated` 与确认评估均为 `true`，
`foregroundConsentRequired=false`、`hostForegroundActivationRequested=false`，并固定
`automaticRetryProhibited=true`、`retrySafe=false`。

安全字段固定声明 provider 为 `uix-agent-v1`，仅允许应用内部事件：
`applicationInternalEventsDispatched=true`、`desktopInputInjected=false`、
`desktopPointerMoved=false`、`requestScopedBalancedClicks=true`、
`independentButtonOwnership=false`、`doubleClickSupported=false`、`clickCountSupported=false`、
native/transport identity 均为 `false`、`x11Used=false`、`fallback=none`。公开结果不得泄露
原生窗口身份、传输连接身份或 provider 句柄，不注入桌面输入，不使用 X11/XWayland，也不回退
到其他输入路径。用户真实交互验收暂缓属于外部验收状态，不是运行时字段，也不是此 capability
的成功证明。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer.click.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
