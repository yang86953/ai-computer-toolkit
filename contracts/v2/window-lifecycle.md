# `window.lifecycle@2`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

版本二通过统一 `app.apply` surface 控制显式启用 UIX Agent 的精确应用窗口，不改写 Windows
物理外框语义的 `window.lifecycle@1`。

Linux 当前支持四项动作：`restore`、`minimize`、`maximize` 和 `resize`。`resize` 必须显式使用
`client-logical-px`，对应 UIX 跨平台 logical 客户区尺寸；宽高范围为 1..65535。Wayland 标准协议
不允许客户端任意设置顶层窗口屏幕位置，因此版本二 Linux Provider 不接受 `move`，也不回退
X11/XWayland、compositor 私有协议或脚本。

调用必须在读取输入文件、目标解析或端点访问前同时具备逐操作 `--confirm` 与
`--allow-foreground`。工具包重新解析当前 `s2:w`，核对 Agent `hello` 正式发布对应窗口动作，随后
使用当前 generation 与 revision 发送一次无 target 的 `perform`。

Agent v1 的成功响应证明 UI turn 内的平台调用完成并提供 revision、presentedRevision 与 settle
事实，但不公开最终窗口状态或客户区读回，所以成功结果固定 `effectConfirmed=false`、
`finalStateReached=false`；调用方不得把它解释为 compositor 已确认最终效果。发送后断连、
`window_operation_failed`、`did_not_settle` 或 `outcome_unknown` 均按不可重试 `OUTCOME_UNKNOWN`
处理。

```text
ai-computer-toolkit run app apply --capability window.lifecycle@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm --allow-foreground
```
