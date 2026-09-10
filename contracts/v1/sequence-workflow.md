# Sequence 工作流契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 定位

`sequence --input <file|->` 是 `ComputerControlSystem` 的显式同步 Workflow。它按输入顺序调用统一公开控制请求，不绕过 assessment、确认、前台同意、隔离或权限策略。

本版本的契约 ID 是 `act/sequence-workflow/v1`，输入 schema 是 `sequence-input.schema.json`。顶层和每个 step 都是封闭对象；未知字段必须在任何步骤执行前以 `INVALID_ARGUMENT` 拒绝，不能静默忽略。

`totalTimeoutMs` 允许 `1..=1920000`，默认 300000；每个 step 的 `timeoutMs` 允许 `1..=30000`，默认 30000。当前步骤使用两者剩余时间的较早者，步骤切换不得重置总预算。Windows 每步由固定 `ai-computer-toolkit-sequence-step-worker.exe` 在 Job 中执行；Linux 每步由当前主映像经固定 `/proc/self/exe __sequence-step-worker-v1` 隐藏入口在独立 process group 中执行，不接受调用方路径或额外 argv。两种平台都复用同一双阶段协议，取消优先于同次观察中的 deadline。

## 输入边界

- `steps` 必须包含 1–64 项。
- `continueOnError` 默认 `false`，只允许普通 provider 步骤错误后继续。
- `maxResultBytes` 默认 1 MiB，可配置范围为 256 字节至 16 MiB。
- `maxResultBytes` 统计已纳入 `results` 的紧凑 JSON `result` 或 `error` 负载 UTF-8 字节数；步骤 envelope、预算证据和工作流错误不计入该预算，使耗尽错误始终可以返回。
- 输入 JSON 文档使用所有 CLI `--input` 共用的 16 MiB 硬上限；文件 metadata 只用于快速拒绝，文件与 stdin 都最多读取上限加一个观察字节。随后才解析完整 JSON，并在任何 provider 调用前验证步骤数。
- 每个 step 可选声明 0–16 个 `bindings`。每项恰好选择一个来源形状：直接来源使用一基 `sourceStep` 和最多 256 个 UTF-8 字节的 RFC 6901 `sourcePointer` 读取严格更早步骤的成功 `result`；结构化 `template` 使用 1–32 个封闭 `literal`/`source` 段确定性产生一个 JSON string。两种来源都用封闭 `destination=target|args` 选择唯一可写区域，并恰好声明旧 `destinationField` 或新 `destinationPointer` 之一。旧目标继续只替换调用方已声明的 1–128 字节顶层字段；新目标必须是 1–256 字节的非根 RFC 6901 Pointer，全部父路径必须已在静态 step 中存在且为对象，最终叶字段可以替换或创建。它不递归创建父对象、不写数组或根对象。
- `template` 首批只允许静态 `status`、`sessions`、`inspect` step；`run` 在任何 provider 调用前以 `INVALID_ARGUMENT` 拒绝。每个模板至少含一个、最多含 16 个 `source` 段；每个 `literal.text` 最多 1024 个 UTF-8 字节，全部 literal 合计最多 4096 个 UTF-8 字节。source 仍只能引用严格更早、成功且结果未省略的步骤，且读取值必须是 JSON string；数字、布尔、null、对象和数组都不做隐式转换。渲染只按段顺序原样拼接，不解释表达式、转义、环境变量、格式化、条件、循环、函数、正则或脚本；最终字符串最多 4096 个 UTF-8 字节。
- 同一步骤按解码后的对象字段段规范化全部目标；同一区域中重复、转义别名以及祖先/后代重叠路径均在任何 provider 调用前拒绝，不能依赖绑定声明顺序覆盖。
- 单个直接来源值的紧凑 JSON 或单个模板最终字符串最多 4096 个 UTF-8 字节，因此每步绑定引入的拥有型值最多 64 KiB。来源二选一、模板段、引用方向、Pointer、字段边界、目标存在性与重复目标在任何 provider 调用前验证；来源结果、字符串类型和值大小只能在来源步骤完成后判定，但仍发生在当前 provider 启动前。
- 每个 step 可选声明 0–16 个 `postconditions`。每项必须是封闭对象，只支持 `operator=exists|equals` 和 RFC 6901 `pointer`；Pointer 最多 256 个 UTF-8 字节，`equals.expected` 的紧凑 JSON 最多 4096 个 UTF-8 字节。
- 每个 step 还可声明 0–16 个 `preconditions`。每项在相同 JSON Pointer 与 expected 边界内增加一基 `sourceStep`，且只能引用严格更早的步骤；第一步不能声明前置条件。引用方向、数量和条件负载同样在任何 provider 调用前验证。
- 后置 `exists` 只允许 `operator`、`pointer`，前置 `exists` 还必须提供 `sourceStep`；对应的 `equals` 都必须再提供 `expected`，其中 JSON `null` 是有效期望值。未知 operator、未知字段、数量超限、非法 Pointer 或负载超限均在任何 provider 调用前拒绝。

