# `act/application-target-identity/v1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该契约只公开 `s2:a` 目标的身份边界，不公开 provider 私有定位材料。

- `providerDomain` 声明目标来自 Linux XDG Desktop Entry provider。
- `generationBound=true` 表示 ID 绑定到发现时的胜出项，以及安全打开后核对的
  device/inode/size/mtime/ctime 与内容代际；内容被替换、同 ID 的高优先级胜出项消失或
  优先级变化后，旧目标必须视为 stale。
- Desktop File ID、原生路径、`Exec`、`TryExec` 与内容摘要始终不公开。
- 该身份不承诺 launch、进程关联、窗口关联或坐标控制。
