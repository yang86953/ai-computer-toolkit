# `window.state.read@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 只读取显式启用 `uix.agent.v1` 且在 `hello.capabilities.window_state_fields`
协商完整字段集的精确窗口 generation。输入是严格空对象 `{}`。

输出包括：

- `client-logical-px` 坐标空间中的 logical 客户区宽高；
- UIX 框架当前维护的可见、可呈现、最大化、最小化和全屏状态；
- 固定的 `observationSource=uix-framework-current` 与
  `compositorFinalStateConfirmed=false`。

该读取不证明窗口管理器或 Wayland compositor 已确认先前动作终态，不覆盖任意第三方应用，
不暴露原生窗口、端点、token 或 PID，也不允许 X11、桌面输入或其他 fallback。旧 Agent 未协商
完整字段集时返回 `CAPABILITY_UNAVAILABLE`，不得从标题、几何或其他旁路推断状态。

`window.lifecycle@2` 保持独立的保守 mutation 契约；调用方可以在动作后显式读取本 capability，
但不得据此把 lifecycle 的 `effectConfirmed` 或 `finalStateReached` 改写为 true。
