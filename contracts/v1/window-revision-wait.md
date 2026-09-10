# `window.revision.wait@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.read` surface 等待显式启用 UIX Agent 的精确窗口代际。调用方必须提供当前 `s2:w`，并在输入对象中二选一：

- `revision-after`：等待 `revision` 严格大于阈值；
- `presented-at-least`：等待 `presentedRevision` 大于或等于阈值。

`timeoutMs` 是 100 至 30000 毫秒的总 deadline，覆盖窗口重新发现、端点认证和 Agent 等待。实现会从总 deadline 中保留短暂响应解析预算，因此传给 Agent 的私有等待值可能更小。

窗口在同一代际内关闭时返回成功终态 `closed`；窗口换代、目标消失、认证失败、超时或协议不一致均返回结构化错误，不发布部分结果。路线只观察应用发布的修订事实，不请求前景激活、不注入桌面输入，也不回退 X11 或 compositor 私有协议。

```text
ai-computer-toolkit run app read --capability window.revision.wait@1 --target sessionId=<s2:w:opaque> --input <file|->
```
