# UIX Agent read-only provider v3

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本契约为显式启用 Agent 控制的 UIX 应用增加七项 provider-neutral 只读能力：

- `window.discover@3`
- `window.metadata.read@3`
- `accessibility.tree.read@3`
- `ui.element.locate@2`
- `window.revision.wait@1`
- `window.closed.wait@2`
- `window.state.read@1`

同一认证 Adapter 还承接独立的确认式 mutation `ui.element.action@2`、应用内按键
`ui.input.key@2`、应用内指针 `ui.input.pointer@2` 与需前景影响授权的 `window.lifecycle@2`；
以及只提交平台关闭请求的 `window.close@2`。五者分别由 `contracts/v2/ui-element-action.md`、
`contracts/v2/key-input.md`、`contracts/v2/pointer-input.md`、`contracts/v2/window-lifecycle.md`
和 `contracts/v2/window-close.md` 冻结。本文七项只读调用本身
仍不执行动作。

它们不修改冻结的 Windows `@1` 或 Linux AT-SPI `@2`，也不表示工具拥有 compositor
全局窗口目录。覆盖范围固定为 `opt-in-uix-agent-applications`；只有同时编译 UIX
`agent-control` feature 并调用 `.enable_agent_control()` 的应用才会发布端点。

## SMC 边界

- `ComputerControlSystem` 把 `app`、`window` 与 `accessibility` surface 路由到生产 Adapter；
- UIX Window Module 拥有能力语义、限制、公共 JSON、中立树 BFS 和错误投影；
- UIX Agent Adapter 独占发现文件、Unix socket、认证 token、PID、UIX `window_id`、
  `generation`、原始 `node_id` 与 JSON Lines 传输；
- opaque target Component 把私有端点代际映射为 `s2:w`、`s2:e` 和 `as3`，公共边界不出现
  PID、路径、token、socket 或 UIX 原生 ID。

工具包不依赖完整 `uix` crate。客户端只实现 UIX 已冻结的 `uix.agent.v1` 本机协议，避免把
渲染器、窗口后端或业务 UI 生命周期带进控制核心。

## Linux 发现与认证

生产发现目录与 UIX server 保持一致：优先使用非空绝对 `XDG_RUNTIME_DIR`，否则使用系统
临时目录，再拼接 `uix-agent-<effective-uid>`。客户端只读且不得修改目录权限。

每个候选必须同时满足：

1. 发现目录是当前有效 UID 拥有的非符号链接目录，group/other 权限为零；
2. 文件名严格为 `uix-<pid>.json`，以 `NOFOLLOW|NONBLOCK` 打开，是当前 UID 拥有的私有
   普通文件，大小为 1..16 KiB，读取前后 device/inode/mode/size/mtime/ctime 代际不变；
3. 描述符严格声明 `schema=uix.agent.v1`、`state=ready`，PID 与文件名相同，token 为 64 位
   canonical 小写十六进制；
4. endpoint 是同一发现目录内的绝对路径，文件名严格绑定 PID 与 token 前 24 位；目标是当前
   UID 拥有的私有 Unix socket，`/proc/<pid>` 属主相同；
5. 非阻塞 connect 在 500 ms 内完成，随后以 `SO_PEERCRED` 再次验证 UID 和 PID；首条
   `hello` 认证 token，响应 PID 和请求能力集必须匹配。

响应读取使用 4 MiB 硬上限、必须以换行终止，并验证 schema、request ID、response type 与
`ok`。单次 I/O deadline 为 2 秒，一次 inventory 总 deadline 为 3 秒；发现最多处理 256 个
端点、4096 个窗口。单端点不可达、超时或非法时只发布无路径 warning 并令 `complete=false`；
发现根本身不安全时整体失败闭合。不存在发现目录表示当前没有 opt-in UIX 应用，返回完整空清单。

## 能力语义

`window.discover@3` 每次连接当前通过认证的端点并调用 `list_windows`。窗口 ID 私有材料绑定
provider domain、进程 ID、随机 token、UIX window ID 与 generation；进程重启、token 更新或
窗口换代都会让旧 `s2:w` stale。`window.metadata.read@3` 在使用时重新扫描并唯一匹配，不缓存
端点或窗口记录。

