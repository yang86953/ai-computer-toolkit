# Window target identity v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

AT-SPI v2 窗口公开 ID 只是不透明路由指纹，不是授权令牌。私有材料绑定 accessibility bus 代际、
当前唯一 owner 与 object path；这些原生事实永不进入 JSON、日志或错误。owner/bus 代际变化会使旧
ID stale，但同一 owner 内 object path 重用无法被证明，因此 generation owner 固定为 none。

该身份只允许只读 inspection snapshot，coverage 固定 partial accessibility exporters，禁止升级为
compositor 窗口身份或进入 close、lifecycle、input、screenshot、Action 等 mutation 路径。
