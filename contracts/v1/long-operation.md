# 长操作任务句柄与生命周期契约

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 当前交付边界

本契约冻结 `GC-OP-001A`（Vikunja #2013）的公开句柄、状态机与内部 broker
frame。`GC-OP-001B1`（#2016）已实现原子单记录 journal 与有界 registry，
`GC-OP-001B2b`（#2020）已实现固定同会话 broker，`GC-OP-001B3`（#2018）已接入主
launcher 的跨调用查询/取消。`GC-OP-001C`（#2015）第一批已启用 broker 内部
`window.record@1` 异步 submit、broker-owned Rust worker 与终态回收；第二批已发布主
launcher 的 `operation start`。同步 `window.record@1` 继续保留，异步调用方必须使用本契约的
固定 start 语法与返回的 operation handle，不得直接连接内部 broker frame。

## SMC 所有权

- `ComputerControlSystem` 只编排 submit Command、status/await Query 和 cancel Command。
- Long Operation Module 唯一拥有任务状态机、dispatch 事实、取消竞争、终态和恢复语义。
- 私有 handle registry/journal Component 只负责有界存储、索引、到期与原子持久化，不拥有
  `window.record@1` 领域行为。
- 固定 Rust broker 进程拥有 registry、worker 子树与退出清理；CLI 客户端断开不转移所有权。
- 该工作流使用显式 Command/Query，不以 EventBus 充当任务队列、结果通道或生命周期所有者。

## 句柄与作用域

任务句柄是 `s2:o:<16 位小写十六进制>`。私有身份必须同时绑定当前登录会话的 broker
代际和系统随机 nonce，再通过共享 opaque ID Component 生成。句柄不包含 PID、路径、用户名、
native handle 或 provider 类型，也不是权限凭据。broker 只在创建它的同一登录会话、同一
完整性边界内解析；broker 代际不匹配、记录已过期或零/多命中统一返回
`OPERATION_NOT_FOUND`，不得搜索其他会话或猜测旧句柄。

## 状态机

公开状态固定为：

1. `accepted`：broker 已原子建立持久记录和 handle，但尚未越过 dispatch 事实点；
2. `running`：worker dispatch 已开始，业务动作可能发生；
3. `cancel-requested`：取消已经持久记录并传播，但尚未取得可靠终态；
4. `completed`：worker 与领域 Module 已证明成功并给出有界结果；
5. `failed`：已证明失败，或 dispatch 前已证明停止；
6. `outcome-unknown`：dispatch 后因 broker/worker/主机中断无法证明最终结果。

`completed`、`failed`、`outcome-unknown` 是不可覆盖的终态。取消是幂等 Command：首次把
`accepted` 或 `running` 变为 `cancel-requested`，重复调用只返回相同事实，终态调用保持原终态。
取消请求本身绝不伪造 `completed` 或 `failed`；只有 worker/Module 的可靠证据才能终结任务。
若完成与取消竞争，实际完成证据可以从 `cancel-requested` 进入 `completed`。

## 接受、重试与未知结果

- transport/envelope 失败：`transportAccepted=false`、`businessAccepted=false`，不回显不可信
  request nonce，也不建立 handle。
- 版本、确认、容量、输入或 Policy 在持久接受前失败：
  `transportAccepted=true`、`businessAccepted=false`，不建立 handle。
- 返回 handle 后：`acceptedMayHaveOccurred=true`。调用方必须查询该 handle，不能自动重复提交。
- `dispatchStarted=false` 且终态为 `failed` 时，`retrySafe=true`；这是唯一允许安全重提原写操作的
  任务终态。
- `dispatchStarted=true` 的超时、broker 崩溃、worker 失联或清理不确定必须收敛为
  `outcome-unknown`、`retrySafe=false`，不得声称“未执行”或自动重试。

## 有界资源与清理

- 每个 broker 最多同时执行 4 个任务，最多跟踪 128 条记录。
- 每个终态 JSON 结果的 UTF-8 编码最多 1,048,576 字节；超限任务以
  `OPERATION_RESULT_TOO_LARGE` 失败，不截断成伪合法结果。
- 终态记录固定保留 86,400 秒；到期先从可查询索引撤销，再清理 journal/result/staging。
- 客户端断开后，已接受任务继续由 broker 拥有；未完成接受的请求不建立 handle。
- broker 正常退出时先停止接收、持久记录取消意图、终止并等待其 Job 约束的 worker 子树，再
  刷新终态并清理 staging。进程异常退出后，下一 broker 恢复：未 dispatch 记录转为可安全重试的
  `failed`；已 dispatch 非终态记录转为 `outcome-unknown`。
- registry 容量不足时在业务接受前返回 `OPERATION_CAPACITY_EXHAUSTED`，不得驱逐活动记录或
  静默扩大内存、磁盘和进程预算。

## 私有 journal 与 registry

- journal 使用 `act/long-operation-journal/v1` 私有单记录文档；文件名只包含 operation
  指纹，正文只保存 handle、固定 capability、状态、dispatch/cancel 事实、修订、时间、
  有界 result 或稳定 error，不保存 target、input、公开产物路径、PID 或 native handle。
- 每次迁移先构造并验证完整候选，再使用项目统一的同目录 `CREATE_NEW` staging、
  `sync_all` 和 `MoveFileExW` write-through 原子替换；提交失败时内存索引保持旧事实。
- 启动严格拒绝未知文件、链接、特殊项目、损坏 JSON、未知版本、文件名/正文 handle 不一致、
  非法状态组合和超限文档；只清理完整匹配自有命名规则的 stale staging。
