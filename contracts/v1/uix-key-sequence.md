# `ui.input.key.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证的 UIX 应用内对一个精确窗口
generation 提交一次完整、请求级配平的按键序列。它运行在 `same-session-no-focus`，逐操作确认
必需，且不需要也禁止前景同意：调用方必须提供 `--confirm`，不得提供
`--allow-foreground`。它只接受 opt-in UIX Agent，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-key-sequence-input.schema.json` 冻结，严格封闭，只接受 `presses`、
`intervalMs` 与 `timeoutMs`：

- `presses` 必须包含 `1..64` 个可复用的 `UixKeyPress`，每项为 provider-neutral `key` 和
  可选、无重复的 `modifiers`；键名和修饰键沿用 UIX 按键契约的唯一白名单；
- `intervalMs` 为 `0..500` 毫秒，默认 `0`；
- `timeoutMs` 为 `100..30000` 毫秒，默认 `30000`。

计划时长固定为 `(presses.length - 1) * intervalMs`，必须不超过 `5000` 毫秒；Rust 解析器
还要求 `timeoutMs >= plannedDurationMs + 100`。未知字段、空序列、非法键名、非法修饰键、
重复修饰键和越界参数都在目标解析及 Agent 连接前失败。输入只表达完整 press 序列，不公开
独立 `down`、`up`、`hold`、`repeat` 或 `text`。

## Agent 预检与有序执行

首个 dispatch 前，Agent hello 必须同时预检并发布 `perform`、`press_key`、全部允许的
`key_names` 与全部允许的 `key_modifiers`；缺少任一能力不得把序列拆成部分可用能力。Adapter
在同一条已认证连接上按当前 window `revision` 链逐项发送完整成对的 `press_key`，相邻项之间
只等待有界 `intervalMs`，不把按键所有权跨请求保存。

只要任一 dispatch 已开始，后续发生传输失败、按键未被完整接受、revision 链断裂、deadline
超时、连接丢失或终态不可信，都必须返回 `OUTCOME_UNKNOWN`，并标记
`acceptedMayHaveOccurred=true`、`automaticRetryProhibited=true` 与 `retrySafe=false`。
调用方不得自动重试，也不得声称序列已回滚或全部消费。任何首个 dispatch 尚未开始前的确认、
解析、目标 stale、策略拒绝或 Agent 能力缺口，都必须结构化失败并保持
`acceptedMayHaveOccurred=false`，不得冒充部分执行。

## 成功结果与安全边界

成功结果由 `uix-key-sequence-result.schema.json` 冻结：envelope 固定为 `app.apply`、
`ui.input.key.sequence@1`、精确 `s2:w:<16 位小写十六进制>` target、
`executionRealm=requiredExecutionRealm=same-session-no-focus`。data 固定
`action=key-sequence`、provider-neutral `presses`、`pressesRequested`、`pressesAccepted`、
`intervalMs`、`plannedDurationMs`、`outcome=completed`、`dispatchState=completed`、
`accepted=true` 与 `allPressesBalanced=true`，并携带最终 `revision`、`presentedRevision`、
`settled`。

成功只证明完整 press 序列已按协议提交并获得 dispatch 接受事实，不证明目标应用消费了按键或
达到最终 UI 效果；因此 `effectConfirmed=false`、`finalStateReached=false`。结果中的
`windowReResolved`、`applicationPolicyEvaluated` 与确认评估均为 `true`，
`foregroundConsentRequired=false`、`hostForegroundActivationRequested=false`，并固定
`automaticRetryProhibited=true`、`retrySafe=false`。

安全字段固定声明 provider 为 `uix-agent-v1`，仅允许应用内部事件：
`applicationInternalEventsDispatched=true`、`desktopInputInjected=false`、
`requestScopedBalancedPresses=true`、`textInputSupported=false`、`keyHoldSupported=false`、
`keyRepeatSupported=false`、native/transport identity 均为 `false`、`x11Used=false`、
`fallback=none`。公开结果不得泄露原生窗口身份、传输连接身份或 provider 句柄，不注入桌面
输入，不使用 X11/XWayland，也不回退到其他输入路径。用户真实交互验收暂缓属于外部验收状态，
不是运行时字段，也不是此 capability 的成功证明。

```text
ai-computer-toolkit run app apply --capability ui.input.key.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