每个 step 复用统一 `CommandRequest` 的公开字段；`target` 与 `args` 可以包含 capability 自有的 provider-neutral 数据，但不得出现 HWND、PID、COM/WinRT/UIA 对象、第三方对象或任意执行脚本。

## 执行与停止语义

步骤按数组顺序同步聚合，但每一步的 provider 执行位于固定 Rust worker 中。Linux 主进程以原子 SIGINT/SIGTERM 事实驱动关联 cancel 帧，并在固定协作窗口或 deadline 后回收整个 worker process group；worker 同时绑定父进程死亡信号。正常完成不清理 provider 有意创建的持久对象，强制停止仍只按最后可靠帧报告事实。普通 provider 错误在 `continueOnError=false` 时停止，在 `true` 时继续；取消、总 deadline、步骤 deadline、未知终态、worker transport 失败、前置条件、输入绑定、后置断言和结果预算错误始终停止，不能被 `continueOnError` 放宽。

worker 必须先输出 `dispatch-accepted`，再输出唯一 `final`。在 accepted 前停止时，步骤返回 `outcome=not-dispatched`、`retrySafe=true` 和对应的 `CANCELLED`、`SEQUENCE_WORKFLOW_DEADLINE_EXCEEDED` 或 `SEQUENCE_STEP_DEADLINE_EXCEEDED`；accepted 后尚无可信 final 就停止时，步骤必须返回 `outcome=unknown` 与 `OUTCOME_UNKNOWN`，不得推测 provider 是否完成，也不得自动重试。两种情况都只记录当前步骤，不为后续未启动步骤伪造结果。

四种工作流终止错误码构成封闭集合，由 sequence Workflow 自有的强类型契约统一序列化为 `SEQUENCE_PRECONDITION_FAILED`、`SEQUENCE_BINDING_FAILED`、`SEQUENCE_POSTCONDITION_FAILED` 和 `SEQUENCE_RESULT_BUDGET_EXCEEDED`。各判定实现不得自行维护错误码字符串；本次类型收敛不改变既有文本、停止优先级或错误详情形状。

前置条件在当前 provider 启动前按声明顺序读取 `sourceStep` 已经完成、成功且未省略的完整 `result`。`exists`/`equals` 的值语义与后置断言一致；它不读取来源步骤 envelope、错误负载或当前调用方尚未执行的输入。全部通过时，当前步骤记录增加 `preconditions.checked` 与 `passed:true`。

首个前置条件失败时当前步骤不启动，也不向 `results` 插入伪 provider 步骤；历史结果保持原样，`count` 与 `failedCount` 只反映已经完成的 provider 步骤。顶层返回 `workflowError.code=SEQUENCE_PRECONDITION_FAILED`，其 `details` 包含被阻止的 `stepIndex`/`stepName`、`stepStarted=false`、`stepCompleted=false`、一基 `conditionIndex`/`sourceStep`、`operator`、`pointer`、条件判定进度和封闭 `reason=missing|not-equal|source-step-failed|source-result-unavailable`。该错误令顶层 `ok=false`、`workflowErrorCount=1` 并硬停止后续步骤，不受 `continueOnError` 放宽。

前置条件通过后，绑定按声明顺序从已经完成、成功且未省略的来源 `result` 复制完整 JSON 值，或把多个严格字符串来源与 literal 原样渲染为一个 JSON string，再写入当前请求中经过规范重叠检查的顶层或嵌套对象叶字段。全部修改只发生在尚未提交给 Policy 的内存候选请求上；任一绑定失败时整份候选请求被丢弃，不产生当前 provider 副作用。绑定只能取得最终 `CommandRequest.target` 或 `args` 的可变权；`verb`、`app`、`operation`、`confirmed`、`foregroundConsent`、`isolationRequirement`、`maxItems` 与 `maxDepth` 均来自当前 step 的静态声明，不能被绑定。全部应用后，最终请求仍完整进入统一 Policy/assessment、确认、精确目标、前台、隔离与权限门禁；模板和直接绑定都不授予确认、同意或权限。声明过且成功应用的步骤记录只增加 `bindings.applied`，不回显来源值、literal 或最终字符串。

首个绑定失败时当前步骤不启动，也不插入伪 provider 结果。顶层返回 `workflowError.code=SEQUENCE_BINDING_FAILED`，`details` 包含被阻止的步骤、一基 `bindingIndex`、可用时的一基 `sourceStep`/`sourcePointer`、模板失败时的一基 `segmentIndex`、`destination`、与输入形状一致的 `destinationField` 或 `destinationPointer`，以及封闭 `reason=source-step-failed|source-result-unavailable|source-pointer-missing|bound-value-too-large|template-source-type-mismatch|template-output-too-large|destination-field-missing|destination-path-unavailable`；值超限时还报告 `attemptedBytes` 与 `maximumBytes`，但从不回显 literal、来源值或最终字符串。该错误同样令顶层 `ok=false`、`workflowErrorCount=1` 并硬停止。

后置断言只针对其所属步骤已经成功、且经过 System 策略证明的完整结果执行。断言按声明顺序执行并在首个失败处停止；`exists` 把 JSON `null` 视为存在，`equals` 使用类型敏感的完整 JSON 值相等。失败步骤仍保持 `ok:true` 和完整 `result`，同时返回：

