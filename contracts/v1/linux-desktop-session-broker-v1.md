# Linux Portal Desktop Session Broker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`act/linux-desktop-session-broker/v1` 是 `desktop.session.open@1`、
`desktop.session.close@1`、`screen.capture@1`、`ui.input.key@3` 与 `ui.input.pointer@3` 的候选生产边界。真实生产 launcher 验收通过前，capability
状态保持 `live-acceptance-pending` 且不发布 available。验收调用方运行
`ai-computer-toolkit session-host desktop`，保持该进程及其 stdin/stdout 存活，逐行交换
UTF-8 JSON。broker 先发送唯一 `broker-ready`；后续每个请求必须绑定该
`brokerEpoch` 与 128 位小写十六进制 `requestNonce`。

短调用场景可使用同一候选 broker 的 `session-host desktop --socket` 模式，再以
`session-call desktop --input <file|->` 提交单个完整 JSON 请求。调用方的稳定边界仍为
版本化 JSON over stdio；一次性 CLI 在收到完整、同 epoch/nonce/operation 的结果后立即退出，
不用轮询常驻进程输出。内部 socket 只位于当前内核 UID 的私有运行目录，双向校验对端 UID，
不接受调用方指定 socket 路径、不监听网络，也不自动创建 Portal 会话。ready 的 `transport`
为 `json-lines-unix-socket`，会话、策略、nonce 台账与输入 owner 均复用原实现。

socket 模式每连接只接受一帧及写半边 EOF，最多 32 个接入 reader、16 个排队请求，接入 I/O
超时为 2 秒。reader 在同一语义锁内依次预检与入队，只能置位取消；Module 和 EIS 仍只在
owner 线程执行。错误连接只得到当前连接的错误，不能终止其他调用；单个客户端断开也不等同
撤销已接受的请求。`input-cancel` 可通过另一条连接在原输入执行期间请求取消。响应最多 1 MiB，
读取总期限通常为 35 秒，`open` 为请求的授权等待期限加 5 秒且最多 305 秒；它包含排队等待，
不延长领域操作本身的 deadline。

单次 CLI 的退出码为完成 `0`、broker 业务失败 `1`、客户端解析或传输失败 `2`。已解析请求的
传输失败返回绑定原请求的 `client-error`：开始写入前为 `outcome=failed`、
`acceptedMayHaveOccurred=false`；尝试写入后失联、超时、坏响应或身份错配一律为
`outcome=unknown`、`acceptedMayHaveOccurred=true`，禁止自动重发。未能解析请求时只返回
`broker-error`，不得伪造请求身份。此入口没有自动重试、自动重开或自动接管焦点功能。

真实运行结果需由项目负责人按使用说明进行人工确认。
手册中的 Rust 契约测试只防止步骤漂移，不能代替用户看见并批准 Portal 弹窗。

broker、Desktop Session Module、Portal Adapter 与全部 live lease 位于同一线程。此边界
是必要约束：`reis` 的 EIS event stream 不可跨线程移动。公开层只看见 `s2:i`、授权设备
类别、流数量、mapping 数量与 RemoteDesktop/ScreenCast 版本；D-Bus object path、request
handle、PipeWire node、mapping identity、EIS/PipeWire FD 均不得输出。

