# `window.lifecycle.sequence.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

这是一次已认证 UIX 应用窗口生命周期动作序列与框架当前状态条件的绑定。能力使用
`app.apply`、`host-foreground`，并明确需要逐操作确认和前景影响同意；同意前景影响不等于
请求激活窗口或承诺获得焦点。

## 输入契约

输入由 `window-lifecycle-sequence-transition-input.schema.json` 冻结，根对象严格封闭，只接受
`actions`、可选 `intervalMs`、可选 `timeoutMs`、必填 `condition` 和可选 `pollIntervalMs`：

- `actions` 为 `2..16` 个严格复用的 `restore`、`minimize`、`maximize` 或 `resize`，至少包含
  两种不同动作类型；`resize` 只接受 `client-logical-px` 与 `1..65535` 的宽高；
- `intervalMs` 范围为 `0..500` 毫秒，默认 `0`；计划时长为
  `(actions.length - 1) * intervalMs`，且不超过 `5000` 毫秒；
- `timeoutMs` 范围为 `100..30000` 毫秒，默认 `30000`，并且必须覆盖计划时长再保留至少
  `100` 毫秒；
- `condition` 严格复用 `visibility`、`focus`、`client-size`、`window-flags` 四类
  `UixWindowStateWaitCondition`；每类只接受其定义的字段，visibility/flags 至少指定一个事实；
- `pollIntervalMs` 范围为 `20..500` 毫秒，默认 `50`。

未知字段、任意显式 `null`、浮点时间、越界尺寸、少于两类动作、非法动作或非法条件必须在
任何 provider I/O 前失败。Component 只暴露已验证的动作、条件、计数及时序，不回显非法原始 JSON。

## 双前置门禁与同连接执行

Module 的顺序固定为 confirmation first，然后是 `FOREGROUND_CONSENT_REQUIRED`，再解析 input、
target 和 provider；确认或前景同意缺失时不得读取输入文件、解析目标或访问 Agent。

Adapter 在总 deadline 内只 resolve 一次并建立一个认证操作连接。首个 dispatch 前一次性预检
`perform`、实际使用的全部 window action，以及状态观察所需的六个窗口字段；预检缺口不得产生
任何 mutation。随后在同一连接、固定 window ID/generation/opaque target 和连续 revision 链上，
按 `intervalMs` 串行发送完整动作序列。

完整序列成功接受后，以最终动作 revision 为基线在同一连接重复 `list_windows`，通过有界
`pollIntervalMs` 观察 framework-current 的 visibility、focus、client size 和 flags，直到条件
命中或 deadline 到期。该路线使用 provider polling，不使用 revision wait；`settled=false` 仍可
成功，只表示不宣称 compositor 或全局最终状态。

## 成功与失败边界

成功只证明完整动作序列已被 Agent 接受，并且 dispatch 后观察到框架当前条件匹配；不证明动作
与观察之间的因果关系，不证明 compositor 最终状态、应用消费、持久焦点、事务或回滚。结果固定
`compositorFinalStateConfirmed=false`、`effectConfirmed=false`、`finalStateReached=false`，并
公开同连接、fixed generation、revision chain 与 provider polling 事实。

首个 dispatch 前的输入、确认、前景同意、协议、权限、目标或能力缺口可以精确失败。任一动作
dispatch 开始后出现部分接受、连接关闭、timeout、stale、状态协议异常或不可信终态，必须公开
`OUTCOME_UNKNOWN`/`acceptedMayHaveOccurred` 与已接受动作数，并禁止自动重试；不得把部分序列
当成完整成功。

## 安全与版本边界

本能力只使用 UIX Agent v1 的公开窗口生命周期动作和 `list_windows` 状态观察，不移动桌面指针，
不注入桌面输入，不支持任意窗口 move，不公开 native/transport identity，不使用 X11/XWayland、
compositor 私有协议或 fallback。用户真实窗口交互验收暂缓是外部状态，不是运行时成功字段。

契约依赖 `uix-app v0.0.2` 已有公开的 lifecycle action response、固定 generation 窗口状态和
同连接 `list_windows` 语义，本批不修改 uix-app。
