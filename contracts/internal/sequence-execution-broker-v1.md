# Sequence Execution Broker 与 Journal 内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 状态与所有权

该契约由 Vikunja #2390 冻结，是 #2388 的首个实现输入。内部 wire 版本固定为
`act/sequence-execution-broker/v1`，持久记录版本固定为
`act/sequence-execution-journal/v1`。本批只冻结协议和停止线，不发布公开 launcher，也不声称
现有同步 `sequence` 已经可以 resume。

`ComputerControlSystem` 只协调显式 Workflow、Policy 和 Module。Workflow Execution Module 是
execution、step、revision、receipt、恢复、取消决定与结果聚合的唯一领域所有者；Sequence
Binding/Template Module 继续拥有候选请求物化，Policy 继续拥有最终请求的风险、确认、前台、
隔离与权限。Journal Codec、Atomic File、Known Folder、固定 pipe、peer authentication 和
request fingerprint 都是窄 Component。broker/launcher 只组合 System、Module 与 Component，
不得拥有第二份 execution registry、领域状态或恢复决定。本链路使用直接调用，不使用 EventBus。

可以复用既有 `atomic_file` 的同目录 CREATE_NEW staging、durable flush 和 write-through 原子替换，
以及固定 Known Folder 和 broker 认证原语；不得复用 long-operation 的 `s2:o` identity、record
schema、状态机、容量或生命周期。两个 Module 只能共享无领域含义的 Component。

## 身份、容量与规范语义

- broker 每次启动生成 32 位小写十六进制 `brokerEpoch`；重启后 wire request 必须绑定新 epoch，
  但已持久 execution 不因 epoch 变化而失效。
- execution 使用随机 `s2:q:<32 hex>`；每个 step 在首次创建 journal 时取得随机
  `s2:qs:<32 hex>`。step identity 与一基 `stepIndex` 一同持久化，恢复时不得重建或换号。
- 固定 journal 最多保留 64 个 execution；单记录最多 35,651,584 字节，目录最多扫描 256 个
  final/staging 项目。达到边界时新 start 在建立 execution 前返回
  `SEQUENCE_EXECUTION_CAPACITY_EXHAUSTED`，不得驱逐 completed、failed、cancelled 或
  outcome-unknown 记录来伪装可用容量。
- broker epoch 内 request ledger 与 cancel ledger 各最多 4096 项，terminal entry 不淘汰；容量不足
  在 business acceptance 前返回 `BROKER_REQUEST_LEDGER_FULL`。
- 显式 forget 使用独立、单文件原子 tombstone index，最多保留 4096 个 executionId、start nonce 与
  start semantic digest；达到边界时 forget 失败并保留原 journal。tombstone 不保存 input 或结果。
- start 的完整规范语义对象恰好包含 `contractVersion`、`action=start`、完整 `input`；status 对象
  恰好包含版本、`action=status`、executionId；resume/cancel/forget 对象再包含
  expectedExecutionRevision，forget 还包含 `confirmed=true`。requestNonce、fingerprint、broker epoch、
  remaining timeout 和连接事实不进入业务语义。规范 JSON 使用 UTF-8、对象键按
  Unicode code point 升序、数组保持顺序、字符串按 JSON 转义、数字使用解析后无歧义的最短表示。
- wire `semanticFingerprint` 是完整规范语义的 FNV-1a 64-bit，编码为 16 位小写十六进制；broker
  必须重算。64 位摘要只能快速校验，绝不能代替完整语义比较。
- journal 保存完整、严格解析后的 input，并保存其规范 JSON SHA-256 `inputDigest`；step 保存规范
  最终 `CommandRequest` 与 execution/step identity 计算出的 SHA-256 `semanticDigest`。digest 只做
  漂移检测，不是认证凭据，不能代替原始 input、完整语义或 Policy 决定。

start 还把 `startRequestNonce` 和完整 start 语义的 SHA-256 `startSemanticDigest` 原子写入 journal。
同一 nonce、同一完整语义在 broker 重启后必须找到并 attach 到原 execution，不得创建第二份；
同一 nonce 携带不同完整语义必须返回 `NONCE_SEMANTIC_CONFLICT`。这保证 client 在未收到
executionId 前断线时仍可用同一 start frame 找回唯一 execution。status/resume/cancel 在 broker
重启后使用已知 executionId、新 request nonce 和 expected revision；不得用旧 epoch ledger 猜测。

## Journal 与状态机

每个 execution 只对应 `<executionId>.json` 一条完整 journal。创建、状态推进和 receipt 更新都必须
先编码完整新记录，再经同目录 staging、durable flush 与 write-through rename 原子提交。只有提交
成功后内存状态才能对外可见。记录必须包含单调 `recordRevision`、完整 input、input digest、
execution state、当前一基 step、取消事实、按顺序排列的 step receipt，以及已有 Workflow 结果。
record revision 从 0 开始，每次合法状态变化严格加 1；重复 status 不推进 revision。

