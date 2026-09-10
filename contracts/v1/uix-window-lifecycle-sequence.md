# `window.lifecycle.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证 UIX 窗口的同一连接、固定 generation 与连续
revision 链中顺序提交多阶段窗口生命周期动作。它必须同时获得逐操作确认和前景影响同意，但固定
`hostForegroundActivationRequested=false`：同意可见窗口变化不等于请求激活或承诺取得焦点。

## 输入与真实增量

输入由 `uix-window-lifecycle-sequence-input.schema.json` 冻结，只接受 `actions`、`intervalMs`、
`timeoutMs`。`actions` 包含 2..16 个复用版本二生命周期契约的 restore/minimize/maximize/resize，
且至少出现两种不同动作；resize 只接受 `client-logical-px` 和 1..65535 的客户区宽高。相邻间隔
0..500 ms，计划时长最多 5000 ms，总 deadline 必须至少保留 100 ms。

至少两种动作避免把单动作或同类重复包装成新 capability。现有 Workflow 的每步会重新发现和重连，
不能证明同一 connection/generation/revision 链；该请求内连续性是本能力的唯一真实增量。

## 执行与失败边界

Adapter 在首个 dispatch 前预检 `perform` 及请求实际使用的全部 window action，随后只在同一认证
连接、固定 generation 和响应 revision 链上顺序提交。任一 dispatch 后出现传输、revision、
deadline、presentability 或最终响应不可信，都返回 `OUTCOME_UNKNOWN`，公开已接受动作数并禁止自动
重试。首个 dispatch 前的确认、前景同意、输入、stale、策略或能力缺口保持安全失败。

成功结果由 `uix-window-lifecycle-sequence-result.schema.json` 冻结，只证明动作按协议获接受，不证明
compositor 最终窗口状态；因此 `effectConfirmed=false`、
`finalStateReached=false`、`compositorFinalStateConfirmed=false`。能力明确无事务、回滚、任意 Wayland
move、桌面输入或桌面指针，不泄露原生/传输身份，不使用 X11/XWayland、compositor 私有协议或
fallback。用户真实窗口交互验收暂缓是外部状态，不是运行时成功证明。

```text
ai-computer-toolkit run app apply --capability window.lifecycle.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground
```
