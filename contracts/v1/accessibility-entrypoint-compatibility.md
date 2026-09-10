# Accessibility 只读入口兼容

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

Rust 是 `accessibility.tree.read@1` 的主实现；C++ 保留为反向迁移期对照证据。
公开树结果由 `contracts/v1/accessibility-tree.schema.json` 约束。

Rust 与 C++ 的 `uia` / `accessibility` 是现有调用方的受限只读别名，映射到同一个
`DiscoveryModule` 和 `act/observation-worker/v1`：

- `status uia`
- `sessions uia --max-items 1..4096`
- `inspect uia --target sessionId=<s2:w:opaque> --timeout-ms 1..30000`
- `inspect-tree uia --target ... --view control|raw`

别名不建立新的 UIA provider、session 命名空间或写路径。目标仍来自
`window.discover@1` 的 opaque window session；inspect/tree 在 Job-bounded worker
内重新解析目标。

旧 Rust 兼容入口的 `uia:window:<hwnd>` 已移除；HWND/PID、原生 provider identity
不进入任一公开契约。树等价只比较公开 Control/Raw 节点事实，UIA 不读取 Value/
Text/bounds，不查询写 pattern，不允许 Invoke/SetValue/SetFocus。Rust 主进程以
`CREATE_SUSPENDED` 创建 worker，先绑定 `KILL_ON_JOB_CLOSE` Job 再恢复；deadline 与
cancellation 均在返回前终止 Job、等待退出并回收管道。
