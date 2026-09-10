# Sequence step worker 内部协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 定位与所有权

`act/sequence-step-worker/v1` 是主进程 sequence Workflow 与固定隔离 worker 之间的私有 JSON Lines 协议。Windows 使用固定 sibling，Linux 使用当前主映像的固定隐藏自进程入口；二者不改变协议字段。协议 Component 拥有封闭字段、字节边界、关联值和帧状态机；`ComputerControlSystem` 仍拥有 Policy、权限、执行计划与 dispatch 决定；worker/Module 拥有实际执行和清理；Workflow 拥有步骤顺序、停止与聚合结果。

该协议由 Vikunja #2022 冻结。Vikunja #2023 已提供固定 Rust worker 与 `ComputerControlSystem` 私有 dispatch hook；Vikunja #2024 已实现并以自包含 Rust fixture 验证复用现有私有 Win32 原语的 Job-bound 流式 runner；Vikunja #2025 已把生产 Workflow 与公开 `sequence` schema 接入该链路。2026-08-30 的 Linux L5-B 复用同一协议和 Workflow 聚合，runner 只执行 `/proc/self/exe` 的编译期固定隐藏参数，创建独立 process group、安装父进程死亡回收、限制 stdout/stderr，并在取消或 deadline 后强制回收；Linux release 因而仍是 1 CLI/0 companion。协议本身不是公开 capability，也不允许任意 path、argv、shell、native handle、第三方对象或前台回退。

## stdin

第一行必须是唯一 request：

```json
{"contractVersion":"act/sequence-step-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","timeoutMs":2500,"command":{"verb":"status","app":"desktop","operation":null,"target":{},"args":{},"maxItems":50,"maxDepth":4,"confirmed":false,"foregroundConsent":false,"isolationRequirement":"standard"}}
```

- request 紧凑 UTF-8 JSON 最多 128 KiB；未知字段拒绝。
- `requestNonce` 固定为 32 位小写十六进制一次性关联值，不包含目标、路径或身份事实。
- `timeoutMs` 是总预算与当前步骤 deadline 取较早者后的唯一剩余预算，只允许 `1..=30000`。worker 不得从本地默认值重置预算。
- `command` 与绑定后的最终 `CommandRequest` 同构；`target` 和 `args` 分别最多 64 KiB 紧凑 JSON，`app` 与可选 `operation` 分别最多 128 个 UTF-8 字节。
- target/args 只承载 provider-neutral JSON；Policy、确认、前台同意和隔离要求仍完整进入同一个 System，不因 worker 边界获得授权。

request 后 stdin 可保持打开，并且最多再接受一行 control：

```json
{"contractVersion":"act/sequence-step-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","kind":"cancel"}
```

control 必须与当前 request 的版本和 nonce 完全一致。未知 control、关联漂移、第二个 cancel 或额外行均为协议失败，不得重复触发 Module 取消。

## stdout

stdout 只允许以下两种顺序之一：

1. dispatch 前拒绝：单个 `final`；
2. provider 路线：`dispatch-accepted` 后跟单个 `final`。

`dispatch-accepted` 只能在 System 完成协议、Policy、确认、权限和执行计划门禁后，并在调用 provider 前写出并刷新。Linux 的 `process` mutation 与全部已认证 `app` capability 都进入同一个 System Policy；UIX mutation 也不得把 adapter 内确认拒绝误报成已接受：

```json
{"kind":"dispatch-accepted","contractVersion":"act/sequence-step-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","dispatchAccepted":true,"completed":false}
```

`final` 是以下封闭状态之一：

| outcome | dispatchAccepted | completed | retrySafe | acceptedMayHaveOccurred | 负载 |
| --- | --- | --- | --- | --- | --- |
| `not-dispatched` | false | false | true | false | `error` 对象，且 code 不是 `OUTCOME_UNKNOWN` |
| `completed` | true | true | false | true | `result` |
| `failed` | true | true | false | true | `error` 对象，且 code 不是 `OUTCOME_UNKNOWN` |
| `unknown` | true | false | false | true | `error.code=OUTCOME_UNKNOWN` |

完成可信 request 关联后的正常 worker 退出必须包含 final；无法建立可信版本和 nonce 的 transport 请求以非零退出且不伪造 frame。deadline、cancel、Windows Job 或 Linux process-group 回收后的部分 stdout 可以为空，也可以只有 accepted；这两种情况分别证明最后可靠观察是 dispatch 前或 dispatch 后。parser 不得从 transport 停止伪造 final：accepted-only 由 Workflow 映射为 OutcomeUnknown，零帧由 Workflow 映射为未 dispatch 停止。

两帧 stdout 合计最多 16 MiB + 128 KiB。结果不截断；超出时结构化失败并由 Workflow 的结果预算规则决定公开省略证据。

协议 Component 的封闭失败文本为 `INVALID_ARGUMENT`、`WORKER_REQUEST_TOO_LARGE`、`WORKER_OUTPUT_TOO_LARGE` 和 `WORKER_PROTOCOL_FAILED`。这些失败只描述协议或 transport，不能冒充 provider 业务结果。

## 禁止事项

- 不用 EventBus、跨 System 消息或 generic transaction 表示 request/response。
- 不自动重试 dispatch 后的失败、未知结果、timeout 或 cancel。
- 不允许 final 先于 accepted、重复 accepted/final、关联值漂移、`outcome` 与错误码漂移，或 completed/accepted/retry 字段自相矛盾。
- 不在该 Component 内创建进程、拥有 Job、保存 Workflow 状态或调用 provider。