`capture-frame` 只接受当前 broker 仍持有的精确 `s2:i`，要求 `confirmed=true` 并拒绝
`strictIsolation=true`。它不重新显示 Portal 选择器、不激活窗口，也不需要前景影响同意；系统
捕获指示器仍可能出现。Adapter 使用同一 ScreenCast 会话的授权 remote：首次快照用一个 remote
建立会话内复用的常驻 PipeWire 连接，之后的每次快照都在该连接上新建一条 stream 并等待首帧，
一次性 frameId/映射核对与坐标快照语义不变；不再每帧重新 `OpenPipeWireRemote`（实机证据：
同会话重复请求会在回复宽限内得不到应答，`code=TIMEOUT`/`stage=open-pipewire-remote`；Portal
侧原因的旧栈未取得，属候选解释）。连接通道与订阅 worker 各自独占一条 remote，互不复制或
共享已连接 socket；通道建立前的重开请求一律受回复宽限约束。ScreenCast v5 只在
私有边界使用 node ID，v6 起必须优先 `pipewire-serial` 与 `PW_KEY_TARGET_OBJECT`。两者都不得进入
Module、JSON、日志或文件名。每次请求只协商 packed RGBA/BGRA/RGBx/BGRx/xRGB/xBGR，拒绝
DMA-BUF、modifier、YUV、多平面与损坏 chunk；raw frame、stride、宽高、像素数和 PNG 均有硬
上限。首帧逐行归一化后复用 Atomic File Component 提交单个 `.png`，响应只返回路径、字节、
宽高、digest 与覆盖事实，不返回像素或 Base64。超时或像素是否已被消费不明时禁止自动重试；
单次 PipeWire 失败不冒充 Portal lease 已失效，Portal/session/logind liveness 失败则使 `s2:i`
stale。首批只有总 deadline，不接受 `input-cancel` 或其他异步取消；补齐通用 Workflow 取消前
不能宣称达到 L5 停止线。此能力不等同连续录制。

`capture-frame.input.maxDimension` 是显式图像预算扩展：整数 256..16384，限制最长边、不放大。
缺省保持原图与既有结果形状；指定时在完整验证的源 mapping 中直接进行像素中心最近邻采样，
只分配输出尺寸 RGBA。输出 `width/height/pixelDigest` 描述实际 PNG，并额外返回
`sourceWidth/sourceHeight/sampling`；`sampling` 为 `identity` 或 `nearest-preview`。
会话 `pixelsConsumed` 仍计完整源帧，预览摘要不能证明源帧未变化，预览也不提供桌面输入坐标。
原图是像素无损，预览是明确的细节取舍；文字或细节不清时由调用方另发原图观察请求，不能自动
把旧帧或预览冒充完整验证证据。不改变单帧消费、授权、取消停止线和 capability 候选验收状态。

stdin 由独立只读线程解析 64 KiB 有界帧；普通请求进入最多 16 帧的有界队列，Module、EIS 与
stdout 仍只归原 owner 线程。reader 只能在严格 epoch/nonce/语义校验后置位请求局部取消令牌，
不能调用 Portal/EIS 或写响应。队列满、坏帧或非 UTF-8 会请求当前输入安全停止并由 owner 输出
唯一 terminal error；stdin EOF 不等同取消，已接受请求仍按原终态收敛。

`open` 必须同时携带 `confirmed=true` 与 `foregroundConsent=true`，并拒绝
`strictIsolation=true`。它执行 `CreateSession`、键盘/指针 `SelectDevices`、同会话
`SelectSources`、`Start`、EIS sender 握手、键盘及相对指针 seat/device 绑定与 `OpenPipeWireRemote`；打开操作
本身不发送输入、不消费像素。可选 `authorizationScope=session` 把 open 处的一次确认
扩展为整条会话的授权作用域（缺省 `operation` 保持逐操作显式确认）；可选
`rememberAuthorization=true` 只在 Linux Portal 后端受支持，其他后端在任何 Portal
派发前以 `DESKTOP_AUTHORIZATION_PERSISTENCE_UNSUPPORTED` 失败闭合。

`input-key` 只接受同一 broker 仍持有的精确 live `s2:i`。缺省作用域下要求
`confirmed=true`、`foregroundConsent=true`、`strictIsolation=false`；`session`
作用域的会话可以省略这三个字段并继承 open 处已授予的作用域，但任何显式拒绝
（`confirmed=false`、`strictIsolation=true` 等）仍按原错误码闭合，不能被继承覆盖。
输入由
[`ui.input.key@3`](../v3/key-input.md) 冻结：只允许 provider-neutral 命名键、同请求配平状态和
有界 deadline，不接受文本或原生 keycode。每个 EIS 按下/释放分属独立 frame；成功仅证明事件
flush 到 EIS socket，`effectConfirmed=false`，禁止自动重试。

