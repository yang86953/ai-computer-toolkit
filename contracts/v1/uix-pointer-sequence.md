# `ui.input.pointer.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证的 UIX 应用内对一个精确窗口
generation 提交一次混合悬停—点击序列。它运行在 `same-session-no-focus`，逐操作确认必需，
且不需要也禁止前景同意：调用方必须提供 `--confirm`，不得提供 `--allow-foreground`。
它只接受 opt-in UIX Agent，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-pointer-sequence-input.schema.json` 冻结，严格封闭，只接受
`coordinateSpace`、`steps`、`intervalMs` 与 `timeoutMs`：

- `coordinateSpace` 必须为 `client-logical-px`；
- `steps` 必须有 `2..64` 项，每项只含 `type`、`x`、`y`，其中 `type` 只能是 `move` 或
  `click`，坐标必须 finite 且在 `0..65535`；
- 整个序列必须至少含一个 `move` 与一个 `click`；
- `intervalMs` 为 `0..500` 毫秒，默认 `0`；
- `timeoutMs` 为 `100..30000` 毫秒，默认 `30000`。

计划时长固定为 `(steps.length - 1) * intervalMs`，必须不超过 `5000` 毫秒；Rust 解析器
还要求 `timeoutMs >= plannedDurationMs + 100`。未知字段、空类型、缺失坐标、其他坐标空间和
越界参数都在目标解析及 Agent 连接前失败。该 Component 复用 `UixPointerCoordinateSpace`
与 `UixPointerPoint`，不复制坐标 finite/范围校验。

## Agent 预检与有序执行

Agent hello 必须在首个 dispatch 前同时预检并发布 `pointer_move` 与 `click_at`；缺少任一动作
不得把序列拆成部分可用能力。Adapter 在同一条已认证连接上固定目标 window generation，并
沿同一 revision 链按顺序执行 provider-neutral 步骤：`move` 只映射为 `pointer_move`，普通
左键 `click` 只映射为 `click_at`，后者由 UIX Agent v0.0.2 派发配对的
`PointerDown` + `PointerUp`。序列不取得独立按钮所有权。

该 capability 不声明 drag、`PointerDoubleClick`、double click、click count、滚轮、其他按钮、
事务、回滚或时间聚合语义；不暴露独立 down/up/hold/repeat/text，也不把应用内事件转换成桌面
输入或桌面指针移动。

只要任一 dispatch 已开始，后续发生传输失败、步骤未被完整接受、generation/revision 链断裂、
deadline 超时、连接丢失或终态不可信，都必须返回 `OUTCOME_UNKNOWN`，并标记
`acceptedMayHaveOccurred=true`、`automaticRetryProhibited=true` 与 `retrySafe=false`。
调用方不得自动重试，也不得声称序列已回滚或全部被消费。任何首个 dispatch 尚未开始前的确认、
解析、目标 stale、策略拒绝或 Agent 能力缺口，都必须结构化失败并保持 `accepted=false`，
不得冒充部分执行。

## 成功结果与安全边界

成功结果由 `uix-pointer-sequence-result.schema.json` 冻结：envelope 固定为 `app.apply`、
`ui.input.pointer.sequence@1`、精确 `s2:w:<16 位小写十六进制>` target、
`executionRealm=requiredExecutionRealm=same-session-no-focus`。data 固定
`action=pointer-sequence`、provider-neutral `steps`、`stepsRequested`、`stepsAccepted`、
`movesAccepted`、`clicksAccepted`、`intervalMs`、`plannedDurationMs`、
`outcome=completed`、`dispatchState=completed`、`accepted=true`、
`allClicksBalanced=true`、`doubleClickSemantics=false`、`clickCountSemantics=false`，并公开
`sameConnectionRevisionChain=true` 以及最终 `revision`、`presentedRevision`、`settled`。

成功只证明混合步骤已在同一认证连接、固定 generation 与连续 revision 链上获得 dispatch 接受
事实，不证明目标应用消费了事件或达到最终 UI 效果；因此 `effectConfirmed=false`、
`finalStateReached=false`。结果中的 `windowReResolved`、`applicationPolicyEvaluated` 与确认
评估均为 `true`，`foregroundConsentRequired=false`、`hostForegroundActivationRequested=false`，
并固定 `automaticRetryProhibited=true`、`retrySafe=false`。

安全字段固定声明 provider 为 `uix-agent-v1`，仅允许应用内部事件：
`applicationInternalEventsDispatched=true`、`desktopInputInjected=false`、
`desktopPointerMoved=false`、`sameConnectionUsed=true`、`windowGenerationFixed=true`、
`revisionChainUsed=true`、`requestScopedBalancedClicks=true`；`independentButtonOwnership`、
`dragSupported`、`doubleClickSupported`、`clickCountSupported`、`scrollSupported` 与 native/transport
identity 均为 `false`，并固定 `x11Used=false`、`fallback=none`。公开结果不得泄露原生窗口身份、传输连接身份
或 provider 句柄，不注入桌面输入，不移动桌面指针，不使用 X11/XWayland，也不回退到其他输入
路径。用户真实交互验收暂缓属于外部验收状态，不是运行时字段，也不是此 capability 的成功证明。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
