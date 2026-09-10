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
捕获指示器仍可能出现。Adapter 使用同一 ScreenCast 会话的授权 remote，ScreenCast v5 只在
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
本身不发送输入、不消费像素。Adapter 不请求 `persist_mode`，丢弃且不保存 restore token。

`input-key` 只接受同一 broker 仍持有的精确 live `s2:i`，并要求 `confirmed=true`、
`foregroundConsent=true`、`strictIsolation=false`。输入由
[`ui.input.key@3`](../v3/key-input.md) 冻结：只允许 provider-neutral 命名键、同请求配平状态和
有界 deadline，不接受文本或原生 keycode。每个 EIS 按下/释放分属独立 frame；成功仅证明事件
flush 到 EIS socket，`effectConfirmed=false`，禁止自动重试。

`input-pointer` 服从相同的精确会话、确认、前景同意与非 strict 门禁。输入由
[`ui.input.pointer@3`](../v3/pointer-input.md) 冻结，只允许 `relative-logical-px` 相对移动、
左/右/中键状态、单击/双击、横纵 discrete scroll 与有界拖拽；不接受绝对坐标、窗口命中、
Linux button code 或设备身份。成功只证明 EIS flush，固定
`effectConfirmed=false`、`finalPointerPositionConfirmed=false`，不声称应用消费或最终绝对位置。

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

普通收据容量为 1024 条。耗尽后，只为安全收尾额外保留独立配额：`input-cancel` 16 条、`close` 8 条（与最大活跃 lease 数一致）、`sessions` 8 条、`shutdown` 1 条；总收据最多 1057 条。各配额互不挤占，查询满载不能耗尽关闭/退出额度；输入类和 open/observe/capture 等普通新请求继续返回 `BROKER_REQUEST_LEDGER_FULL`，没有额外副作用。错误 close 同样保留收据并占用 close 配额，因此调用方仍须绑定先前发现的精确会话。

校验与旧 nonce 冲突检查先于配额分配；同 nonce/同语义重放不再次执行，也不消耗新配额。reader 的早期取消语义表使用相同有界配额，不能因收尾扩容遗忘旧请求或接受冲突取消。满载后的 sessions 是新 nonce 下的当前查询；重放旧 sessions 只得到旧收据，不能代替关闭后的核验。

这是有界降级与确定收尾，不是无限期运行、会话恢复或跨进程 exactly-once。取消/权限撤销/目标变化/结果未知仍需停止后续写操作；客户端应在自然批次边界预留预算，真实资源回收结论由 close 回执与空 sessions 共同建立。
