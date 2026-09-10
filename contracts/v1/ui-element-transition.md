# `ui.element.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 把一次已确认的 UIX 语义元素动作与一个封闭的 postcondition 绑定在同一
次操作中。输入使用初始 `snapshotId`、`elementId`、完整复用 `ui.element.action@2` 的九类
动作，以及 exact-AND selector 的 `unique|missing` 条件；`timeoutMs` 为 100..30000，默认
30000。

## 固定执行顺序

Module 固定 confirmation-first：确认必须先于输入解析、target 解析、发现、认证和任何
provider I/O。Adapter 在总 deadline 内只 resolve 一次、建立一次认证操作连接，并在初始
snapshot 中严格验证窗口 ID/generation、opaque target、snapshot revision/presentedRevision
和 elementId 对应的唯一 source。元素必须仍然 enabled，且初始 snapshot 发布请求动作；缺口
在 mutation 前安全失败。

随后在同一认证连接上发送 `perform`。若应用策略返回可选的应用确认，Adapter 仍使用该
连接完成确认，不把确认身份带入公开结果。动作响应的 revision/presentedRevision 是后续
观察的基线；先立即读取 snapshot。若 postcondition 尚未满足，则在同一连接发送
`revision-after` wait，收到 revision 变化后再次读取 snapshot，直到 `unique` 或 `missing`
成立或 deadline 结束。`settled=false` 只表示动作尚未给出 settle 事实，可以继续观察，不等价
于动作失败或最终 UI 状态。

成功只证明动作已被 Agent 接受，并且 dispatch 后观察到的当前语义 snapshot 满足条件；不
证明动作与条件之间存在因果关系，不声明最终 UI、业务消费、compositor 终态或回滚/事务。
snapshot 中两个或更多 selector 匹配保持元素语义 `AMBIGUOUS_TARGET`，但动作一旦开始就
不得自动重试。

## 不确定性与平台边界

任何 dispatch 开始后的 transport/timeout、连接关闭、stale、协议异常或无法取得可信
postcondition，均返回 `OUTCOME_UNKNOWN`，带 `acceptedMayHaveOccurred=true`、
`automaticRetryProhibited=true`、`retrySafe=false`；调用方必须先读取当前语义状态，再决定
下一步。dispatch 前的输入、source、动作目录或策略错误保持精确失败，不能伪造部分执行。

本能力是 `same-session-no-focus`：不请求主机前景，不注入桌面键盘/指针输入，不公开 UIX
原生身份或传输身份，不使用 X11/XWayland、Portal、compositor 私有协议或 fallback。uix-app
v0.0.2 已公开所需的同连接 `snapshot`、`perform`、revision-after `wait` 与语义 revision
通知；本批不存在需要修改 uix-app 的框架缺口。用户真实应用交互验收暂缓。
