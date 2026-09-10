# `ui.input.pointer.sequence.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 边界

这是一个仅面向已认证 UIX 应用的应用内指针序列 transition。输入严格闭合为
`coordinateSpace`、`steps`、可选 `intervalMs`、可选 `timeoutMs` 与必填
`postcondition`。步骤只能是 `move` 或普通左键 `click`，坐标空间只能是
`client-logical-px`；序列必须包含至少一个 move 和一个 click。后置条件使用
`exact-and` selector，并且只允许 `unique` 或 `missing`。

`intervalMs` 默认为 `0`，范围为 `0..=500`；`timeoutMs` 默认为 `30000`，范围为
`100..=30000`，且必须覆盖计划序列时长并保留至少 100ms 余量。未知字段、显式
`null`、浮点 timeout、空 selector、越界或非有限坐标均拒绝。

## 执行与结果

Module 必须先完成 confirmation 与策略评估，再解析输入、解析 opaque window
target 并交给 Adapter。Adapter 在同一认证连接和固定 window generation 内依次发送
完整的 move/click 序列；只有所有步骤成功接受、click 保持请求作用域平衡并取得最终
action revision 后，才允许从该 revision 开始做 snapshot/revision wait，检查
`unique` 或 `missing` 后置条件。结果中的 `sequenceSettled` 可以为 `false`；它不把
单帧 settled 误报成全局最终状态。

提交后语义条件匹配只证明完整序列已被接受且观测到了所声明的条件，不证明动作与
条件之间存在因果关系，不证明应用消费、最终 UI 状态或桌面指针状态。部分序列、
dispatch 后的 ambiguity 或其他无法证明的结果均保留不确定性并禁止自动重试；未经
dispatch 的明确协议错误可以结构化返回，但不得把它伪装成成功。

## 安全边界

本能力只使用 `uix-agent-v1` 的公开应用内 `pointer_move` 与普通 `click_at`，不暴露
原生或传输身份，不发布敏感 snapshot 或主机坐标映射，不轮询 provider，不声明
stability 语义，不请求前景，不注入桌面输入，也不移动桌面指针。它不提供独立按钮
所有权、拖拽、双击、click count、滚轮、X11 或 fallback。`uix-app` v0.0.2 已有
足够的同连接动作与 reply drain 语义，本批不修改它。
