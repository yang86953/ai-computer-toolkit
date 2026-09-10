# 独立交互会话隔离契约

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 状态与定义

本文冻结 `GC-ISO-001` 的 provider-neutral 技术边界。当前 Rust 生产候选已经实现固定
session broker、认证 endpoint、一次性 Job worker 与 System/Policy 路由；`ISO-D` 的无
endpoint 生产 launcher 和合成异常矩阵已完成，技术范围还必须补齐真实授权 endpoint 的
跨系统 session 执行证据。用户专属的真实视觉与交互验收仍由 #2008 独立拥有。

“独立交互会话”必须是 Windows 认为可接收窗口激活与 `SendInput` 的活动交互会话，且
其系统 session 身份与调用方当前会话不同。隐藏窗口、同一会话中的备用 window station、
非输入 desktop、同进程线程、普通 Job companion 和 headless worker 都不满足此定义。

工具只使用用户或部署方预先显式授权的独立会话，不创建、接收或保存登录凭据，也不
绕过锁屏、UAC、安全桌面、Session 0、完整性级别或系统并发会话限制。没有合格会话时
返回 `ISOLATED_WORKER_UNAVAILABLE`，不得把请求重放到当前桌面。

## 公开身份与路由

- 独立会话以 canonical `s2:i:<16 lowercase hex>` 标识；指纹由 worker 私有的系统 session
  代际与一次授权代际生成，公开结果不得包含 session number、用户名、SID、token、
  window station、desktop、pipe、端口、路径或 native handle。
- 调用方先从通用隔离会话发现入口取得 `interactiveSessionId`，再在统一请求中同时提供
  该值与 worker 会话内发现的原 `sessionId`。目标窗口、进程和元素继续使用既有 opaque
  类型，不把 `s2:i` 当作可写窗口。
- `ComputerControlSystem` 只按精确 `interactiveSessionId` 选择认证端点并冻结
  `isolated-worker` 执行计划；本地 Input、Window、Accessibility 或应用 provider 不得在
  选择后再次解析该请求，也不得在远端失败时接管。
- worker 必须在自己的实时 inventory 中重新解析内部 `sessionId`。零命中返回
  `STALE_SESSION`，多命中返回 `AMBIGUOUS_TARGET`，不得使用调用方会话中的同名目标。
- Rust CLI 的公开发现入口为 `discover isolation`。用户或部署方必须先在目标登录会话内
  显式启动同安装目录的 `ai-computer-toolkit-interactive-session-broker.exe`；host 不创建、
  登录或解锁会话。broker 未运行或认证失败时发现集合不包含该 endpoint，精确执行则返回
  结构化 stale、认证失败或不可用结果。

## SMC 所有权

- Interactive Isolation Module 拥有授权会话清单、精确匹配、endpoint lease、请求 deadline、
  取消、断线和结果证明；它只向 System 暴露“发现端点”和“执行一个统一请求”两个窄端口。
- Session Broker Component 只在已授权的独立会话内监听固定、版本化、带 OS 访问控制的
  本机协议；它不接受调用方指定 pipe、端口、路径、argv、shell、凭据或任意子进程。
- 每个请求由 broker 启动固定 Rust command worker。该子进程在首条请求前进入
  `KILL_ON_JOB_CLOSE` 等价的生命周期容器；stdin/stdout、输出上限、deadline、取消和整树
  回收沿用统一 worker 约束。broker 本身只拥有 endpoint lease，不跨请求持有按键、按钮、
  target 或 mutation 状态。
- command worker 在自己的会话中装配既有 `ComputerControlSystem`、Input Module 与 Window
  Module。领域语义不在 broker、host 转发器或平台 Adapter 中复制。
- Windows Adapter 只拥有 session、进程、Job、IPC 和当前桌面事实；公共接口不得出现
  Win32、WTS、token、pipe security descriptor 或其他平台类型。

内部 broker wire 契约由 `contracts/internal/interactive-session-broker-v1.md` 与
`contracts/internal/interactive-session-broker-v1.schema.json` 冻结。构建标识只绑定包名、
包版本和协议角色，不冒充代码签名；认证还必须同时成立固定 sibling 完整路径、内核 peer
进程、Windows session、同一用户主体、活动 Default 输入桌面和本连接随机 lease。

