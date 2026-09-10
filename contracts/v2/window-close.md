# `window.close@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.close` surface，向显式启用 UIX Agent 的精确窗口 generation 提交一次平台关闭请求。输入仅接受可选 `timeoutMs`；范围为 100 至 30000 毫秒，默认 30000 毫秒。调用必须逐操作确认，但不请求焦点，也不接受前景同意。

总 deadline 覆盖目标重新发现、端点认证与 Agent 响应。Agent 必须在 `hello.capabilities.window_actions` 中发布 `close_window`，并在目标 UI turn 内通过 UIX 平台窗口契约调用 `request_close()`。成功只证明请求已接受，固定 `closeConfirmed=false` 与 `finalStateReached=false`；调用方必须使用 `window.closed.wait@2` 另行证明原 generation 已关闭。

目标 stale、动作未发布、应用策略拒绝或窗口不可呈现均在 dispatch 前结构化失败。请求发送后断连、应用关闭、平台调用失败或 settle 不可信统一返回不可自动重试的 `OUTCOME_UNKNOWN`；下一步仍是 `window.closed.wait@2`，不得盲目重试。实现不激活窗口、不注入桌面输入，也不回退 X11、XWayland 或 compositor 私有协议。

```text
ai-computer-toolkit run app close --capability window.close@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
