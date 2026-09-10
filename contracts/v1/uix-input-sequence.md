# `ui.input.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在一个已认证 UIX 窗口的同一连接、固定 generation
与连续 revision 链中交错提交完整按键和应用内指针动作。它运行在 `same-session-no-focus`，必须
逐操作确认，且禁止前景同意。它只接受 opt-in UIX Agent，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-input-sequence-input.schema.json` 冻结，只接受 `coordinateSpace`、`steps`、
`intervalMs` 与 `timeoutMs`。`coordinateSpace` 固定为 `client-logical-px`；`steps` 必须包含
`2..64` 项，类型只允许 `press`、`move`、`click`，且至少出现一个完整 `press` 和一个指针步骤。
`press` 复用 UIX 按键契约的唯一键名、修饰键与 provider 映射；`move`/`click` 复用 finite 且
`0..65535` 的客户区点校验。`intervalMs` 为 `0..500`，计划时长不得超过 `5000` 毫秒；
`timeoutMs` 为 `100..30000`，并必须至少比计划时长多 `100` 毫秒。

该跨模态要求是新能力边界：两个独立 capability 调用会重新发现并重连，不能声称拥有同一连接和
revision 连续性。反之，纯按键或纯指针序列必须使用已有专用 capability，不得借此重复包装。

## Agent 预检与有序执行

Adapter 在首个 dispatch 前一次性预检 `perform`、序列实际使用的 `press_key`、`pointer_move`、
`click_at`，以及全部键名和修饰键。随后仅在同一认证连接内按固定 generation 和响应 revision 链
串行提交，每个 `press_key` 与 `click_at` 都是 Agent 提供的请求级配对事件。任一 dispatch 后若
传输、revision、deadline 或最终响应失去可信事实，结果固定为 `OUTCOME_UNKNOWN`，公开各类已接受
计数并禁止自动重试。首个 dispatch 前的确认、解析、stale、策略或能力缺口保持安全失败。

## 成功结果与停止线

成功结果由 `uix-input-sequence-result.schema.json` 冻结，公开 provider-neutral 原步骤、总步骤及
press/move/click 接受计数、计划时长、最终 revision 和 settled 事实。成功只证明所有步骤在同一
认证连接和 revision 链上被协议接受，不证明应用消费或最终 UI；因此 `effectConfirmed=false`、
`finalStateReached=false`。

该 capability 明确 `transactionSemantics=false`、`rollbackSemantics=false`，不公开独立按键或按钮
所有权、文本输入、持键、重复、拖拽、双击、click count、滚轮或桌面指针语义；不请求前景，不泄露
原生/传输身份，不使用 Portal、X11/XWayland 或 fallback。用户真实交互验收暂缓是外部状态，不是
运行时成功证明。

```text
ai-computer-toolkit run app apply --capability ui.input.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
