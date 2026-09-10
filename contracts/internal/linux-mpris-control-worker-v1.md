# Linux MPRIS control worker 协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

协议固定为 `act/internal/linux-mpris-control/v1`。这是
`linux-mpris-candidate` feature 下的内部候选控制 worker 输入协议，不是公开
capability，也不构成生产路由。生产 execution 固定为 `none`；Component 只解析并验证请求，
不建立总线连接、不读取 provider、不调用 MPRIS method。

## 请求

```json
{
  "protocolVersion": "act/internal/linux-mpris-control/v1",
  "sessionBusAddress": "unix:path=/run/user/1000/bus",
  "brokerEpoch": "fixture-epoch-1",
  "expectedBusGuid": "0123456789abcdef0123456789abcdef",
  "targetId": "s2:m:0123456789abcdef",
  "operation": "play",
  "confirmed": true,
  "maximumItems": 128,
  "timeoutMs": 30000
}
```

根对象严格封闭，字段名使用 camelCase，递归拒绝任意位置的 `null`。必填字段是
`protocolVersion`、`sessionBusAddress`、`brokerEpoch`、`targetId`、`operation` 和
`confirmed`；`maximumItems` 缺省为 128，范围为 1..128；`timeoutMs` 缺省为 30000，范围为
1..30000。

`expectedBusGuid` 是可选的 32 位小写十六进制 D-Bus GUID；current-user runtime client 必须携带，
旧 raw fixture 可省略。worker 必须在 `ListNames`、目标解析、属性门禁、accepted 和 method 前把它
与自身连接的 `GetId` 结果比较；错配以 `STALE_SESSION` 失败闭合且 provider read/method 为零。

`targetId` 必须是 canonical `s2:m:[0-9a-f]{16}`。`operation` 只允许 `play`、`pause`、
`toggle-play-pause`、`stop`、`skip-next`、`skip-previous`。`confirmed` 必须是布尔值；
`confirmed=false` 仍是可解析请求，但 worker 必须在任何 provider I/O 前发布
`pre-dispatch-rejected` final，并且 provider read 为零。

`sessionBusAddress` 必须是非空的单一 `unix:path=` 地址，最长 4096 字节，不能含 NUL、LF、
CR 或 `;`；不允许 D-Bus 多地址备用列表或非 unix transport。合法地址可以保留
dbus-daemon 附带的 `,guid=` 参数。`brokerEpoch` 长度为 1..128 字节，只能使用 ASCII 字母、
数字、`.`、`_`、`:`、`-`。输入 JSON 总长度不超过 64 KiB，单次 stdout JSONL（accepted 与
final 合计）输出不超过 2 MiB。

## JSONL 输出时序

worker 每次 stdin 请求只发布固定的 JSONL 时序：

- 在 dispatch 前只允许一个 final frame。解析失败、目标/权限/能力预检失败以及
  `confirmed=false` 都必须在该 frame 中结束，不得先发布 accepted。
- `confirmed=true` 且所有解析、目标重新解析、能力读取和其它 dispatch 前门禁通过后，
  在真正调用 MPRIS method 之前写出并 flush 恰好一行 accepted frame：

  ```json
  {
    "protocolVersion": "act/internal/linux-mpris-control/v1",
    "phase": "accepted",
    "targetId": "s2:m:0123456789abcdef",
    "operation": "play"
  }
  ```

  accepted frame 必须恰好包含上述固定字段，不可替换为 provider 名称、总线地址、owner、
  路径、PID 或 transport identity。
- accepted frame 之后只允许一个 final frame。成功 final 使用公开
  `media.playback.control@2` 结果；失败 final 使用稳定 error envelope。
- accepted 一旦 flush，任何后续缺失响应、provider 异常、总 deadline 超时、取消、连接
  丢失或无法证明终态，都必须映射为 `OUTCOME_UNKNOWN`，`retrySafe=false`，并禁止自动重试。
  不得把 accepted 之后的异常伪装成业务失败或成功。

accepted 只证明控制 method 已越过 dispatch 前门禁并已进入调用边界，不证明播放器已经消费
控制、不证明播放状态改变，也不证明最终 UI 或 compositor 状态。普通 method reply 也只允许
形成约定的最终公开结果；`effectConfirmed` 不因 reply 自动变为 true。

## provider 与生命周期边界

- worker 只在显式 `linux-mpris-candidate` feature 下供私有 fixture 使用；公开 registry 与
  默认生产路由保持 `execution:none`。
- launcher 仅以同一主 CLI 映像的固定隐藏 argv 启动 self-worker；继承环境清空、工作目录
  固定为 `/`、进程组独立且安装父进程死亡信号，异常分支必须整组 kill+wait。不搜索
  PATH/sibling，不从 argv 或环境变量接收目标、操作或总线地址。
- 控制前必须先重新解析唯一目标，并在调用 method 前读取必要的控制能力；只允许对应的
  MPRIS 控制 method，不允许 metadata、`GetAll`、activation 或其它隐式启动。
- 目标重新枚举、能力门禁、owner 复核、accepted 前检查和最终 method 必须共享同一 D-Bus
  连接与同一 worker 总 deadline，不能在 resolve 内再启动独立 discover deadline。
- 重新枚举必须在列名前订阅 `NameOwnerChanged`，对全部候选执行双次 owner 一致性核验，并在
  同总线屏障后排空有界变更队列；枚举窗口内任一 MPRIS owner 变化都必须在 accepted 前以 stale
  失败闭合。
- 候选父进程只能从固定 current-user runtime resolver 注入地址和父代际：有效 UID、0700
  `/run/user/<uid>`、同 UID Unix bus socket 与 `GetId(NoAutoStart)` GUID 必须同时成立。runtime
  client 已把 resolver 与固定 control self-worker 串联并共享原始总期限；worker 还必须核对
  `expectedBusGuid`，仍未接入公开 App。
- 不泄露 MPRIS bus name、owner、PID、路径、原生对象、传输身份，不使用桌面输入、前景
  切换、X11、私有 compositor 或 fallback。
- 该协议只冻结输入与隔离 worker 的 frame 时序；真实播放器消费与用户验收仍然暂缓。
