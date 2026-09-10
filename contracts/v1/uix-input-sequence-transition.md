# `ui.input.sequence.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 是 UIX 协作式应用内键盘与指针混合序列的状态过渡入口。它通过
`app.apply` 在 `same-session-no-focus` realm 中运行，逐操作确认，不请求前景同意，且只
接受精确的 opaque window generation。

## 输入边界

`uix-input-sequence-transition-input.schema.json` 冻结输入为
`coordinateSpace`、`steps`、`intervalMs`、`timeoutMs` 与必填 `postcondition`，未知字段和任意
位置的显式 `null` 都拒绝。坐标空间固定为 `client-logical-px`；`steps` 为 `2..64` 项，只允许
`press`、`move`、`click`，并且至少有一个完整按键步骤和一个指针步骤。按键名称与修饰键复用
`ui.input.sequence@1` 的严格 provider-neutral 目录及映射，指针坐标复用其 finite、`0..65535`
校验，不公开独立 down/up、持键、重复、文本或按钮所有权。

`intervalMs` 默认为 `0`，范围为 `0..=500`；计划时长为
`(steps.length - 1) * intervalMs` 且不得超过 `5000` 毫秒。`timeoutMs` 默认为 `30000`，范围为
`100..=30000`，并且必须至少覆盖计划时长再保留 `100` 毫秒余量。后置条件使用复用的
exact-AND selector，只允许 `unique` 或 `missing`。

## 执行与结果

确认必须先于输入解析、目标解析、认证连接和任何 provider I/O。Adapter 应在总 deadline
内只解析一次目标、建立一个认证连接，在首次 dispatch 前预检 `snapshot`、`perform`、`wait`、
实际使用的 `press_key`/`pointer_move`/`click_at` 以及全部键名和修饰键；随后固定 generation，
按连续 revision 顺序完成整个序列，再以最终 sequence revision 为语义观察基线。

成功结果由 `uix-input-sequence-transition-result.schema.json` 冻结：公开 provider-neutral
步骤、四类请求/接受计数、`allPressesBalanced=true`、`allClicksBalanced=true`、
`sequenceRevision`/`sequencePresentedRevision`/`sequenceSettled`，以及同一连接上的
`unique` 或 `missing` 观察、snapshot/revision 与元素脱敏投影。成功只表示完整序列被 Agent
接受且观察到了声明的后置条件；不证明动作与条件之间存在因果关系，不证明应用消费或最终
UI 状态。`sequenceSettled=false` 仍可随可信后置观察返回成功。

首个 dispatch 前的确认、解析、stale、策略或能力缺口应保持精确安全失败。部分接受、后置
条件歧义或 dispatch 后任何不可信终态均为 `OUTCOME_UNKNOWN` 或对应的 `AMBIGUOUS_TARGET`，
必须公开已接受计数并禁止自动重试；本能力没有事务或回滚语义。

## 安全与停止线

结果固定声明同一认证连接、固定 window generation、连续 revision 链和 UIX Agent 应用内部
事件。它不注入桌面输入、不移动桌面指针、不请求前景，不泄露 native/transport identity，
不使用 X11/XWayland 或 fallback；也不提供拖拽、双击、click count、滚轮、独立按键/按钮
所有权或稳定性语义。

用户真实交互验收暂缓属于外部验收状态，不是运行时成功字段，也不得被结果伪装成已完成。
本契约不要求修改 `uix-app`。
