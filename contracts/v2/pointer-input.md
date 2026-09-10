# `ui.input.pointer@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

版本二通过统一 `app.apply` surface 向显式启用 UIX Agent 的精确应用窗口发送应用内部指针事件，
不改写 Windows 主机前台物理指针语义的 `ui.input.pointer@1`。

输入只接受 `client-logical-px` 中 0..65535 的有界坐标，以及 `move` 或左键 `click`。`move` 只派发
应用内部 PointerMove；`click` 在同一 UI turn 内按顺序派发 PointerDown 与 PointerUp。版本二不接受
独立 down/up、拖拽、滚轮、双击、右键或中键，也不移动桌面光标；这些缺口不得借用桌面输入、
RemoteDesktop、X11/XWayland 或 compositor 私有协议补齐。

调用必须在读取输入文件、目标解析或端点访问前具备逐操作 `--confirm`。该路线不需要也不接受
`--allow-foreground`：事件只进入目标 UIX 应用自己的 UI 树，不激活宿主窗口或接触桌面指针。
`same-session-no-focus` 不满足 strict isolation，严格请求失败闭合。

Adapter 重新认证当前 `s2:w` generation/revision，并核对 Agent hello 发布对应 targetless 窗口动作。
成功证明声明的 UIX 事件均被组件消费并完成 settle，但不证明业务效果或最终状态，因此固定
`effectConfirmed=false`、`finalStateReached=false`。未处理的 move 可证明没有组件消费并返回
`POINTER_DISPATCH_FAILED`；click 的未处理错误可能发生在 PointerDown 已消费而 PointerUp 未消费之后，
因此保守返回不可自动重试的 `OUTCOME_UNKNOWN`。发送后断连、`did_not_settle` 或
`outcome_unknown` 同样不可重试。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
