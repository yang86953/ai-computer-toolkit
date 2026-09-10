# 独立交互会话 Command Worker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 边界

`act/interactive-command-worker/v1` 是 Interactive Isolation Module 与固定 Rust command
worker 之间的一次性 JSON over stdio 协议。它是固定接收者的 D3/R0 Command，不是
EventBus、远程任意命令入口或公开 provider API。生产环境只能由已通过 OS 对等身份、授权
会话、部署清单和 endpoint lease 认证的 session broker 创建该 worker；调用方不得指定
可执行文件、路径、pipe、端口、argv、shell、凭据或原生 session 身份。

`requestNonce` 只做请求关联，`endpointLeaseNonce` 只把一次 worker 生命周期绑定到 broker
已建立的 lease；二者都不能替代 OS 访问控制、对等进程认证或固定构建身份校验。所有 nonce
必须是 32 位小写十六进制随机值，不能由 session number、用户名、SID、PID、路径或目标
身份派生，两种 nonce 也不得复用同一个值。

## 请求

请求形状由 `interactive-command-worker-request-v1.schema.json` 冻结。host 必须同时冻结：

- `isolationRequirement=strict`；
- `requiredExecutionRealm=isolated-worker`；
- `hostImpactPolicy=strict-no-interference`；
- capability、统一 operation、确认、只作用于 worker 会话的前景许可、deadline 和原
  canonical `s2:w`；
- capability 输入对象中任何层级都不得出现 `interactiveSessionId`，worker 不接受递归
  endpoint 选择；input 顶层也不得重复 `timeoutMs`，完整生命周期只认请求顶层的剩余
  deadline，command worker 在装配既有 Module 时负责把该预算注入领域输入。

v1 只接受四条 provider-neutral 路线：

| capability | operation | 独立会话前景许可 |
| --- | --- | --- |
| `ui.input.key@1` | `apply` | 必须 |
| `ui.input.pointer@1` | `apply` | 必须 |
| `window.lifecycle@1` | `apply` | 必须 |
| `window.close@1` | `close` | 不要求激活 |

确认先于协议版本之后的 capability、target 和 input 语义验证。完整请求最大 128 KiB，
`timeoutMs` 为 1–30000；broker 必须从自身请求 deadline 扣除握手和启动耗时，不能给 worker
重新开始一个更长的预算。

## 接受、完成与失败

响应形状由 `interactive-command-worker-response-v1.schema.json` 冻结，并分开表达：

- `transportAccepted`：完整协议帧是否被 worker 接受；
- `businessAccepted`：mutation 是否已经可能越过 dispatch 点；
- `completed`：capability 是否得到确定完成结果；
- `outcome`：`not-dispatched`、`completed` 或 `unknown`。

协议、确认、认证、stale、歧义或其他 dispatch 前失败必须是
`businessAccepted=false`、`completed=false`、`outcome=not-dispatched`、`retrySafe=true`、
`targetMayHaveMutated=false`。一旦 mutation 可能被接受，timeout、取消、断线、worker crash、
broker crash 或无法验证的响应必须由 Module 映射为 `OUTCOME_UNKNOWN`，并固定
`businessAccepted=true`、`completed=false`、`retrySafe=false`、
`targetMayHaveMutated=true`；调用方不得自动重试。

command worker 只返回自己会话内的业务结果和 best-effort 输入释放证据。host 前景、光标、
输入状态未变以及本地 provider 零调用由 host System 复核并投影，不能信任 worker 自报。

## 当前迁移状态

固定 Rust worker 现在只接受由同安装 session broker 创建、且父进程镜像、Windows session
与同一用户主体均通过 OS 认证的 stdio 生命周期。父进程认证完成后，worker 移除递归
endpoint 字段，恢复唯一剩余 deadline，并在自己的交互会话内装配既有
`ComputerControlSystem` 执行四条通用 mutation；结果会剥离内层 System 执行计划证明，
由 host System 重新投影最终隔离证明。

broker、endpoint、lease 和 Job 路由的 wire 边界见
`interactive-session-broker-v1.md` 与 `interactive-session-broker-v1.schema.json`。当前结果
是 `ISO-B`/`ISO-C` 生产候选；`ISO-D` 的异常恢复矩阵和无 endpoint 生产 launcher 已完成，
真实授权 endpoint 的跨系统 session 技术执行证据仍在 #2001。#2008 的真实视觉/交互与
零干扰验收仍只属于 `yang86`。