## 认证与生命周期

一个 endpoint 只有同时满足以下条件才可标记为 certified：

1. 协议版本、固定 worker 构建身份与部署清单一致；
2. OS 对等身份证明 endpoint 属于本机已授权会话，且该会话当前 active、interactive、
   unlocked、位于普通 input desktop；
3. endpoint 的系统 session 身份与 host 当前会话不同；
4. broker 能为本请求创建受 Job 约束的固定 command worker，并在任何目标解析前完成握手；
5. host 与 worker 都冻结同一 `isolationRequirement`、capability、确认、前景许可、deadline
   和 opaque 目标；worker 不接受递归 `interactiveSessionId`；
6. host 断线、取消、timeout、worker crash 或 broker crash 后，不再存在本请求拥有的子进程、
   pipe、按键或按钮状态；后续请求必须通过新握手和新 worker 恢复。

host 在连接 endpoint 前冻结唯一总 deadline；连接、peer 认证、lease 握手和 worker 执行
共同消费该预算，任何阶段都不得重新获得完整 timeout。broker 的空闲 accept、首帧、command
帧及最终响应交付均以短轮询观察取消和单调 deadline。最终响应写入预留的单帧有界缓冲后，
broker 最多等待固定握手窗口让 host 读取并关闭；不得调用不可取消的 named-pipe flush。

endpoint 不满足任一条件时必须失败闭合。认证事实只在一次请求内有效，不能从旧成功结果
推断后续可用性。

## 执行与结果语义

独立交互路线复用既有 provider-neutral 输入和窗口能力，至少覆盖：

- `ui.input.key@1` 的命名键、组合键、按下/释放和 Unicode 文本；
- `ui.input.pointer@1` 的移动、按钮、单击/双击、滚轮和拖拽；
- `window.lifecycle@1` 的 restore/minimize/maximize/move/resize；
- 独立确认的 `window.close@1`。

确认和风险等级不因隔离而降低。worker 内的前景同意只授权改变独立会话前景，不能授权
改变 host 当前桌面。dispatch 前失败可以返回确定错误；一旦 worker 可能接受 mutation，
timeout、取消、断线、进程异常或协议异常统一保留 `OUTCOME_UNKNOWN`、
`retrySafe=false`、`targetMayHaveMutated=true` 以及 best-effort 输入释放证据。

成功结果必须由 host System 投影并证明：

- `executionRealm` 与 `requiredExecutionRealm` 均为 `isolated-worker`；
- `executionRealmCertified=true`、`isolationRequirement=strict`、
  `hostImpactPolicy=strict-no-interference`；
- `isolationKind=independent-interactive-session`，并返回原 `interactiveSessionId`；
- worker session 与 host session 不同，host 前景、host 光标和 host 输入状态在调用前后未变；
- 本地 provider 未解析目标或执行 mutation，且请求资源已经回收。

任何证明缺失或前后不一致都返回 `HOST_INTERFERENCE_DETECTED` 或
`ISOLATED_WORKER_UNAVAILABLE`，不能接受 provider 自报成功。

## 验证边界

Rust 自动化必须覆盖协议/schema、opaque 会话 stale/ambiguous、认证矩阵、本地 provider
零调用、固定 worker/Job、成功、dispatch 前失败、accepted 后 timeout/取消、worker 与
broker crash、host 断开和后续恢复。无独立会话的生产环境必须通过真实 launcher 验证
结构化不可用且零前台回退。

工具自有 fixture 可以验证输入与窗口结果、当前桌面快照不变及资源回收，但不能代替
`yang86` 在真实独立交互会话中的视觉/交互判断。AI 只能把 #2008 移交为验收候选，不得
批准或关闭该任务。

当前生产候选已具备 `ISO-B` 的固定 broker/endpoint/lease/Job 边界和 `ISO-C` 的发现、Policy、
System 与四条通用生产路由。`ISO-D` 的全目标回归、无第二授权会话 launcher 和合成故障
矩阵已完成；真实授权 endpoint 的跨系统 session 技术执行证明仍待环境具备后补齐。此前的
同会话拒绝、纯协议测试或合成异常矩阵不能冒充该项机器证据，更不能冒充 #2008 验收。
