# 协作式 UIX 语义元素定位 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.element.locate@2` 是统一 `app.read` surface 上的只读 Query，只覆盖显式启用
`uix.agent.v1` 的协作式应用。它不会改写 Windows UIA `ui.element.locate@1` 的物理屏幕
坐标、ClickablePoint 或隔离 worker 语义，也不把 UIX 原生节点、进程、端点、token 或
传输身份暴露到稳定 JSON。

## 所有权与调用顺序

- `ComputerControlSystem` 只把已登记的只读 capability 路由到 Linux App Adapter；
- UIX Element Location Module 拥有 selector、零一多、公开元素身份、逻辑几何和错误语义；
- UIX Agent Adapter 每次重新发现精确窗口 generation、认证本机端点并读取完整有界 snapshot；
- selector Component 只拥有六种公开字段的精确 AND 匹配，不取得 Adapter 或 Module 生命周期。

调用方通过生产 launcher 执行：

```text
ai-computer-toolkit run app read --capability ui.element.locate@2 --target sessionId=<s2:w:opaque> --input <file|->
```

输入只包含 `selector`。它至少具有 `automationId|role|name|focused|enabled|action` 之一；
多字段按大小写敏感的精确 AND 匹配，未知字段、空字符串和越界字符串都在端点发现前拒绝。

## 结果、身份与几何

Module 在同一次认证 snapshot 内遍历最多 4096 个节点。零匹配是成功的
`matchState=missing`；唯一匹配返回 snapshot-scoped `s2:e`；两个及以上匹配返回
`AMBIGUOUS_TARGET`，不得选择任意候选。窗口消失、代际变化、snapshot 超限或协议字段非法均
结构化失败，不发布部分结果。

成功结果携带 `as3` snapshot、revision/presentedRevision、匹配语义与完整遍历计数。元素身份
只在该 snapshot revision 内有效；后续 mutation 必须携带当前 `snapshotId + elementId` 进入
`ui.element.action@2`，不得把原生节点或旧身份缓存为写目标。

返回的 `frame` 与可选 `visibleBounds` 来自 UIX 应用客户区，坐标空间固定为
`application-client`，单位为 `logical-px`。它们不是 compositor、输出流或宿主桌面坐标，不能
推导屏幕点击点、遮挡、全局窗口位置或任意第三方应用命中区域。该 capability 不发布
ClickablePoint，也不把矩形中心猜测为点击点。

## 安全与验收

读取会接收 UIX snapshot 中的敏感字段，但只发布唯一匹配元素的有界公共语义字段和逻辑几何；
value、selection、文本接口内容、完整树、原生 ID 和传输身份均不发布。调用不请求宿主窗口激活、
不发送桌面输入、不调用 mutation，不允许 strict isolation 降级，也不使用 X11/XWayland、
compositor 私有协议或其他 fallback。

完成门禁包括输入/输出 schema、Rust selector/零一多/坐标与协议回归、生产 CLI 路由、私有
Agent fixture，以及真实 Wayland `uix-lang-demo` 上的 unique、missing 与 logical geometry
复验。真实验收只证明 opt-in UIX 应用路线，不扩大为 compositor 全局控制。