Windows 物理文件名固定为 `execution-<32 lowercase hex>.json`，该指纹必须与文档内
`s2:q:<fingerprint>` 逐值绑定。#2391 第二批已实现同目录 `CREATE_NEW` staging、durable flush、
write-through 原子替换、非跟随有界读取、canonical stale staging 清理、未知项目失败闭合和最多
4096 条活动记录的恢复扫描；固定 Known Folder/owner-only 目录仍由 #2391 后续批次建立。

execution 状态是封闭集合：

1. `created`：input 和全部稳定 step identity 已原子建立，尚未准备第一步；
2. `running`：当前 step 是 `prepared` 或 `dispatching`；
3. `awaiting-resume`：仅在步骤之间或 `prepared` 尚未 dispatch 的安全检查点暂停；
4. `awaiting-confirmation`：为 #2389 保留，当前 #2388 实现不得自行产生确认收据；
5. `completed`、`failed`、`cancelled`、`outcome-unknown`：不可倒退的 execution 终态。

step receipt 状态是封闭集合：

1. `pending`：稳定 stepId 已建立，尚未物化候选；
2. `prepared`：前置条件、绑定/模板、最终请求 semantic digest 与 Policy revision 已建立并持久化，
   但没有创建 worker、Job、pipe 或 provider 资源；
3. `dispatching`：固定 `dispatchNonce` 和 `acceptedMayHaveOccurred=true` 已原子持久化；只有该提交
   成功后才能创建 worker 或调用任何 provider；
4. `completed` 或 `failed`：可信 final、结果/错误、执行证据与结果预算事实已原子持久化；
5. `outcome-unknown`：dispatching 后没有可信 final，禁止重派。

`prepared → dispatching → completed|failed|outcome-unknown` 单向推进，每个 step 的
`stepRevision` 从 0 开始且每次推进严格加 1。completed、failed、outcome-unknown step 不得重新
物化、重新验证成另一语义或重新 dispatch。普通 failed step 是否继续只由原 input 的
`continueOnError` 决定；恢复不能改变该策略。后置条件和结果预算仍按公开 sequence 契约执行并
写入同一可信 receipt。

## 崩溃与恢复

broker 首先取得固定 endpoint 首实例和双向 peer authentication 所需 OS 资源，随后才扫描 journal；
第二 broker 不得并发恢复同一目录。恢复必须非跟随检查固定目录和文件，拒绝 reparse point、链接、
未知扩展、重复 identity、非法 UTF-8/JSON/schema、digest 漂移、revision 回退、step 顺序漂移、状态
组合冲突、超容量和残留 staging 无法清理。任一不可信记录返回
`SEQUENCE_JOURNAL_CORRUPT` 并阻止该 execution，不得跳过坏字段继续执行。

- `created`、`pending` 或 `prepared` 可以恢复为 `awaiting-resume`；resume 只从当前安全检查点继续，
  不重跑任何 completed/failed step。
- `dispatching` 在恢复时无论 journal 中是否已经记录 worker accepted，都必须原子推进为
  `outcome-unknown`，并使整个 execution 成为 `outcome-unknown`。因为 crash 可能发生在 provider
  接受与 accepted/final 持久化之间，禁止自动重派。
- completed/failed/outcome-unknown receipt 是不可变事实；terminal execution 只可 status 或显式
  forget，不能 resume。cancelled 只有权威停止事实才能建立，cancel request 本身不是 Cancelled。
- journal 原子提交失败时不得发布新 revision。若旧 record 仍可信，status 返回旧事实和持久化错误；
  若无法证明旧事实，execution 失败闭合为不可恢复，不能依赖进程内 map 继续。

恢复 `prepared` 时必须重新读取当前 Policy revision、授权主体、目标 freshness 和静态候选事实，
重算 semantic digest 并逐值比较。任何漂移返回 `SEQUENCE_RESUME_CONFLICT` 或为 #2389 保留的
awaiting-confirmation，不能静默重物化为另一个请求。该检查不得重跑 earlier provider step。

## Broker frame、revision 与动作

固定 broker 只接受 schema 中的 `start`、`status`、`resume`、`cancel` 和 `forget`：

- `start` 建立或 attach execution；journal 创建成功后才允许发送 business accepted。
- `status` 是只读 Query，不推进 execution/step/record revision。
- `resume` 必须携带 `expectedExecutionRevision`；只有完全相等且状态可恢复才能推进。
- `cancel` 必须携带 expected revision。Module 先持久化 `cancelRequested=true`；若尚未 dispatch 可建立
  cancelled，若已经 dispatch 只能请求协作停止并等待可信 final，否则 outcome-unknown。
- `forget` 必须携带 expected revision 和 `confirmed=true`，只允许 completed、failed、cancelled、
  outcome-unknown 终态。Module 必须先把 executionId、start nonce 与 start semantic digest 原子加入
  forgotten tombstone index，再删除完整 journal；tombstone 提交或 journal 删除失败都不得报告 forget
  成功。成功后 identity 永久 stale；同 start nonce/同语义返回 `SEQUENCE_EXECUTION_FORGOTTEN`，
  同 nonce/异义仍返回 `NONCE_SEMANTIC_CONFLICT`，绝不能创建第二份 execution。