`input-pointer` 服从相同的精确会话、确认（含省略继承）与非 strict 门禁。输入由
[`ui.input.pointer@3`](../v3/pointer-input.md) 冻结，只允许 `relative-logical-px` 相对移动、
左/右/中键状态、单击/双击、横纵 discrete scroll 与有界拖拽；不接受绝对坐标、窗口命中、
Linux button code 或设备身份。成功只证明 EIS flush，固定
`effectConfirmed=false`、`finalPointerPositionConfirmed=false`，不声称应用消费或最终绝对位置。

`observe`、`capture-frame`、`observe-subscribe` 与 `observe-next` 适用同样的省略继承规则：
缺省作用域必须显式 `confirmed=true` 且非 strict；session 会话可省略并继承。省略字段而会话
不存在时返回 `STALE_SESSION`，不凭省略字段放行任何输入或像素。

## 一次授权、记住授权与撤销

`authorizationScope=session` 表示调用方显式选择「一次确认覆盖整条会话」：open 仍是唯一
确认点，后续同一 live 会话的全部受支持桌面操作（观察、键鼠、`interact`、订阅与长流程批次）
不必重复确认字段。继承只发生在字段被省略时；显式传入的拒绝字段优先闭合。CLI socket 模式、
stdio 模式与 MCP `computer_connect` 使用同一授权事实，不靠外部包装代填。

`rememberAuthorization=true` 在 Linux 上按 Portal 原生机制记住授权：

- Adapter 对 RemoteDesktop `SelectDevices` 传 `persist_mode=2`（直到显式撤销）；已有已保存
  token 时同时传 `restore_token` 自动尝试恢复。组合 RemoteDesktop+ScreenCast 会话的持久化
  只走 RemoteDesktop，`ScreenCast.SelectSources` 不携带持久化选项。
- restore token 单次有效；`Start` 成功返回下一枚，Adapter 立即在同一跨进程互斥内轮换保存。
  保存位于当前用户私有状态目录（`XDG_STATE_HOME` 或 `~/.local/state` 下的
  `ai-computer-toolkit/`，0700 目录、0600 文件、同目录 staging 原子替换、O_NOFOLLOW 与
  属主/组权限校验）。同 token 的并发连接由 flock 串行，锁覆盖恢复、授权与轮换全过程，
  期限为 open 的剩余 deadline；超时返回 `DESKTOP_AUTHORIZATION_BUSY`（retrySafe），不重复
  消费同一枚 token。
- token 绝不进入 JSON、MCP 结果、日志、任务、源码或文件名。公开事实只有脱敏布尔：
  会话视图的 `authorization.persistence.requested/restoreAttempted/restoreTokenRetained`
  与顶层 `restoreTokenRetained`；保存失败或 Portal 未授出持久化时如实报告
  `restoreTokenRetained=false`（附 `note`），不宣称已记住。轮换失败会作废可能已被消费的
  旧 token，下一次连接重新走正常授权。
- `restoreAttempted` 只表示本次 open 向 Portal 提交了已保存 token（恢复尝试）。Portal 在
  无法恢复时按官方语义忽略该 token 并正常弹窗，客户端无法观测免提示恢复是否真实发生；
  本工具不把 token 存在、恢复尝试或连接耗时当作免提示恢复成功的证据。成功打开并获得
  下一枚凭据由 `restoreTokenRetained=true` 表达，且仅表达这一点。
- Portal 无法恢复已存授权时按官方语义回退为正常选择弹窗；本工具不自动点击系统同意，
  失败或取消也不自动重复弹窗或重放输入。接口支持不等于具体后端已实现免提示恢复，
  实机行为以桌面环境为准。
- 经 `call_with_flags` 等待回复的 Portal 方法（`ConnectToEIS`、`OpenPipeWireRemote`）都
  有硬期限兜底（打开序列用剩余 deadline，会话内重开用固定回复宽限）：zbus 该路径不应用
  连接级 `method_timeout`，Portal 后端卡住不回包时不得挂死唯一 owner 线程。