`accessibility.tree.read@3` 在重新解析窗口后调用 `snapshot`，复核 window ID、generation 与
closed 状态。Module 从原始 parent 图执行确定性 parent-first BFS，拒绝重复节点、缺失 parent、
cycle 或不可达节点，并在 0..20 深度、1..4096 节点上限内一次性发布终态。公共节点仅包含：

- snapshot-scoped `s2:e` 与 `as3`；
- 可选 automation ID、depth、role、name、focused、enabled；
- UIX 声明的动作名称，仅作为只读能力描述。

`ui.element.locate@2` 复用同一认证 snapshot，但只按 `automationId|role|name|focused|enabled|action`
六种公开字段执行大小写敏感的精确 AND 匹配。零匹配成功、唯一匹配发布当前 `as3 + s2:e` 与
应用客户区 logical frame/visible bounds、两项及以上返回歧义；不发布宿主坐标、ClickablePoint，
也不从矩形中心推断点击点。完整契约由 `contracts/v2/ui-element-location.md` 持有。

`window.revision.wait@1` 接受 `revision-after` 或 `presented-at-least` 二选一条件；100..30000 ms
总 deadline 覆盖重新发现、端点认证与等待，Adapter 为响应解析保留短暂预算。Agent 返回
`changed`、`presented` 或同代际 `closed` 后，Adapter 复核窗口 ID、generation、修订单调性与条件
确已满足，再由 Module 发布 opaque 终态。目标换代、超时和协议不一致均结构化失败，不发布部分结果。

`window.closed.wait@2` 复用同一精确 Agent `wait`，但普通 `changed` 只更新 revision 基线并在
同一认证连接继续等待。总 deadline 同样覆盖重新发现、认证和全部请求；只有原始 window ID 与
generation 的 `closed` 才成功。端点重启或目标换代不重连续等，也不通过标题或全局窗口清单猜测关闭。

`window.state.read@1` 只在 `hello.capabilities.window_state_fields` 协商完整字段集后发布到该窗口。
Adapter 从同一次 `list_windows` 响应提取 logical 客户区尺寸与最大化、最小化、全屏状态；Module 与
既有可见、可呈现事实共同投影为 `uix-framework-current`。这些值是 UIX 跨平台属性的当前观测，固定
`compositorFinalStateConfirmed=false`；未协商字段的旧 Agent 返回 `CAPABILITY_UNAVAILABLE`，不得旁路推断。

UIX `snapshot` 响应本身包含 frame/visible bounds、value 与 selection，因此工具包进程会在有界
协议帧内接收这些敏感字段；Adapter 不缓存。普通树读取仍丢弃 geometry、value 与 selection；只有
独立 `ui.element.locate@2` 会发布唯一匹配元素的应用客户区 logical geometry，仍不发布 value、
selection、文本接口内容、完整树、PID、token、socket 或原生节点 ID。七项 capability 均标记
`read-sensitive`。只读路线
不调用 `perform` 或 `confirm`；revision wait 仅调用 Agent 的条件变量等待，不取得焦点、不发送输入、
不连接 Wayland compositor，
执行域固定 `same-session-no-focus`，无 X11/XWayland 或其他 fallback；`--strict-isolation` 在
扫描端点之前返回 `ISOLATION_REQUIRED`，不得被静默忽略或降级。

## 验收状态

Linux 生产组合根已经路由这七项只读能力以及五项独立 mutation；私有 Unix fixture 覆盖认证握手、
同进程 peer credential、窗口发现、快照解析、revision wait、跨 revision 的精确 generation 关闭等待、
协商后的 targetless 生命周期动作、应用内键盘/指针/关闭目录与 targetless payload、descriptor
symlink 拒绝、opaque 身份和树边界。真实 Wayland `uix-lang-demo` 进一步完成十二项 UIX 窗口 capability 的
端到端验收：真实 descriptor/socket 发现、含合法空名称的 43 节点脱敏树、精确元素 logical geometry、语义 invoke、应用内
pointer/key、revision/presented wait、框架当前状态读取、restore/minimize/maximize/logical resize，以及先挂起 closed wait
再提交 close 的精确 generation 终态链。capability matrix 因此标记 `verified`；该状态只证明 opt-in
UIX 应用路线，不得提升为任意第三方 Linux 应用控制能力。复验步骤见
项目使用说明。