#2391 第四批已实现可信 success/failure final、按持久 `continueOnError` 推进、取消请求和权威取消
终态：未 dispatch 可直接 Cancelled，已 dispatch 只先保存取消意图，必须收到可信 worker 停止 final
才能 Cancelled。永久 forgotten index 固定为 `forgotten-v1.json`，最多 4096 条且 revision 等于追加
记录数；同 nonce 同义重送不推进 revision，同 nonce 异义优先冲突。journal 已能独立原子替换并在
同一次严格扫描中返回该保留索引；生产 forget 的“先提交 tombstone、后删完整记录”顺序仍由 #2391
最终 Workflow Execution Module 组合负责。

每个 wire request 都携带 32 hex `requestNonce`、16 hex `semanticFingerprint`、当前
`expectedBrokerEpoch` 和 `remainingTimeoutMs=1..=30000`。broker-ready、accepted、final 都回显 epoch。
同 epoch 同 nonce 同完整语义只能 attach/replay；同 nonce 异义返回 `NONCE_SEMANTIC_CONFLICT`。

业务接受后的首帧固定为 `accepted(requestRevision=0)`，只在 start 已建立 durable journal、或其他
动作已经与既有 execution/revision 线性化后发送。随后唯一 final 固定
`requestRevision=1`。业务前 rejected 可以是唯一 revision 0 final。低 revision、同 revision 异义、
final 后帧、跳过 accepted 的业务执行结果都属于协议污染。响应同时携带当前
`executionRevision` 和隐私安全 snapshot；request revision 与 execution revision 不得混用。

client 拥有单次调用的单调绝对 deadline，覆盖 connect、双向认证、broker-ready、frame、journal、
Module、worker/Job 和响应聚合。重送只能缩短首次 deadline，不能延长或重置。连接在 accepted 前
断开只说明本次 client 未取得结果；start 可按相同 nonce/语义 attach，其他动作以 status 和 journal
为权威。execution 中已有 dispatching receipt 而缺少可信 final 时，最终只能是 OutcomeUnknown，
不得因 cancel、deadline、断线、broker crash 或 launcher crash 伪造 Cancelled 或自动 retry。

## 存储、认证与隐私

生产 broker、client 和 launcher 只能使用编译期固定 sibling、固定本机 pipe 与当前登录会话；不得
接受 path、argv、shell、环境变量 endpoint、端口扫描、前台输入 fallback 或外部 broker。pipe 必须
拒绝远程 client，双向认证把 PID、token session、镜像和 token 事实绑定在同一已打开进程对象上；
broker 必须首实例，launcher 不拥有 journal。

journal 位于当前用户不可由环境变量覆盖的 LocalAppData 固定叶目录，目录与文件必须是 owner-only、
非 reparse 的真实对象。输入、步骤结果和动态来源可能敏感，只能进入 journal 和版本化、受结果预算
约束的 Workflow 结果；日志、错误、fingerprint、文件名、identity 和非结果状态证据不得额外回显
literal、来源值、凭据、路径、token、原生 identity、完整 CommandRequest 或 journal 内容。
schema/digest/认证失败只报告安全类别、execution/step/revision 和字节边界。

#2391 第三批已固定使用 Known Folder API 解析的当前用户 `LocalAppData`，并在
`ai-computer-toolkit/sequence-execution-v1` 两级固定目录创建时安装受保护 DACL；owner 为当前用户，
ACE 只授权当前用户与 LocalSystem 完全访问并继承到子项。既有目录会幂等加固并回读比较 owner、
完整 DACL 字节和 protected 标记；任何链接、junction、reparse、普通文件占位或权限漂移均失败闭合。

## 依赖与明确非目标

- #2390：冻结本协议、两个 schema 和契约门禁。
- #2391：分批实现 Workflow Execution Module、journal Component 与恢复状态机；首批已建立严格
  record codec、稳定 execution/step identity、单调 revision、prepared 恢复冲突检查，以及
  dispatching 中断永久收敛为 OutcomeUnknown；第二批已建立 canonical identity 共享 Component 和
  原子多记录 journal 文件生命周期；第三批已建立固定 owner-only LocalAppData Known Folder 目录。
  第四批已补齐可信终态、cancel 迁移和 forgotten tombstone 原语。完整 SequenceInput 恢复复验、
  tombstone-before-delete forget 编排和生产 Module 组合仍由 #2391 后续批次完成。
- #2392：实现固定 broker、认证、request ledger 与动作协议。
- #2393：实现 client、生产 launcher、打包和一次调用 deadline。
- #2394：保持同步 sequence 向后兼容并发布 start/status/resume/cancel/forget 与生产 E2E。
- #2389：只能在上述稳定 receipt/resume 已完成后增加动态物化摘要和确认收据。

本契约不承诺通用事务、自动重试、补偿、临时资源逆序清理、跨用户/跨会话恢复、网络 broker、
任意脚本、秘密回显或把 OutcomeUnknown 修正为成功/失败。显式 forget 只释放本工具的 journal 容量，
不撤销外部副作用，也不证明未知操作从未发生。
