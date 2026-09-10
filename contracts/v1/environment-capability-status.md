# Environment capability status v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`status browser|win32-control|notepad|desktop` 只探测当前实现可观察到的运行环境，
不执行任何对应 operation。browser 已使用 Rust；其余字段保留各自兼容状态。结果必须
同时报告：

- `readOnly:true`；browser 用 `implementation:rust-isolated-worker` 与
  `available/runtimeDetected/workerAvailable` 报告当前事实；notepad/desktop 继续
  `cppExecutionEnabled:false`、`writesEnabled:false`；browser 与
  win32-control 在各自正式门禁通过后报告两者为 `true`；
- 检测前后前台不变；
- 不返回浏览器、Notepad、编码器或 worker 路径；
- 不返回 HWND、PID、className 或 provider identity；
- runtime detected 不得解释为 capability 已认证。

browser status 只检查已安装的 Chromium/Edge runtime，不访问 profile、标签页、
凭据或扩展；实际 screenshot 另需确认，并只启动隔离临时 profile worker。
notepad 只做可执行文件发现，不启动进程。desktop 只枚举 Media Foundation H.264 MFT，
不创建文件或启动编码会话。win32-control 的 C++
status 汇总公开窗口目录中的标准 Edit 数量，以及 `requires-confirmation/
permission-blocked/indeterminate` 计数；不返回目标 ID，`activeWriteProbes`
固定为 0。消息发送只经精确 `s2:c:*`、确认与静态权限 facade 开放。desktop 可报告 capture worker 是否随包
存在和 `mediaFoundationH264Available`；screenshot 与 recording 都固定进入确认后的
Rust 隔离 worker 路由。
