# Linux 精确进程温和终止 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`process.terminate.graceful@2` 是 `process.run terminate-graceful` 上的 Linux 独立
capability。它不复用或扩写 Windows `process.terminate.graceful@1` 的顶层窗口关闭语义；
固定通过 pidfd 向同一非 root UID 的精确进程代际提交 `SIGTERM`。公共输入和结果不出现
PID、UID、signal number、fd、路径或 procfs 字段。

## 确认、输入与目标

确认必须先于 input 文件读取、目标解析、procfs 枚举和 pidfd 打开。输入严格匹配
`process-termination-graceful-input.schema.json`，只接受总 deadline `timeoutMs=1..30000`
（默认 5000）；不接受信号、PID、进程名授权、自动强制升级或 shell 参数。目标必须是当前
`process.sessions` 返回的 canonical `s2:p:<opaque>`，名称只参与 opaque 代际构造，不参与
授权。

`s2:p` 私有身份材料额外绑定 procfs owner epoch；它由 canonical kernel boot ID 与当前
PID namespace 的 nsfs device/inode 组成，跨重启或跨 PID namespace 的旧目标自动 stale，
但 owner epoch 不进入公共 JSON。Adapter 在提交前重新枚举有界完整进程清单，唯一解析 opaque ID，
打开 pidfd 后再次核对 owner epoch、start ticks、session ID、`/proc/<pid>/status` 的四项 UID 与 NSpid 链。清单不完整时不把
未命中误报为 stale；当前工具、检查时 procfs 可见的祖先、任一命名空间 PID 1、其他 UID、
root 或带 `CAP_KILL` 执行、保护状态未知均失败闭合。没有 PID 重用或同名进程回退，也不
声称保护不可见命名空间外的祖先。

## 接受、等待与失败语义

Module 只发送一次固定 `SIGTERM`，不回退 `SIGKILL`，也不自动重试。提交前 deadline 或错误
返回可安全重试的确定失败；内核接受后使用同一 pidfd 有界等待退出。确认 pidfd readable
才返回 `finalStateReached=true`；procfs 发现会排除 zombie/dead 终态，因此同一 opaque ID 的
重新观察为 absent，成功结果同时声明 `processOwnerGenerationBound=true`。提交后 deadline 或
等待错误统一返回 `OUTCOME_UNKNOWN`，
声明 `acceptedMayHaveOccurred=true`、`retrySafe=false` 与 `automaticRetryProhibited=true`。

该首批同步 route 只有单一总 deadline，尚不提供异步 cancellation。调用方可使用原 opaque
ID 通过 `process.discover@1` 重新观察；不得因为超时或未知结果再次自动发送终止请求。
