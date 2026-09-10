# `window.state.wait@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.read` surface，在一个总 deadline 和同一认证 UIX Agent 连接内等待
精确窗口 generation 的框架当前状态满足调用方条件。它补齐 `window.lifecycle@2` 只证明动作接受、
`window.state.read@1` 只能瞬时读取之间的观测缺口，但不改变两者既有语义。

## 条件与边界

输入由 `window-state-wait-input.schema.json` 冻结。`condition` 只接受四类封闭条件：

- `visibility`：至少指定 `visible` 或 `presentable`；
- `focus`：指定 `focused`；
- `client-size`：指定 1..65535 的 logical `width` 与 `height`；
- `window-flags`：至少指定 `maximized`、`minimized`、`fullscreen` 之一。

`pollIntervalMs` 为 20..500，默认 50；`timeoutMs` 为 100..30000，默认 30000。条件只在
`client-logical-px` 和 `uix-framework-current` 事实上判断，不读取或推断 compositor 私有状态。

## 为什么不使用 Agent `wait`

`uix-app v0.0.2` 的 `wait` 只接受 revision/presentedRevision 阈值。该版本
`publish_window_state` 会更新 visible、presentable、focused、logical size 和窗口 flags，但不会仅因
这些字段变化推进 revision 或唤醒 wait。因此 Adapter 先重新解析目标并认证一次，然后仅在同一
`AgentClient` 上有界重复 `list_windows`；每轮都要求固定 window ID、generation、opaque target、
单调 revision，拒绝重复、关闭、缺失、字段未协商或协议倒退。

成功结果由 `window-state-wait.schema.json` 冻结，只证明 UIX 框架当前条件已匹配；
`frameworkConditionMatched=true` 且 `compositorFinalStateConfirmed=false`。能力只读、可安全重试，
不确认 mutation、不请求前景、不注入桌面输入，不泄露端点、token、PID、原生窗口 ID 或 generation，
不使用 Portal、X11/XWayland、compositor 私有协议或 fallback。用户真实观察验收暂缓。

```text
ai-computer-toolkit run app read --capability window.state.wait@1 --target sessionId=<s2:w:opaque> --input <file|->
```
