# `ui.input.pointer.move.sequence.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

这是一次应用内 `pointer_move` 多点序列与提交后语义条件的绑定。能力只面向已认证的
UIX 应用窗口，执行域为 `same-session-no-focus`，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-pointer-move-sequence-transition-input.schema.json` 冻结，根对象严格封闭，
只接受 `coordinateSpace`、`moves`、可选 `intervalMs`、可选 `timeoutMs` 与必填
`postcondition`：

- `coordinateSpace` 必须为 `client-logical-px`；每个点仅含 finite 的 `x`、`y`，范围为
  `0..65535`；
- `moves` 为 `2..64` 个 provider-neutral 点；每个点只映射为应用内 `pointer_move`，不
  插值、不平滑、不点击、不提供独立 down/up、拖拽或滚轮；
- `intervalMs` 范围为 `0..500` 毫秒，默认 `0`；计划时长为
  `(moves.length - 1) * intervalMs`，且不超过 `5000` 毫秒；
- `timeoutMs` 范围为 `100..30000` 毫秒，默认 `30000`，且至少覆盖计划时长并额外保留
  `100` 毫秒；
- `postcondition.selector` 复用 exact-AND 语义 selector，至少包含一个有效字段；condition
  只允许 `unique` 或 `missing`。

未知字段、任意显式 `null`、浮点时间、非法或非有限坐标、过短序列、越界时序、空 selector、
其他坐标空间或其他条件必须在 provider I/O 前失败。Component 只暴露已验证的点、计数、时序
和条件，不回显非法原始 JSON。

## 确认、同连接 dispatch 与观察

Module 必须先完成 confirmation，再解析输入、目标或访问 provider。Adapter 在总 deadline
内只 resolve 一次、建立一个认证 Agent 连接，并固定 window generation；首个 dispatch 前
一次性预检 `snapshot`、`perform`、`wait` 及 `pointer_move`。预检失败不得发送任何移动。

确认后按输入顺序在同一认证连接发送完整 `pointer_move` 序列，并维护连续 revision。完整
序列成功接受后，以序列最终 revision 为观察基线取得 semantic snapshot；必要时在同一连接
执行 revision-after wait，再取得 snapshot，检查 exact-AND 的 `unique` 或 `missing` 条件。
结果中的 `sequenceSettled=false` 仍可成功，只表示不把一次动作误报为全局最终 settled。
结果同时冻结 `transactionSemantics=false` 与 `rollbackSemantics=false`，不把逐项接受的序列冒充
事务或可回滚操作。

成功只证明完整移动序列已被 Agent 接受，并且 dispatch 后观察到了当前语义条件；不证明
`pointer_move` 与条件之间存在因果关系，不证明应用消费、最终 UI、焦点、桌面指针或主机输入
结果。结果固定同连接、固定 generation、revision chain、无插值语义和
`effectConfirmed=false`。

首个 dispatch 前的输入、协议、权限、目标或能力错误可以精确失败。任一 dispatch 开始后
出现部分接受、连接关闭、timeout、stale、协议异常、后置条件歧义或无法证明终态，必须
公开相应 accepted 事实或 `OUTCOME_UNKNOWN`/歧义错误，并禁止自动重试；不得把未 dispatch
或不可信结果伪装成完整成功。

## 安全与版本边界

本能力不请求前景，不注入桌面输入，不移动桌面指针，并固定 `clickSupported=false`、
`keyPressSupported=false`；不提供按键、点击、拖拽或 scroll，
不公开 native/transport identity，不使用 X11/XWayland、compositor 私有协议或 fallback。
结果中的 element 仅是 snapshot-revision 的脱敏语义投影。

契约依赖 `uix-app v0.0.2` 已有的公开同连接 `pointer_move`、action revision、semantic
snapshot 与 revision-after wait 语义，本批不修改 uix-app。用户真实交互验收暂缓；静态契约
和单元测试通过不等同于真实应用消费或最终 UI 验收。
