# Sequence 模板物化与动态确认内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 状态与所有权

该契约由 Vikunja #2386 冻结；Vikunja #2387 已把其中的只读结构化模板边界发布到
`act/sequence-workflow/v1`。当前调用方可在静态 `status`、`sessions`、`inspect` step 提交
`template`；`run` 模板、materialization receipt 或 resume 字段仍必须失败闭合。

`ComputerControlSystem` 只协调显式 Workflow。Sequence Binding Module 拥有来源形状、目标
路径和候选请求写入；Sequence Template Materialization Module 拥有模板结构验证、来源读取、
确定性渲染和后续物化摘要候选；Policy 继续拥有最终请求的
风险、确认、前台、隔离、权限和 assessment。后续 Workflow Execution Module 才能拥有
稳定 execution identity、step receipt、暂停、恢复、去重状态和 journal 生命周期。
模板不能授予确认，Materialization Module 不能保存 Policy 收据，Policy 不能重跑步骤。

该同步 Workflow 使用直接调用链，不使用 EventBus。平台类型、provider 私有身份、路径、
句柄、凭据和原始敏感值都不得进入公开模板、摘要或恢复身份。

## 结构化模板

后续模板绑定沿用每步最多 16 个 `bindings`、`destination=target|args`，以及 #2384 的
`destinationField` / `destinationPointer` 恰好二选一目标规则。一个 binding 再恰好采用：

1. 当前直接来源：顶层 `sourceStep` + `sourcePointer`；或
2. `template`：1–32 个封闭、有序段。

模板不是字符串表达式语言。每个段只能是以下形状之一：

```json
{"kind":"literal","text":"report-"}
{"kind":"source","sourceStep":1,"sourcePointer":"/document/name"}
```

- `literal` 只允许 `kind` 与 `text`；单段最多 1024 个 UTF-8 字节。
- `source` 只允许 `kind`、一基 `sourceStep` 和 RFC 6901 `sourcePointer`；Pointer 最多
  256 个 UTF-8 字节。
- 一个模板最多 16 个 source 段，且至少包含一个 source；静态 literal 合计最多 4096
  个 UTF-8 字节。
- source 只能引用严格更早、已经成功且结果未省略的步骤；读取值必须是 JSON string。
  数字、布尔、null、对象和数组不做隐式字符串转换。
- 渲染只按数组顺序拼接 literal 文本和 source string 的原始 Unicode scalar 序列；不做
  Unicode 规范化、转义解释、环境变量、格式化、条件、循环、函数、正则、脚本或求值。
- 最终字符串最多 4096 个 UTF-8 字节，并作为一个拥有型 JSON string 写入目标叶字段。
  不截断、不流式输出，也不保留来源结果借用。
- 目标仍按解码后的对象字段段规范化；重复、转义别名、祖先/后代重叠、根目标、数组、
  缺失或非对象父路径在首个 provider 前拒绝。

模板结构、全部来源方向和静态字节边界必须在首个 provider 前验证。运行时首个失败用
`SEQUENCE_BINDING_FAILED` 硬停止，定位 binding 与一基 `segmentIndex`，原因封闭为：

- `source-step-failed`；
- `source-result-unavailable`；
- `source-pointer-missing`；
- `template-source-type-mismatch`；
- `template-output-too-large`；
- `destination-field-missing` 或 `destination-path-unavailable`。

错误和成功证据只报告数量、索引、Pointer、目标路径、实际/最大字节数和类型类别，不回显
literal、来源值或最终字符串。直接绑定的既有错误形状保持向后兼容。

## 首批安全发布范围

Vikunja #2387 只能把结构化模板发布给静态只读 `status`、`sessions`、`inspect` step。
`run` 以及任何最终 assessment 为 mutation、destructive、需确认、需前台同意或目标范围
变化的请求，都必须在 dispatch 前返回尚未支持的结构化停止，不能接受现有 `confirmed=true`
或 `foregroundConsent=true` 作为运行时未知值的批准，也不能静默退化成直接绑定。

只读首批仍把完整物化请求交给统一 Policy/assessment。模板不能修改 `verb`、`app`、
`operation`、`confirmed`、`foregroundConsent`、`isolationRequirement`、`maxItems`、
`maxDepth`、deadline、nonce 或任何执行控制字段。

## 为什么动态 mutation 不能从头重跑

当前同步 sequence 没有稳定 execution identity 或持久 step receipt。假设步骤一已经创建
资源，步骤二物化出保存、覆盖、删除、凭据或范围变化请求并需要用户确认：

