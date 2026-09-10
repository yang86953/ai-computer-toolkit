# `ui.input.key@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

版本二通过统一 `app.apply` surface 向显式启用 UIX Agent 的精确应用窗口发送应用内部按键，
不改写 Windows 主机前台输入语义的 `ui.input.key@1`。

输入只接受一次完整 `press`：一个跨平台键名、零至四个不重复修饰键，以及 100..30000 ms
总 deadline。当前键集为字母、数字、F1-F12、常用编辑/导航键和不区分左右的
control/alt/shift/super。版本二不接受独立 down/up、长按、重复或文本；这些缺口不得借用桌面输入、
Wayland 虚拟键盘、X11/XWayland 或 compositor 私有协议补齐。

调用必须在读取输入文件、目标解析或端点访问前具备逐操作 `--confirm`。该路线不需要也不接受
`--allow-foreground`：Agent 只在目标 UIX 应用自己的 UI turn 内派发 KeyDown 与 KeyUp，不激活宿主
窗口，也不向桌面输入栈注入事件。`same-session-no-focus` 不满足 strict isolation，严格请求失败闭合。

Adapter 重新认证当前 `s2:w` generation 与 revision，并核对 Agent hello 同时发布 `press_key`、
键名和全部修饰键，再发送一次无 target 的 perform。成功证明 KeyDown/KeyUp 均被 UIX 组件消费并完成
settle，但 Agent v1 不公开业务状态读回，因此固定 `effectConfirmed=false`、
`finalStateReached=false`。任一事件可能已派发后的 `not_interactable`、断连、`did_not_settle` 或
`outcome_unknown` 均按不可自动重试 `OUTCOME_UNKNOWN` 处理。

```text
ai-computer-toolkit run app apply --capability ui.input.key@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