`authorization-status` 返回脱敏状态：`savedAuthorizationState` ∈ `saved`/`absent`/
`unreadable`（存储未通过私有性校验时拒绝使用）与 `savedAuthorizationBackend`；不返回任何
凭据内容。`forget-authorization` 清除本工具保存的凭据并停止本客户端 broker 内全部 live
会话，回执 `liveSessionsClosed/liveSessionsFailedToClose` 与
`revokesSystemPortalRecords=false`：本地忘记只撤销本工具保存的内容，系统 Portal 自身的
授权记录需在桌面环境权限管理中单独撤销。两个操作与 `sessions` 一样只绑定当前 epoch 的
身份字段，不携带 session 或原生身份。

`input-cancel` 只携带自己的 `requestNonce` 与目标 `targetRequestNonce`，不携带 session、输入、
确认或原生身份。cancel-before-target 安装当前 epoch 内 tombstone，后到输入在 provider 前返回
`CANCELLED`；在途 hold/drag 最迟每 10ms 观察令牌，随后逆序释放、stop emulation、关闭 lease 并
使 `s2:i` stale。回执状态封闭为 `unknown-request`、`cancellation-requested`、`cancelled` 或
`too-late`；相同 cancel nonce/语义精确重放，异义返回冲突。取消不能撤回已经 flush 的事件，
原输入会公开已完成步骤和事件数，`outcome=cancelled`，且仍禁止自动重试。

lease 建立前，Linux 私有 Adapter 通过公开 systemd-logind D-Bus 接口调用
`GetSession("auto")`：由 logind 按调用方凭据选择其所属会话；调用方位于 `user@.service`
等无所属登录会话的进程范围时，选择同一用户的主显示会话。只接受 `Type=wayland`、`Class=user`、`Active=true`、`CanLock=true` 且
`LockedHint=false` 的会话。它预订阅 login1 owner、`SessionRemoved` 与当前 Session
`PropertiesChanged`；服务换代、会话移除、必需属性失效、类型错误、总线/监视流异常、inactive
或 locked 均失败闭合。`LockedHint=false` 只代表桌面环境当前公开的锁定提示，不是绝对未锁屏
证明；缺失或不可读取时拒绝输入。返回的具体 session 对象在 lease 生命周期内固定；
不监视相对的 `auto` 对象，不重新选择会话，不枚举或选择其他用户的显示。D-Bus path、session ID 与 PID 只存在于 Adapter，不进入公开
schema、日志或结果。

每次输入先按“取消 → deadline → Portal/宿主活动 → EIS”固定顺序检查；每个 EIS effect、事件
drain、最长 10ms 的 hold/drag 等待片段以及最终 stop flush 后再次检查。Portal `Closed` 与明确
owner 变化会粘性保存清理证据；监视流异常只使输入失败，不能伪造 cleanup confirmation。标准
Wayland 不存在标准的跨应用全局焦点 owner、所属进程或退出通知，因此当前输入只承诺“发送到
当前物理焦点”，不绑定焦点进程；焦点在 checkpoint 间变化仍是已声明的前台风险，禁止借助
X11/XWayland、compositor 私有协议或 `/proc` 身份推断补洞。

输入期间的 EIS pause/remove/disconnect、deadline 或 flush 失败会使会话 identity stale，并先
best-effort 逆序释放工具持有键或按钮、停止 emulation、关闭 Portal session；错误公开
`acceptedMayHaveOccurred`、`releasesConfirmed`、`sessionCleanupConfirmed` 与 `outcome`，不得隐式
恢复句柄或转用其他输入路线。