- `postconditions.checked`、`passed:false` 与一基 `failedCondition`；
- `workflowError.code=SEQUENCE_POSTCONDITION_FAILED`；
- `workflowError.details.stepCompleted=true`、`stepSucceeded=true`、一基 `conditionIndex`、`operator`、`pointer` 与封闭 `reason=missing|not-equal`；
- 顶层 `ok:false`、`workflowErrorCount:1`，且 `failedCount` 不增加。

通过的断言只返回 `postconditions.checked` 与 `passed:true`。断言证据和错误不回显实际值或期望值；完整 provider 结果仍受既有结果预算约束。

预算只接纳完整负载，不截断 JSON。若已完成步骤的下一份完整负载不能接纳：

- 成功步骤保持 `ok:true`，设置 `resultOmitted:true`；
- 失败步骤保持 `ok:false`，设置 `errorOmitted:true`；
- 两者都返回 `workflowError.code=SEQUENCE_RESULT_BUDGET_EXCEEDED`；
- `workflowError.details` 报告 `stepCompleted`、`stepSucceeded`、`attemptedBytes` 与 `remainingBytes`；
- 顶层 `ok:false`、`workflowErrorCount:1`、`budget.exhausted:true`，后续步骤不执行。

这一区分保证结果收集失败不会改写 provider 已成功或已失败的事实。`failedCount` 只统计真实 `ok:false` 的 provider 步骤；成功后结果省略不计为 provider 失败。

成功结果的后置断言在内存中先执行，随后才尝试接纳完整结果负载。若同一步既断言失败又无法纳入结果预算，对外 `workflowError` 由 `SEQUENCE_RESULT_BUDGET_EXCEEDED` 优先占用；步骤仍返回已完成的紧凑 `postconditions` 判定证据和 `resultOmitted:true`，但不返回断言错误或不完整结果。该优先级保证断言不被跳过，同时保证结果预算、单一终止错误和不截断 JSON 的契约稳定。

## 结果证据

顶层固定返回：

- `contractVersion=act/sequence-workflow/v1`；
- `count` 为已经执行的步骤数，`total` 为请求步骤数；
- `failedCount` 为真实 provider 失败数；
- `workflowErrorCount` 当前只能为 0 或 1，统计首个生命周期停止、前置条件、输入绑定、后置断言或结果预算终止错误；
- `budget.maxSteps`、`maxResultBytes`、`resultBytes` 与 `exhausted`；
- `results` 为已执行步骤的有序事实。

## 明确不保证

本版本不提供事务、通用回滚、补偿、幂等去重或自动重试。输入绑定只允许在静态对象父路径下创建或替换叶字段；结构化模板只对静态只读 step 开放，不提供自由格式模板字符串、表达式或脚本，也不能读取错误 envelope 或步骤元数据。保存、覆盖、删除、凭据或范围变化仍只使用既有最终请求 Policy，本版本没有新增动态值物化后的交互式重新确认协议。前置条件与绑定只读更早步骤的成功结果，后置断言只观察本步骤成功结果；它们都不能撤销已发生操作。取消和 deadline 只保证停止、回收和如实报告最后可靠观察；accepted 后缺失 final 时仍可能已经发生 provider 副作用，因此只能返回 `OUTCOME_UNKNOWN`。

Vikunja #2386 已在 `contracts/internal/sequence-template-materialization-v1.md` 冻结结构化
模板、稳定 execution/resume 和动态物化摘要重新确认的依赖；#2387 已按上述边界发布只读
`template`。#2388 继续负责持久 execution journal 与重复保护，#2389 必须等待它后才能开放
mutation 动态确认；在这些任务完成前仍不得接受 resume 或 materialization receipt 字段，
也不得把 `template` 用于 `run`。

Vikunja #2390 已在 `contracts/internal/sequence-execution-broker-v1.md` 冻结后续固定 broker、原子
journal、stable execution/step identity、单调 revision、resume/cancel/forget 与重复保护协议；实现
按 #2391–#2394 顺序推进。该内部契约不改变本 v1 当前同步入口，也不表示公开 resume 字段已发布。

Vikunja #2021 已在 `contracts/internal/sequence-execution-budget-v1.md` 冻结两级单调预算、停止优先级和 dispatch 前后事实边界；#2025 已把这些边界接入公开 schema 和生产 Workflow。

Vikunja #2022 进一步在 `contracts/internal/sequence-step-worker-v1.md` 冻结 request/cancel、dispatch-accepted 与 final 的严格双阶段协议和关联状态机；#2023 实现固定 worker 与 System 私有 dispatch hook，#2024 完成 Job runner 和竞争 fixture，#2025 已将公开 Workflow 路由到该执行链。步骤结果的 `execution` 固定公开 `outcome`、`completed`、`retrySafe`、`acceptedMayHaveOccurred`、`lastReliableObservation`、`stopReason` 与 `forcedReap`；accepted-only 必须映射为 `OUTCOME_UNKNOWN`，零帧停止必须保持未 dispatch，二者都无条件停止后续步骤。
