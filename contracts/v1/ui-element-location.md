# Provider-neutral UI element location v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.element.locate@1` 是统一 `app.read` surface 上的只读 Query。它以当前
`sessions app` 返回的精确 `s2:w:*` 窗口和 provider-neutral 语义 selector 为输入，
在 Job-bounded observation worker 中重新解析窗口、执行有界 UIA 搜索，并返回零或
唯一匹配、当前物理屏幕 bounds 与 provider 给出的 ClickablePoint。它不建立 UIA 写入口。

## 所有权与依赖

- `ComputerControlSystem` 冻结 `isolated-worker` 执行计划并附加策略证据；
- Element Location Module 拥有输入、零一多、搜索完整性、坐标快照和公开结果契约；
- Accessibility observation worker 私有 Component 拥有 UIA cache、TreeWalker、
  Per-Monitor-V2 线程 DPI 上下文、最小化查询和 ClickablePoint 读取；
- 共享 Accessibility Selector Component 只拥有五种公共语义字段的精确 AND 语义；
- 公共边界不出现 HWND、PID、RuntimeId、COM/UIA 类型、provider identity 或原生错误。

这是同步 Query，不使用 EventBus。现有 `accessibility.tree.read@1` 继续不读取或公开
bounds；新增位置数据只存在于本 capability 的独立 cache request 与公开 schema 中。

## 输入与搜索

输入由 `ui-element-locate-input.schema.json` 约束。selector 至少包含一项
`name|automationId|className|frameworkId|controlType`，多项按精确 AND 匹配；字符串
运行时限制为 1..512 UTF-8 bytes。`maximumDepth` 为 0..20，`maximumItems` 为
1..4096，`view` 为 `control|raw`，`timeoutMs` 为 1..30000。

主 Module 和 worker 每次执行都分别从当前窗口 inventory 重新解析同一 `s2:w:*`。
零窗口返回 `STALE_SESSION`，碰撞返回 `AMBIGUOUS_TARGET`。深度、数量、TreeWalker 或
属性读取不能证明完整搜索时返回 `SEARCH_INCOMPLETE`；两个已证明匹配立即属于
`AMBIGUOUS_TARGET`，不得选择任意候选。

零 element 是成功的 `matchState=missing`，与 stale window 不同。唯一 element 的
`s2:e:*` 和 `identityFreshness=location-snapshot` 仅用于本次观察，不是后续写授权或
稳定路由键；任何后续动作必须用原 selector 对窗口重新定位。

## 坐标与命中区域

UIA BoundingRectangle 和 GetClickablePoint 均解释为虚拟桌面物理屏幕像素。worker 在
坐标调用前进入 Per-Monitor-V2 线程 DPI 上下文，允许多显示器负坐标；结果携带窗口
session 代际、重新解析、坐标空间、DPI 上下文和
`snapshot-only-re-resolve-before-action` 有效期。

可用 bounds 返回 `left/top/right/bottom/width/height`；空矩形或 provider 缺口返回
显式 `unavailable`。命中点只接受 UIA provider 的 ClickablePoint，并要求位于同一
snapshot bounds 内；不得用矩形中心猜测。最小化、offscreen、无 bounds、无 clickable
point、provider 缺口、点越界和 element missing 分别返回稳定不可用原因。

ClickablePoint 可用只表示 provider 在该快照给出了用于点击该 element 的点，不扩大为
视觉遮挡、持续可点或动作成功证明。不可用时 `occlusion=unknown`；调用方不得把 unknown
解释为无遮挡。窗口移动、缩放、DPI 或显示器布局变化后必须重新执行定位。

## 安全与验证

该 Query 不读取 Value/Text 内容，不查询 Invoke/Value/Toggle/Selection/Scroll 等写
pattern，不设置焦点、不激活窗口、不发送键鼠或消息。前景变化失败闭合。worker 受 Job、
deadline、输出上限和 cancellation 生命周期约束，生产入口仅为
`tools/Invoke-ComputerControl.ps1` 选择的 Rust runtime。

Rust 门禁使用工具自有 no-activate 标准窗口，通过正式 launcher 覆盖 unique、missing、
物理 bounds、provider clickable point 和 minimized 不可点击结果。schema 与协议测试覆盖
负坐标、多显示器、unknown 字段、歧义、不完整搜索和 native 字段拒绝。真实软件视觉与
交互验收仍只由 `yang86` 执行；本 Query 本身不授权任何真实窗口动作。