1. 若 Workflow 返回确认请求，进程内候选状态和步骤一结果随调用结束消失；
2. 调用方把 `confirmed=true` 写回原输入再调用，会从步骤一开始；
3. 步骤一可能再次创建资源，或者第一次响应丢失后真实结果已经是 `OUTCOME_UNKNOWN`；
4. 新步骤二的值、target、Policy revision 或风险可能与第一次物化不同；
5. 因而静态布尔确认既没有绑定第一次最终值，也不能证明前置副作用没有重复。

缓存 CLI 输出、把步骤注册顺序当作状态、自动重试、按名称猜测既有资源、EventBus 广播或
临时进程内 map 都不能建立跨调用恢复事实。没有稳定恢复身份时，动态 mutation 模板必须
保持不可用。

## 稳定 execution 与恢复前置

Vikunja #2388 必须先交付持久 Workflow Execution Module。最低契约包括：

- 随机 opaque execution identity、稳定 step key、不可变语义指纹和单调 revision；
- 有界容量、原子 journal、明确保留/过期和同用户访问授权；
- `created → running → awaiting-confirmation → dispatching → completed | failed |
  cancelled | outcome-unknown` 的封闭状态；名称可以调整，但语义不得合并；
- 每个已建立步骤的完整结果或明确省略事实、provider dispatch receipt 和最后可靠观察；
- `resume` 只从 journal 中的下一未建立阶段继续，不重跑 completed/failed/unknown 步骤；
- 同 execution/step/revision 的同义请求 Attach/Replay，异义复用结构化冲突；
- dispatch accepted 后缺少可信 final 时保持 `OUTCOME_UNKNOWN`，禁止自动重派；
- cancel request 与 Cancelled 终态分开，deadline 不冒充取消或 provider 失败；
- 崩溃恢复、关闭、过期和清理不能删除唯一恢复所有权。

普通 GUI provider 没有端到端幂等保证。稳定 step key 只能用于能验证语义指纹和收据的
provider；不支持幂等的 provider 一旦 accepted，就只能保存结果或 OutcomeUnknown，不能因
resume 再次执行。#2388 应再按协议、journal、client/launcher 和崩溃恢复拆分实现批次。

## 物化摘要与确认收据

Vikunja #2389 同时依赖 #2387 与 #2388。最终请求物化并经过无副作用 assessment 后，
需要确认的步骤必须原子进入 `awaiting-confirmation`，保存：

- execution identity、step key 与 revision；
- sequence/materialization 契约版本和 Policy revision；
- 最终 `verb`、`app`、`operation`、`target`、`args`、`maxItems`、`maxDepth`、
  `isolationRequirement` 的规范 JSON；
- assessment 的 risk、mutation、destructive、requiresConfirmation、
  requiresForegroundConsent、executionRealm、权限与目标类别；
- 来源 step/revision/Pointer 的非敏感 provenance；
- 以上事实计算出的 64 个小写十六进制字符 SHA-256 `materializationDigest`。

规范 JSON 采用 UTF-8、对象键按 Unicode code point 升序、数组保持顺序、字符串按 JSON
转义、数字使用解析后无歧义的最短 JSON 表示；不包含随机 nonce、trace、时间、确认布尔、
日志字段或可更新认证材料。digest 是关联与漂移检测，不是认证凭据。

公开确认摘要只显示 capability、目标类别、风险、动态字段路径、来源步骤和敏感分类；凭据、
路径、内容、token 和最终动态值默认不回显。确认收据必须由认可的交互主体产生，并绑定
execution/step/revision/digest、actor/session、Policy revision、授权范围和到期时间。

恢复前必须重新读取 journal 并比较全部绑定事实。digest、revision、Policy、actor、session、
授权、目标 freshness 或有效期任一漂移都返回新的确认请求或结构化拒绝；不得沿用旧收据。
收据验证成功只把当前步骤从 awaiting-confirmation 推进到 dispatching，不授予后续步骤权限。

## 依赖与明确非目标

- #2386：已冻结本契约和依赖。
- #2387：已实现只读结构化模板，不实现 mutation 确认。
- #2388：由 #2390–#2394 分批实现稳定 execution 协议、journal、broker、launcher、resume 与重复保护。
- #2389：在 #2387/#2388 完成后实现动态物化摘要重新确认。

本契约不承诺通用事务、自动重试、通用补偿、临时资源逆序清理、跨 System 消息、任意
表达式/脚本、秘密回显或 foreground fallback。补偿和临时资源清理必须在稳定 execution/
step receipt 之后另行设计，且不能把补偿成功表述为原操作从未发生。
