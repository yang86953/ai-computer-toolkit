# `ui.input.pointer.move.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 将一次应用内单次 `pointer_move` 与动作提交后的 exact-AND 语义条件绑定在同一
个请求中。它只面向已认证的协作式 UIX 应用窗口，不控制宿主桌面或任意第三方窗口。

## 输入契约

正式输入由 `uix-pointer-move-transition-input.schema.json` 冻结，严格封闭，只接受
`coordinateSpace`、`x`、`y`、`postcondition` 与 `timeoutMs`：

- `coordinateSpace` 必须为 `client-logical-px`；
- `x`、`y` 必须是 finite 数值，且处于 `0..65535`；
- `postcondition.selector` 复用 exact-AND 语义 selector，至少包含一个非空字段；
- `postcondition.condition` 只能为 `unique` 或 `missing`；
- `timeoutMs` 为 `100..30000` 毫秒，默认 `30000`。

输入不含 `click`、`button`、`intervalMs`、`interpolation`、`drag` 或 `scroll`，也不开放独立
pointer down/up。未知、null、浮点 deadline、非法坐标、空 selector、其他坐标空间和越界参数
必须在任何 provider I/O 之前失败。Provider value 固定复用单次应用内 `pointer_move`。

## 确认与同连接执行

Module 负责 confirmation-first：确认必须先于输入解析、目标或 provider 访问。Adapter 只预检
认证 Agent 的协议 surface，不发送 pre-action snapshot。确认后，在同一认证连接上发送一次
`pointer_move`；动作响应的 action revision 成为观察基线，随后在该连接上读取 semantic snapshot，
必要时使用 revision-after wait，再读取 snapshot，直到 `unique` 或 `missing` 条件满足或 deadline
结束。

成功只证明请求被接受，并且 dispatch 后观察到的当前语义条件匹配；不证明 pointer_move 与条件
之间存在因果关系，不证明应用消费、最终 UI 或桌面指针结果。动作后出现歧义时保留明确的
specific 错误并禁止自动重试；其他 dispatch 后未知、连接关闭、timeout、stale 或不可信终态
均为 `OUTCOME_UNKNOWN`，同样禁止自动重试。

## 安全边界与版本依据

该 capability 使用 `same-session-no-focus`，不请求前景，不注入桌面指针，不声明独立 down/up、
click、双击、拖拽、滚动或插值语义，不公开 native/transport 身份，不使用 X11/XWayland、
compositor 私有协议或 fallback。UIX Agent v0.0.2 已提供所需的同连接 `pointer_move`、action
revision、semantic snapshot 与 revision-after wait；本契约不修改 uix-app，且保持其只读。
