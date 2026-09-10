# `ui.input.key.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 将一次完整的应用内 `press_key` 与提交后的 exact-AND 语义条件绑定在同一
个请求中。输入是扁平的单次 `key`、可选 `modifiers`、`postcondition.selector` 与
`postcondition.condition`，以及 100..30000 毫秒、默认 30000 毫秒的总 deadline。按键名和
修饰键复用 `ui.key-input@2` 的 provider-neutral 白名单；条件复用元素 transition 的
`unique|missing` 和 exact-AND selector。

## 固定执行顺序

Module 固定 confirmation-first：确认必须先于输入、目标、provider、权限或任何文件和
网络访问。Adapter 对当前精确窗口只 resolve 一次，只建立一个认证 UIX Agent 连接，并在
动作前预检连接已发布 snapshot、perform、wait、`press_key` 以及实际 key catalog；缺少目标或
协议目录时在动作前失败。

在同一认证连接上执行一次 `perform` 的完整 `press_key`。动作响应的 action revision 是后续观察
基线；Adapter 随后在该连接上读取 semantic snapshot，
必要时使用 revision-after wait 后再次读取 snapshot，直到 `unique` 或 `missing` 条件满足或
deadline 结束。不得改用桌面输入、native key code 或后台 fallback。

## 结果与不确定性

成功只证明 Agent 接受了该次请求，并且 dispatch 后观察到的当前语义条件匹配；不证明按键
与条件之间存在因果关系，不证明最终 UI、应用消费、焦点持久性或任何桌面输入效果。动作后
出现多个匹配时保留 `AMBIGUOUS_TARGET`，连接关闭、timeout、stale、协议异常或无法取得可信终态
时返回 `OUTCOME_UNKNOWN`；两类错误都明确动作可能已经发生并禁止自动重试。其他动作前输入、
解析、策略或目录错误仍可返回精确失败。

这是一项 same-session-no-focus 的协作式 UIX capability：不请求主机前景，不注入桌面键盘，
不使用 Portal、PipeWire、X11/XWayland、compositor 私有协议或 native fallback，也不声明
按键一定被应用消费。现有 uix-app 已提供同连接 snapshot、`press_key` perform、key catalog
与 revision-after wait 所需框架；本契约不修改 uix-app。
