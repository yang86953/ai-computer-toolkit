# `window.closed.wait@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.read` surface，等待显式启用 UIX Agent 的精确窗口 generation 关闭。输入仅接受可选 `timeoutMs`；范围为 100 至 30000 毫秒，默认 30000 毫秒。

总 deadline 覆盖目标重新发现、端点认证以及同一连接上的全部 Agent `wait` 请求。普通 revision 变化只推进下一次 `after_revision` 基线，不结束调用；只有 Agent 对原始 `window_id` 和 `generation` 返回 `closed` 才发布成功结果。

目标换代、目标消失、端点重启、认证失败、超时或协议不一致均返回结构化错误，不发布部分结果。实现不通过标题或全局窗口 inventory 猜测关闭，不请求前景激活、不注入桌面输入，也不回退 X11、XWayland 或 compositor 私有协议。

```text
ai-computer-toolkit run app read --capability window.closed.wait@2 --target sessionId=<s2:w:opaque> --input <file|->
```
