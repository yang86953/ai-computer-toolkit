# `ui.input.pointer.drag.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 在已确认的 UIX 应用内，对一个精确窗口 generation 提交一次固定左键拖拽，
并观察动作提交后的语义 postcondition。它运行在 `same-session-no-focus`，需要逐操作确认，
但不需要也禁止前景同意；调用方必须提供 `--confirm`，不得提供
`--allow-foreground`。它只接受 opt-in UIX Agent，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-pointer-drag-transition-input.schema.json` 冻结，严格封闭，只允许：

- `coordinateSpace`，固定为 `client-logical-px`；
- `start` 与 `end`，各自含 finite 的数值 `x`、`y`，每个坐标在 `0..65535`；
- `samples`，`1..64`，默认 `12`；
- `durationMs`，`0..5000`，默认 `250`；
- `timeoutMs`，`100..30000`，默认 `30000`，并且 Rust 校验要求
  `timeoutMs >= durationMs + 100`；
- `postcondition`，由 exact-AND `selector` 与 `condition` 组成，condition 只能是
  `unique` 或 `missing`。

输入只表达一次请求内配平的普通左键拖拽，不暴露独立 button、跨请求按钮所有权、滚轮或
独立 down/up capability。未知字段、null selector 字段、空 selector、非 finite 坐标、浮点
时限和越界 drag 参数必须在访问 Agent 前失败。

## 确认、同连接执行与结果边界

Module 必须先完成 confirmation-first 检查，再解析 input、解析精确窗口并取得认证 Agent。
Adapter 不发送 pre-action snapshot；它只在同一条认证连接上完整执行
`pointer_down` → 有界 `pointer_move` 序列 → `pointer_up`，并要求本请求的 release 已获确认。
释放确认后，以最终 action revision 为起点在同一连接上执行 semantic snapshot/revision wait，
再判断 `unique` 或 `missing` postcondition。不得用另一条连接、provider 轮询或缓存结果拼接成功。

如果 down、移动、响应、deadline、连接或 release 事实变得不可信，直接返回
`OUTCOME_UNKNOWN`，不得进入 postcondition wait，也不得自动重试；特别是未确认 release 时，
不能把部分拖拽解释为成功或已配平。动作后 semantic ambiguity 可以保留为特定失败，仍然
禁止重试。

成功只证明同连接拖拽已按协议 dispatch、release 已确认且动作后 semantic postcondition
满足；不证明因果关系、应用消费、最终 UI 状态或宿主桌面指针位置，也不请求前景激活。
结果不得声明桌面输入、独立按钮所有权或跨请求状态。

实现只使用 UIX Agent 的应用内动作，不使用桌面输入、X11/XWayland、原生身份、transport
identity 或 fallback。v0.0.2 的 `pointer_down`、`pointer_move`、`pointer_up` 与同连接
revision 语义足以承载该窄契约；uix-app 保持只读，本 capability 不改变其代码或协议。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer.drag.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
