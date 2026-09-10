# 语言候选互操作探针

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本目录只用于 C++、Vlang、Zig 的可逆选型验证，不是第二套生产实现，也不进入
发布物。三个探针遵循同一份
`contracts/v1/language-probe-result.schema.json`，仅执行以下只读操作：

- ToolHelp 进程快照；
- `EnumWindows` 顶层窗口枚举；
- 初始化 COM 并创建只读 UI Automation 客户端；
- 验证探针前后前景窗口未变化。

探针禁止激活窗口、发送输入、修改 UIA 控件、写剪贴板或启动目标应用。Vlang
探针刻意保留其所需的窄 C 适配层，以量化 Windows COM/UIA 互操作维护成本。

运行：

```powershell
.\tools\Test-LanguageCandidates.ps1
```

胜出语言经用户确认后，生产纵切必须实现正式 capability 契约和兼容 CLI；不得
直接把探针目录当作新主工程。
