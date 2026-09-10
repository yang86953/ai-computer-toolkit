# 独立交互会话 Session Broker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 边界与部署

`act/interactive-session-broker/v1` 是 host `ComputerControlSystem` 与目标交互会话内固定
Rust session broker 之间的本机 D3/R0 Command 协议，不是 EventBus、远程控制接口或公开
provider API。用户或部署方必须在已经登录、解锁并显式授权的目标交互会话内启动
`ai-computer-toolkit-interactive-session-broker.exe`；host 不创建或解锁 Windows 会话，
不接收登录凭据，也不把启动失败回退到当前桌面。

主程序、broker 与 `ai-computer-toolkit-interactive-command-worker.exe` 必须来自同一安装目录。
broker 构建标识只绑定包名、包版本和协议角色，不是代码签名或文件哈希；生产认证同时依赖
内核提供的对等进程身份、固定 sibling 完整路径、Windows session、同一用户主体、活动
Default 输入桌面和本连接随机 lease。调用方不能提供 pipe、端口、路径、argv、shell、
凭据、PID、SID 或 native session number。

## 固定传输与对等认证

- Windows Adapter 私有派生每个系统 session 的固定 message-mode named pipe；pipe 使用
  protected DACL，只允许 LocalSystem 与当前用户主体，拒绝远程 client，并要求 first instance。
- host 从内核读取 server PID，broker 从内核读取 client PID；双方分别核对固定 sibling
  镜像、预期系统 session 和同一用户主体。host 与 broker 位于同一系统 session 时认证失败。
- broker 启动时及每条连接开始时重新认证自己仍处于 active interactive session 和
  Default input desktop。锁屏、UAC、安全桌面、Session 0 或状态不可认证时不发布 endpoint。
- 单帧上限为 256 KiB。协议只接受字段封闭的 UTF-8 JSON object；未知字段、版本漂移、
  nonce 漂移或额外帧失败闭合。
- host 的连接、认证、lease 与 worker 共用一个总 deadline；broker 的空闲 accept 和每帧读取
  都以短轮询观察取消与单调 deadline，不沿协议阶段重置调用方预算。
- 最终 observation、rejected 或 command-result 写入预留的单帧有界输出缓冲后，broker 最多
  等待固定握手窗口让 host 读取并关闭连接。禁止使用无法观察取消/deadline 的 pipe flush；
  挂起或退出的 client 只关闭本连接，随后 broker 重建同名首实例继续服务。

## 连接状态机

wire 形状由 `interactive-session-broker-v1.schema.json` 冻结。每条连接只能选择以下一种
生命周期：

1. host 发送 `observe` 首帧；broker 返回 `observation` 与认证 endpoint 证明后断开，
   不签发 lease，也不启动 worker。
2. host 发送 `open-command-lease` 首帧；broker 返回 `command-lease` 与本连接唯一
   `endpointLeaseNonce`。
3. host 在同一连接发送唯一 `execute` 帧。该帧必须复用首帧 `requestNonce`、本连接 lease
   和当前 `interactiveSessionId`，并包含完整
   `act/interactive-command-worker/v1` 请求。
4. broker 在启动 worker 前再次验证 endpoint、lease、strict 计划和 command worker 请求，
   再通过固定 Job runner 启动同目录 worker；每个 lease 最多启动一个 worker。
5. worker 完成且进程树回收后，broker 返回 `command-result`；dispatch 前拒绝返回
   `rejected`。连接关闭即使旧 lease 失效，后续请求必须重新发现或重新握手。

`requestNonce` 只关联当前请求，`endpointLeaseNonce` 只绑定本连接 worker 生命周期；二者
都是 32 位小写十六进制系统随机值，不能代替 OS 访问控制或对等认证，也不得相互复用。
broker 每次启动产生新的 `s2:i` 授权代际，旧 ID 返回 `STALE_SESSION`，不能自动重绑。

## endpoint 证明

只有下列事实全部成立的 endpoint 才能进入 `discover isolation` 结果：

- canonical `s2:i` 授权代际、`isolated-worker` 执行域和
  `independent-interactive-session` 隔离类别；
- active interactive session、active Default input desktop 与不同 host 系统 session；
- 固定四条 provider-neutral capability：`ui.input.key@1`、`ui.input.pointer@1`、
  `window.lifecycle@1`、`window.close@1`；
- 固定 broker 构建角色、固定 command worker sibling 已安装，以及
  `os-process-session-principal-integrity-and-fixed-image` 对等认证等级。

公开 observation 只投影 `contracts/v1/interactive-session-observation.schema.json` 允许的
字段，不返回 pipe、系统 session、用户名、SID、PID、路径、桌面名或 handle。单个候选未部署、
认证失败或在握手中消失时不会进入结果；精确执行仍会在新连接上重新认证。

## 接受、完成与恢复

broker 与 command worker 都分开记录 transport accepted、business accepted、completed 和
outcome。协议、认证、stale、lease、confirmation、strict 计划或 command 语义在 worker
启动前失败时，返回 `not-dispatched`、`retrySafe=true`、
`targetMayHaveMutated=false` 和零当前桌面 fallback 证据。

host 完整发送 command 后，写入、读取、timeout、取消、broker/worker crash 或无法认证的
响应都必须保守映射为 `OUTCOME_UNKNOWN`、`retrySafe=false`、
`targetMayHaveMutated=true`，不得自动重试。Job runner 负责 worker 进程树的 deadline、输出
上限与关闭即回收；它不能证明应用自身已派生且脱离 worker Job 的外部进程已经回滚。

## 当前实现与验收边界

Rust 生产候选已经接入固定 broker binary、Windows 本机 endpoint、双向 peer 认证、一次性
lease、固定 Job-bounded command worker，以及 `ComputerControlSystem`/Policy 的发现与四条
通用 mutation 路由。`ISO-D` 已提供无第二授权会话的生产 launcher 失败闭合和合成异常
矩阵；真实授权 endpoint 的跨系统 session 机器证据仍在 #2001，不能由同会话或纯协议测试
替代。

真实独立交互会话中的当前桌面零干扰、目标结果和退出后无残留只由 Vikunja #2008 的
`yang86` 验收。自动化、同会话测试或 broker 自报均不得替代该门禁。
