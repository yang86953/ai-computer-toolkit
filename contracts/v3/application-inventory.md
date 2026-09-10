# `application.discover@3` Linux launch-status inventory

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

版本三保持 `application.discover@2` 的 XDG Desktop Entry、procfs、完整性、隐私与零关系推断
边界，只新增与 `application.open@2` 同一实现批次原子发布的 `launchCapability` 状态。

普通 Desktop Entry 继续返回 `unavailable`。只有 use-time 能重新认证为当前 toolkit 自有
绝对映像、固定私有参数且非 D-Bus activation 的夹具记录返回 `available-confirmed`；该状态不
表示任意用户应用可启动。公开结果仍不包含 Desktop File ID、路径、Exec、TryExec、内容摘要、
argv、PID 或原生句柄。

版本二 schema 与行为保持逐字兼容并继续固定 `launchCapability=unavailable`。调用方必须显式
请求 `application.discover@3` 才能取得版本二启动候选，随后仍需逐操作确认和前景影响同意。
