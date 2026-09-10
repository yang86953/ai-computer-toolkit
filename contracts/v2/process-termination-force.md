# Linux 精确进程强制终止 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`process.terminate.force@2` 是 `process.run terminate-force` 上的 Linux 独立 critical
capability。它不复用 Windows `process.terminate.force@1` 的 integrity/critical 语义，也不是
`process.terminate.graceful@2` 的 fallback。调用方必须显式选择 `terminate-force` 并逐操作确认；
input 不能把温和路线升级为强制路线。

## 身份、保护与输入

公开目标仍是 canonical `s2:p:<opaque>`。Linux 私有身份材料同时绑定 procfs owner epoch、PID、
start ticks 与进程名；owner epoch 由 canonical boot ID 与当前 PID namespace 的 nsfs device/inode
组成，只参与私有 hash。boot ID、namespace 身份、PID、UID、signal number、fd、路径和 procfs
字段都不进入公共 JSON。Adapter 用有界完整快照唯一解析目标，打开 pidfd 后重新读取并
逐项核对 owner epoch、session ID、start ticks、四项 UID 与 NSpid；跨重启旧 ID、PID 重用、同名
进程、清单不完整或任何代际漂移都失败闭合。

当前工具必须是同 UID 非 root、四项 UID 一致且不带 `CAP_KILL`。PID 1、任一 namespace PID 1、
当前工具、检查时 procfs 可见祖先、其他 UID 与保护事实未知均拒绝。确认必须先于 input 文件读取、
target/procfs/pidfd 访问。输入严格匹配 `process-termination-force-input.schema.json`，只允许
`timeoutMs=1..30000`（默认 5000）；不接受 signal、PID、进程名授权、shell 参数或 fallback 开关。

## 接受与终态

Module 只向同一 pidfd 提交一次固定强制终止，不先尝试温和终止，也不自动重试。内核接受后只在
同一 pidfd 观察到退出时返回 `completed`；结果固定 `riskLevel=critical`、
`processOwnerGenerationBound=true`、`gracefulAttempted=false`、`forceWasExplicitlySelected=true` 与
`forceFallbackFromGraceful=false`。提交后 deadline 或等待错误统一返回 `OUTCOME_UNKNOWN`、
`acceptedMayHaveOccurred=true`、`retrySafe=false`，调用方只能重新观察原 opaque ID。

该同步路线当前没有独立异步取消；若由 sequence Workflow 调用，Workflow 仍按 accepted/final
协议保守聚合。机器验证只允许终止项目自己创建且已完成 ready 握手的无窗口测试子进程，不操作
用户进程或系统服务。