- registry 查询使用内存索引，不扫描磁盘；status 与重复 cancel 不增加 revision，也不改变
  `expiresAt`。终态到期先撤销内存索引，再幂等删除固定 journal 记录。
- broker 启动加载后，在对外接收请求前把 dispatch 前非终态原子收敛为可安全重试的
  `failed`，把 dispatch 后非终态原子收敛为 `outcome-unknown`；恢复记录本身同样受固定
  修订、时间与终态保留不变量约束。

## 固定 broker 协议

内部 frame 使用 `act/long-operation-broker/v1`，权威 schema 为
`contracts/internal/long-operation-broker-v1.schema.json`。每条连接只传一个有界 JSON frame，
请求关联值是 32 位小写十六进制 nonce。内部 action 只有 `submit`、`status`、`cancel`；公开
`await` 由客户端在一个单调总 deadline 内组合多次 `status`，不新增 broker action：

- `submit` 当前只接受 `window.record@1`、单字段 canonical `s2:w:*` 目标、既有
  `window-record-input.schema.json` 输入和布尔 `confirmed=true`；
- `status` 与 `cancel` 只接受 canonical `s2:o:*`，不得携带 capability、target、input 或确认；
- 确认必须先于 target、input 和路径的语义解析；所有字段组合失败闭合；
- #2019 已把本机 message pipe 收敛为封闭 endpoint kind：长操作与独立交互会话使用不同的
  编译期固定名称，只附加内核 session ID；当前用户+SYSTEM DACL、首实例、local-only、
  deadline/cancel 与 peer PID 查询共享同一窄 Component。交互 endpoint 保持 256 KiB，
  长操作 endpoint 固定为 1,179,648 字节，以容纳完整 1 MiB 结果及状态 envelope。
- fixed sibling peer 认证同时核对完整镜像、精确 session、当前用户 SID 与完整性 RID；
  不允许请求提供 endpoint、path、argv、shell、PID、SID、RID 或 native handle。
- #2020 broker 先取得 `FILE_FLAG_FIRST_PIPE_INSTANCE` 所有权，再解析当前用户
  LocalAppData Known Folder 并恢复 journal；第二 broker 无权并发恢复 live 记录。它仅接受
  固定主程序 sibling 的同 session、同 SID、同完整性 peer，每连接一帧并使用有界 deadline。
  `status`/`cancel` 已接入 registry，客户端断开不会取消已接受任务。
- #2015 第一批把合法 `submit` 接入固定 Rust 录制 worker：ComputerControlSystem 在业务接受前
  执行 Policy、mutation 权限和无 I/O 领域验证；Recording Tasks Module 先原子建立 handle，
  再持久记录 dispatch 并执行 worker。逐任务取消、worker panic、broker 关闭和 terminal result
  均收敛到同一 registry；任一持久迁移失败都停止接受并由下一 broker 代际保守恢复。

公开状态权威 shape 为 `long-operation-status.schema.json`。状态查询与 await 是无副作用 Query；
取消是幂等 Command。三者都不延长终态保留期，避免客户端轮询制造无限生命周期。await 只在
`completed`、`failed` 或 `outcome-unknown` 时返回成功；等待 deadline 到达只终止 waiter，并
返回 `operationContinues=true`、`operationCancelled=false`，不得改写任务状态或发送隐式 cancel。

## 主 launcher 客户端

- 公共 CLI 发布
  `operation start window.record@1 --target sessionId=<s2:w:*> --input <file|-> --confirm`、
  `operation status <s2:o:*>`、`operation await <s2:o:*>` 与
  `operation cancel <s2:o:*>`。start 只允许再携带 `--timeout-ms`/`--pretty`；
  status/await/cancel 只允许 `--timeout-ms`/`--pretty`，不接受 target、input 或 confirmation。
- 客户端先连接当前 session 固定 endpoint；不存在时只启动同目录、无参数的固定 Rust broker
  sibling。并发 launcher 可以同时尝试启动，但只有 `FILE_FLAG_FIRST_PIPE_INSTANCE` winner
  能进入 journal 恢复，其余进程结构化退出。
- 每次连接后重新核对 broker 完整镜像、精确 session、当前用户 SID 与完整性 RID；响应必须
  严格匹配 broker/status 版本、本次随机 nonce、接受阶段与公开状态跨字段不变量。
- status 与 cancel 都可在 transport 断开后以同一 nonce 有界重发一次：status 无副作用，cancel
  由 registry 保证幂等。重连不搜索其他 session，不触发本地 provider、前景输入或 C++ 回退。
- await 每轮生成新的 status nonce，并把启动、认证、查询和 50 ms 轮询间隔全部扣在调用方的
  同一 1..30000 ms 单调 deadline 内；后续轮询只能取得更短剩余预算。Ctrl+C、waiter 退出或
  await timeout 不发送 cancel，原任务继续由 broker 拥有，调用方可再次执行无副作用 await。
- start 在连接完成前失败可以安全返回；一旦开始写入 submit frame，任何写入失败、断连、timeout
  或取消都返回 `OUTCOME_UNKNOWN`、`businessAcceptedMayHaveOccurred=true`、`retrySafe=false`，
  不自动重连或重发。收到成功响应后，调用方只使用返回的 `s2:o:*` 查询，不重复提交原操作。
- launcher 退出不拥有 broker 生命周期；broker 重启继续按固定 journal 的 dispatch 前/后恢复
  语义收敛记录。
