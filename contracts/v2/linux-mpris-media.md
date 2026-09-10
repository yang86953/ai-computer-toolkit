# Linux MPRIS 媒体候选 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

三项 `@2` 是协调发布、生产关闭的候选能力。它们只在显式注入地址的私有 broker
生命周期内验收；默认 CLI 不读取 `DBUS_SESSION_BUS_ADDRESS`，不连接真实播放器，也不
自动激活服务。`s2:m` 只是绑定 broker epoch、bus GUID、well-known name、unique owner
与 owner generation 的路由指纹，不是授权令牌；每次使用都重新解析，零/多命中分别为
`STALE_SESSION`/`AMBIGUOUS_TARGET`。
单次 state/control 使用时，目标重新枚举、属性或能力门禁、owner 复核与最终 method 共享
同一 D-Bus 连接和同一总 deadline，不嵌套第二个发现 timeout。
每次目录枚举都必须在 `ListNames` 前订阅 `NameOwnerChanged`，对全部候选完成双次 owner 一致性
核验，再以同一总线服务回复建立屏障并排空有界信号队列；窗口内任一 MPRIS owner 变化都使整个
快照 stale，不得发布跨 owner generation 拼接的目录。

生产候选的父代际解析不得读取 `DBUS_SESSION_BUS_ADDRESS` 或 `XDG_RUNTIME_DIR`。它只能从内核
有效 UID 构造 `/run/user/<uid>/bus`，验证同 UID、0700 真实运行时目录与同 UID 真实 Unix socket，
再以禁用自动激活的 `GetId` 读取 D-Bus GUID。运行时目录/socket inode 与 GUID 只作为内部父代际；
任何字段不得进入公共 JSON。父代际解析和后续媒体请求必须消耗同一原始总期限，不得各自重置预算。
feature-gated runtime client 已将解析器与固定主映像 observation/control self-worker 串联；私有总线
链路通过。父进程必须把规范化 GUID 作为内部 `expectedBusGuid` 交给 worker；worker 在
`ListNames`、属性读取、accepted 和 method 前以自身 `GetId` 复核，错配失败闭合。该候选尚未接入
App、公开 registry 或默认 feature。

crate-private MPRIS candidate Policy 现固定把 `Sessions`、`Inspect`、`Run` 分别映射到三项 v2
capability。控制确认必须早于 operation、target、args、数量和 provider 解析；读写目标只接受
canonical `s2:m`，额外字段和前台同意均拒绝。通过后的成功结果可由 Policy 附加
`executionRealm=requiredExecutionRealm=isolated-worker`、认证事实、隔离要求和主机影响策略；三份
v2 schema 允许这些可选字段，以继续兼容 worker 原始结果。该纯 Policy 尚未被 candidate Adapter、
Service、CLI 或公开 registry 调用，不能据此报告生产可用。
crate-private candidate Adapter 现把 runtime `timeoutMs` 显式冻结为 worker v1 的 30000 ms 上限；
Policy 计划把同一值交给 resolver 与 worker，Adapter 不得重置预算。Adapter 强制在 runtime 前授权、
在成功结果返回前 attestation，但仍未加入 Adaptive registry、Service 或 CLI。

发现与状态读取现由 `linux-mpris-candidate` feature 下的一次性只读 observation worker
承载，内部协议为 `act/internal/linux-mpris-observation/v1`。worker 每次只接收一个不超过
64 KiB 的严格请求并发布一个不超过 2 MiB 的终态 JSON。launcher 从同一主 CLI 映像启动固定
隐藏 self-worker，清空继承环境、固定根工作目录、独立 process group 并安装父进程死亡信号；不
搜索 PATH/sibling，并对取消、35 秒外层 deadline、输出超限和异常执行整组 kill+wait。该
worker 不进入默认 feature、Linux archive、公开 registry 或生产 App 路由。

发现只调用 `ListNames` 与 owner 解析，不读取 Player 属性。状态读取使用无缓存 proxy，
只逐项 `Get` PlaybackStatus 与六个 Can* 白名单；禁止 GetAll、Metadata、Identity、
DesktopEntry、PropertiesChanged。总线名称先受 4096 项硬上限约束，公开目录再按请求的
1..128 项上限截断并保留 `total/truncated/complete`；若 state 的有界重枚举已截断且目标
不在结果内，必须返回 provider unavailable，不能伪报 stale。输出不得含 bus name、owner、
PID、路径或传输身份。

控制由同一 feature 下的独立 `act/internal/linux-mpris-control/v1` worker 承载，不把写入
混入 observation worker。`confirmed=false` 在任何 provider I/O 前结束；确认后的目标、能力与
owner 门禁全部通过后，worker 必须在调用 MPRIS method 前先 flush 与 target/operation 精确关联的
`accepted` JSONL frame，之后只允许一个 final。正常回复仅表示
`dispatchOutcome=replied`，永远 `effectConfirmed=false`；worker 内 deadline 以及父 launcher
在 accepted 后遇到超时、取消、进程异常、输出或 final 漂移时都固定为 `OUTCOME_UNKNOWN`、
`retrySafe=false`，并禁止自动重试。accepted 前的拒绝保留零 mutation 与可重试事实。

当前公开三项 capability 继续保持
`execution:none`，不得把私有 fixture 证据解释为生产媒体能力完成。
