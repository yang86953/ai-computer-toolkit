# Sequence 执行预算内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 所有权

`ComputerControlSystem` 保持 `sequence` 为显式同步 Workflow。Workflow 拥有步骤顺序、前置条件、绑定、后置条件、终止决定和聚合输出。`sequence_execution_budget` 是窄的 provider-neutral Component：它只拥有单调 deadline 运算并报告停止观察，不拥有 Workflow 状态、provider dispatch、取消状态存储、重试、回滚或结果映射。

该内部契约由 Vikunja #2021 冻结。它不是公开 capability，也不凭自身宣称支持 deadline；Vikunja #2025 已在不扩大该 Component 所有权的前提下，同时更新公开 schema、System/Workflow 路径、固定 worker 路由与回归证据并发布新语义。

## 边界

- 后续公开总 Workflow 输入限制为 `1..=1_920_000` 毫秒，默认 `300_000` 毫秒。
- 后续公开逐步输入限制为 `1..=30_000` 毫秒，默认 `30_000` 毫秒。
- 步骤从当前单调时间开始，但交给 worker/Module 的有效预算取不可变 Workflow deadline 与当前步骤 deadline 的较早者。开始新步骤不得重置 Workflow deadline。
- 剩余时长向上取整为下一个协议毫秒。deadline 已耗尽时不返回剩余预算，不得把它转换成虚构的一毫秒执行窗口。

总预算最大值等于 64 步乘以现有公开操作的 30 秒上限。五分钟默认值让旧调用方获得有限边界，同时避免每个省略的逐步默认值为 Workflow 重开无限窗口。

## 停止观察

每次观察接收当前单调时间、调用方拥有的取消事实和最后可靠的 dispatch 阶段。它最多返回一个原因，优先级固定为：

1. `Cancelled`；
2. `WorkflowDeadlineExceeded`；
3. `StepDeadlineExceeded`。

dispatch 阶段只有 `BeforeDispatch` 和 `AfterDispatch`。Component 保留调用方提供的阶段，不推测 provider 结果。Workflow 映射必须把 dispatch 前停止视为步骤未启动；dispatch 后停止不能报告为未执行或成功。除非 provider 给出更强的终态事实，否则必须映射为 `OutcomeUnknown`，同时保留 `retrySafe=false`、`acceptedMayHaveOccurred=true` 和最后可靠观察。

## 传播边界

生产批次必须把较早的有效 deadline 与取消从 CLI/System 传到隔离 worker 或拥有行为的 Module。只在无界同步 provider 调用前后检查时间不构成硬超时。禁止使用 Workflow 返回后仍继续运行的脱离线程。

worker 协议必须在任何 mutation 前建立可靠的 dispatch 前/accepted 边界。该边界后的取消或 deadline 必须保留不确定性、停止后续步骤，并且永不自动重试。只有当只读 Module 能证明没有 mutation 被接受时，读取操作才可返回确定的取消或超时。

Vikunja #2022 已在 `sequence-step-worker-v1.md` 冻结 request/cancel、dispatch-accepted 与 final 的双阶段 JSON Lines 状态机。Vikunja #2023 已实现固定 Rust worker、进程内取消发布和 System dispatch hook；#2024 已实现 Job-bound 流式 runner，并覆盖 dispatch 前/后挂起、取消竞争、成功终帧与输出上限；#2025 已完成 Workflow 聚合与公开 deadline 输入。

该契约不引入 EventBus、跨 System 消息、通用事务、隐式重试、公开 native handle、任意进程路线或前台回退。
