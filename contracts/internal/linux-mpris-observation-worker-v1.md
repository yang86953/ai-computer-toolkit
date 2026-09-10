# Linux MPRIS observation worker 协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

协议固定为 `act/internal/linux-mpris-observation/v1`。这是
`linux-mpris-candidate` feature 下的内部候选 read-only worker 协议，不是公开
capability，也不构成生产路由。Component 只解析并验证请求；总线连接、发现、状态读取和
进程生命周期由 feature-gated worker、MPRIS Adapter/Module 和私有 fixture launcher 分层承担。

## 请求

```json
{
  "protocolVersion": "act/internal/linux-mpris-observation/v1",
  "operation": "discover",
  "sessionBusAddress": "unix:path=/run/user/1000/bus",
  "brokerEpoch": "fixture-epoch-1",
  "expectedBusGuid": "0123456789abcdef0123456789abcdef",
  "maximumItems": 128,
  "timeoutMs": 30000
}
```

`operation=state` 必须带 canonical opaque `targetId`，例如：

```json
{
  "protocolVersion": "act/internal/linux-mpris-observation/v1",
  "operation": "state",
  "sessionBusAddress": "unix:path=/run/user/1000/bus",
  "brokerEpoch": "fixture-epoch-1",
  "expectedBusGuid": "0123456789abcdef0123456789abcdef",
  "maximumItems": 128,
  "timeoutMs": 30000,
  "targetId": "s2:m:0123456789abcdef"
}
```

根对象严格封闭，字段名使用 camelCase，所有已知字段拒绝 `null`，并且解析器递归拒绝
未来可能引入的嵌套 `null`。`discover` 不得携带 `targetId`；`state` 必须携带且只接受
`s2:m:[0-9a-f]{16}`。`maximumItems` 缺省为 128，范围为 1..128；state 也解析该字段，
但它只限制 state 的有界重新枚举。`timeoutMs` 缺省为 30000，范围为 1..30000。

`expectedBusGuid` 是可选的 32 位小写十六进制 D-Bus GUID；current-user runtime client 必须携带，
旧 raw fixture 可省略。worker 必须在 `ListNames`、目录发布和任何 Player 属性读取前把它与自身
连接的 `GetId` 结果比较；错配以 `STALE_SESSION` 失败闭合且不发布部分目录。

`sessionBusAddress` 必须是非空的单一 `unix:path=` 地址，最长 4096 字节，不能含 NUL、LF、
CR 或 `;`；不允许使用 D-Bus 多地址备用列表或任何非 unix transport。合法地址可以保留
dbus-daemon 附带的 `,guid=` 参数。`brokerEpoch` 长度为 1..128 字节，且只能含 ASCII 字母、
数字、`.`、`_`、`:`、`-`。
Component 不回显非法原始 JSON，也不执行 provider I/O。

## 生命周期与读取边界

- stdin 接受一个不超过 64 KiB 的请求；一次调用只允许一个请求和一个终态输出，stdout
  之外不得混入诊断；单次终态输出不超过 2 MiB。
- worker 只在显式 `linux-mpris-candidate` feature 下供私有 fixture 使用；默认 CLI、公开
  registry 与生产路由均不包含它。
- `discover` 可以在 `maximumItems` 达到上限时截断，但必须保留截断事实，不得把有界结果
  伪装成完整总线清单。
- `state` 必须使用同一已验证总线配置重新枚举并唯一解析 `targetId`；零匹配是 stale，
  多匹配是 ambiguous，不能任取一个会话。若 `maximumItems` 已截断目录且目标不在有界结果中，
  必须报告 provider unavailable，不得把不完整枚举伪装成 stale。
- `state` 的目标重新枚举、Player 白名单属性读取和 owner 终态复核必须复用同一 D-Bus
  连接和同一 worker 总 deadline，不得再建第二连接或重置时间预算。
- `discover/state` 的每次枚举必须在列名前订阅 `NameOwnerChanged`，对全部候选执行双次 owner
  一致性核验，并在同总线屏障后排空有界变更队列；窗口内任一 MPRIS owner 变化都使整个目录
  stale，不返回部分快照。
- 候选父进程只能从固定 current-user runtime resolver 注入地址和父代际：有效 UID、0700
  `/run/user/<uid>`、同 UID Unix bus socket 与 `GetId(NoAutoStart)` GUID 必须同时成立。runtime
  client 已把 resolver 与固定 observation self-worker 串联并共享原始总期限；worker 还必须核对
  `expectedBusGuid`。地址、inode 与 GUID 不得进入输出，公开 App 仍未接线。
- 仅允许只读的列名/owner 查询和必要的状态属性读取。禁止 metadata、`GetAll`、activation
  和任何 MPRIS 控制 method；不做自动激活，不启动播放器。
- 输出只能是 provider-neutral 的发现或状态事实，不得泄露 bus name、owner、PID、路径、
  原生对象、传输身份或其他 provider identity。
- 不提供 native/transport identity，不使用桌面输入、前景切换、X11、私有 compositor 或
  fallback；不可用、stale、ambiguous、超时和协议漂移必须结构化失败。
- launcher 只使用同一主 CLI 映像的编译期固定隐藏 argv，不搜索 PATH 或 sibling；子进程清空
  继承环境、以 `/` 为工作目录、加入独立 process group 并安装父进程死亡信号。取消、
  外层 deadline、输出超限和进程异常均须整组 kill+wait，且 exit code 必须与唯一终态 JSON 一致。

该协议只冻结 worker 的输入与安全边界；公开媒体 capability 仍须经过独立的生产路由、
权限/隐私评估和用户验收。
