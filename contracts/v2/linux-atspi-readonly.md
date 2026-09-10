# Linux AT-SPI read-only candidate v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.discover@2`、`window.metadata.read@2` 与 `accessibility.tree.read@2` 是同一组
Linux 只读候选。它们只观察主动导出 AT-SPI 的应用，coverage 固定为
`partial-accessibility-exporters`，不能代表 compositor 窗口目录、前景、无遮挡、坐标或完整桌面。
Windows `window.*@1`、`accessibility.tree.read@1` 与 UIA 节点字段保持冻结。

## 身份与生命周期

`act/window-target-identity/v2` 的私有绑定包含 accessibility bus 代际、当前唯一 owner 和 object
path；公开 `s2:w` 只是路由指纹，不公开这些材料。owner 退出、重连或 bus 重启使旧目标 stale；同一
owner 内 object path 被销毁并复用时无法证明新旧代际，因此
`sameOwnerObjectPathReuse=not-guaranteed`、`generationOwner=none`。该身份只能进入上述三项只读
能力，`mutationAllowed=false`；close、lifecycle、input、screenshot、Action 和任何 mutation
不得接收它。

树的 `snapshotId` 不是 s2 授权目标。`s2:e` 私有绑定包含 snapshot、owner 与 path，仅在一次完成的
inspection snapshot 内有效，不能进入写路径。

## 私有 Adapter 与安全停止线

ComputerControlSystem 只登记能力、路由和 fail-closed assessment。LinuxWindowObservationModule
拥有 partial discovery、使用时唯一解析和 stale；LinuxAccessibilityModule 拥有 BFS、总 deadline、
数量/深度边界与公共投影。AtspiBusConnector、InventoryReader、AccessibleReader、Identity 和
PublicProjection 都是私有 Adapter/Component，zbus 类型不能越过该边界。

候选 worker 只在非默认 `linux-atspi-candidate` feature 下构建，只接受显式注入的私有 session bus
和 accessibility bus 地址。默认 Linux CLI、release archive 与生产路由既不读取
`DBUS_SESSION_BUS_ADDRESS`，也不调用真实 `org.a11y.Bus`。真实用户会话验收前，三项能力保持
candidate/unavailable、`executionRealm=none`、`fallback=none`。

读取只允许 `Accessible.Name`、`GetRole`、`GetState`、`ChildCount` 和逐项
`GetChildAtIndex`；proxy 缓存关闭。禁止 GetAll、GetChildren、Cache.GetItems、Action、EditableText、
Selection、Value、Component.GrabFocus、GetApplicationBusAddress、Text、坐标以及属性变化订阅。
公开 JSON 和错误不得包含 bus address/GUID/name、object path、PID/UID、Application.Id、
AccessibleId、toolkit/version、locale、description、attributes、relations、interfaces 或原生错误。

BFS 的 `maxDepth` 为 0..20、`maxItems` 为 1..4096、总 deadline 为 1..30000ms。精确窗口根失败不
发布 partial；后代失败只能以封闭 truncation reason 标记。所有成功或失败都在 worker 完整终止后
一次性发布，timeout/cancel 必须 kill+wait，不得留下 worker 或局部 JSON。