绝对输入的新鲜度失败按脱敏 `stage` 分类，分类由读取全部事实后的纯 helper 按固定优先级
完成：`absolute-input-device-unregistered`（该检查点设备不在可用列表）优先于
`absolute-input-device-dead`（设备对象已失效），两者都命中时代际分支不再被评估，最后才是
`absolute-input-generation`（设备在列表且存活，仅代际与观察时不一致）。EIS 的
DevicePaused/Removed/SeatRemoved 事件会同时改变代际并移出列表，即多个失败条件可共存：
stage 只表示「按优先级最先命中的失败事实」，不是互斥的事件原因；列表缺失也不能区分
pause、remove 还是 seat 撤销，区分它们需要真实事件轨迹，本工具不携带也不推断。
frame-point 的映射比较失败分解为 `frame-point-mapping-dimensions`（维度与观察时不一致）
与 `frame-point-mapping-generation`（仅代际不一致），两者可同时不同，维度优先报出。
模块侧另有 `observation-mapping`（观察后区域映射已变）/`observation-missing`（无本会话
最新观察）。代际只在 apply_event 随服务端设备事件变化，工具自身输入不产生代际变化；
键盘引起的内容变化不使坐标观察失效，几何/授权变化才失效。stage 均不携带原生设备身份。

对历史失败响应的回溯推断以代码路径为限：`outcome`/`accepted` 区分的是失败发生在
emulation 会话的哪个阶段（外层派发前，或 start_emulating 之后的检查点），不能还原该
检查点命中的具体条件；`before_dispatch` 也不蕴含设备健康。2026-09-11 本机（KDE）三次
同会话 `STALE_OBSERVATION` 失效与上述静态候选路径一致——观察与派发之间 EIS 服务端
发生过设备状态事件是代码可达性的必要解释，但每次失败的具体条件、检查点与服务端触发
原因（KDE/KWin EIS 实现的设备重配置或其他）均未取得实际事件轨迹，属未知/候选，
不凭单次现象定因，也不把任何桌面环境当已证对象。

`sessions` 与 `inspect` 只查询当前 broker owner generation；`close` 先使 opaque 句柄 stale，
再消费唯一 lease。主动 `Session.Close` 的成功方法回复，或异步 `Closed`/Portal owner 变化，
均为清理确认；无法确认时返回 `outcome=unknown`、`retrySafe=false`，不得恢复句柄或自动重试。
相同 nonce 与相同语义只重放原响应，不再次发起 Portal 请求；相同 nonce 的不同语义失败闭合。

`shutdown` 在响应写出后退出，Module Drop 回收尚存 lease；stdio 模式的 stdin EOF 或输出断开也走同一回收。
它不声称每条 Drop 清理均有可观察确认，因此需要确定关闭结果的调用方必须先逐条执行
`close`。单帧最大 64 KiB；stdio 模式的未知字段、非 UTF-8、超限、不合法 JSON 或有界队列溢出会终止
broker，socket 模式则拒绝该连接。正常 socket shutdown 只清理由自身创建且未被替换的 endpoint；
异常终止遗留的 endpoint 不会被新进程接管，旧 epoch 不能恢复会话。终止线程不得接管 EIS owner。

## 请求账本与满载收尾

普通收据容量为 1024 条。耗尽后，只为安全收尾额外保留独立配额：`input-cancel` 16 条、`close` 8 条（与最大活跃 lease 数一致）、`sessions` 与 `authorization-status` 合计 8 条、`forget-authorization` 8 条、`shutdown` 1 条；总收据最多 1073 条。各配额互不挤占，查询满载不能耗尽关闭/退出额度；输入类和 open/observe/capture 等普通新请求继续返回 `BROKER_REQUEST_LEDGER_FULL`，没有额外副作用。错误 close 同样保留收据并占用 close 配额，因此调用方仍须绑定先前发现的精确会话。

校验与旧 nonce 冲突检查先于配额分配；同 nonce/同语义重放不再次执行，也不消耗新配额。reader 的早期取消语义表使用相同有界配额，不能因收尾扩容遗忘旧请求或接受冲突取消。满载后的 sessions 是新 nonce 下的当前查询；重放旧 sessions 只得到旧收据，不能代替关闭后的核验。

这是有界降级与确定收尾，不是无限期运行、会话恢复或跨进程 exactly-once。取消/权限撤销/目标变化/结果未知仍需停止后续写操作；客户端应在自然批次边界预留预算，真实资源回收结论由 close 回执与空 sessions 共同建立。
