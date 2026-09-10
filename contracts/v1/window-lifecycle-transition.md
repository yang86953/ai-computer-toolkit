# `window.lifecycle.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在逐操作确认和前景影响同意均先行后，对精确 UIX
窗口 generation 提交一次 restore、minimize、maximize 或 logical 客户区 resize，并在同一认证
操作连接内等待 dispatch 后的 `uix-framework-current` 状态满足封闭条件。它不改变单动作
`window.lifecycle@2` 或独立只读 `window.state.wait@1` 的契约。

## 输入与总 deadline

输入由 `window-lifecycle-transition-input.schema.json` 冻结。`action` 复用生命周期版本二的四类
动作，Wayland 任意 move 保持不可用；`condition` 复用 visibility、focus、client-size 与
window-flags 四类条件。`pollIntervalMs` 为 20..500、默认 50；`timeoutMs` 为 100..30000、
默认 30000，并覆盖重新解析、认证、预检、动作派发、响应和状态等待的全部阶段。
成功结果由 `window-lifecycle-transition.schema.json` 冻结，并公开动作响应、实际框架当前状态和
提交后的条件匹配事实。

## 同连接与失败边界

Adapter 只重新解析一次，随后建立一个认证操作连接。首个 dispatch 前必须同时确认 Agent 发布
`perform`、所选窗口动作和六个完整窗口状态字段；任何缺口都在 mutation 前失败闭合。派发成功后，
以动作响应的 revision/presentedRevision 为基线，在同一连接上有界重复 `list_windows`，每轮固定
window ID、generation 与 opaque target，并拒绝关闭、换代、重复窗口、字段缺失或修订倒退。

状态发布不会唤醒 v0.0.2 Agent wait，因此本能力明确使用 provider polling。成功只证明动作已被
Agent 接受，且一次 dispatch 后观察到的 UIX 框架当前状态满足条件；不证明该动作是条件成立的唯一
原因，也不证明 compositor 最终状态。dispatch 后若超时、目标 stale、协议异常或连接丢失，统一
返回 `OUTCOME_UNKNOWN`，保留 accepted-may-have-occurred、禁止自动重试，不把动作重发。

能力不请求 host activation、不注入桌面输入、不公开原生/传输身份，不使用 Portal、X11/XWayland、
compositor 私有协议或 fallback。用户真实窗口观察验收暂缓。

```text
ai-computer-toolkit run app apply --capability window.lifecycle.transition@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground
```
