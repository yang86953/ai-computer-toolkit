# Browser session worker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 定位与所有权

`act/browser-session-worker/v1` 是主进程 Browser Session Module 与固定 Rust sibling worker 之间的私有 JSON Lines 打开会话握手。Browser Session Module 拥有公开会话语义、页面顺序、deadline、取消和结果投影；授权端点 registry、身份生成、协议 parser 与帧状态机是窄 Components；worker 拥有浏览器协议连接、工具自有隔离 profile 和进程树清理；`ComputerControlSystem` 只协调 Policy、Module 与结果。

该协议由 Vikunja #2026 首批冻结。它不直接新增公开 capability，也不允许任意 URL endpoint、executable、profile path、argv、flag、shell、native handle、浏览器原生对象或前台回退。后续 #2027–#2029 必须在保持本边界的前提下实现 worker、页面语义和公开接入。

## 来源与授权

打开请求只允许两种封闭来源：

- `authorized-endpoint`：`endpointId` 必须是私有 registry 在用户显式授权后签发的 `bse1:<32 lowercase hex>` opaque ID；`authorizationNonce` 是本次打开独占的 128-bit 单次随机挑战。公开请求、日志和结果都不包含调试 URL、端口、token、Cookie、profile 或凭据。worker 只能向固定认证本机 broker 兑换该 ID，不能自行扫描端口或附着未登记端点。
- `isolated-profile`：调用方不携带任何路径或启动参数。worker 只使用项目认证的固定 Chromium runtime、工具自有空 profile、固定安全参数和 Job；不得复用用户 profile、标签页、扩展、Cookie 或保存的凭据。

两种来源都必须在任何浏览器协议 dispatch 前完成确认、授权、期限和来源门禁。授权端点 ID 与 nonce 不可跨打开请求复用；来源失败必须结构化返回，不得静默切换到另一来源。

## stdin

第一行必须是唯一 `open`：

```json
{"kind":"open","contractVersion":"act/browser-session-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","timeoutMs":30000,"source":{"kind":"authorized-endpoint","endpointId":"bse1:0123456789abcdef0123456789abcdef","authorizationNonce":"fedcba9876543210fedcba9876543210"}}
```

或工具自有隔离来源：

```json
{"kind":"open","contractVersion":"act/browser-session-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","timeoutMs":30000,"source":{"kind":"isolated-profile"}}
```

`timeoutMs` 为 `1..=30000`，覆盖授权兑换、runtime 启动、协议连接和首个可信 final；不因内部阶段切换重置。单行 UTF-8 上限为 8192 字节，未知字段、错误版本、非规范 nonce、错误来源组合或额外首行都失败闭合。

父进程可在同一 stdin 继续写入一次或多次幂等 `cancel`，其 `requestNonce` 必须与 open 相同。取消只请求协作停止；它不证明 endpoint 或浏览器尚未接受连接。

## stdout 与双阶段事实

stdout 最多输出一条 `open-accepted` 和一条 `open-final`，全部 stdout 的 UTF-8 总上限为 65536 字节。所有帧必须逐字关联相同 `contractVersion` 与 `requestNonce`；stderr 仅供诊断，不承载协议事实。

`open-accepted` 必须在 worker 首次向授权 broker、浏览器 runtime 或协议端点提交可能产生会话资源的请求之前写入并 flush：

```json
{"kind":"open-accepted","contractVersion":"act/browser-session-worker/v1","requestNonce":"0123456789abcdef0123456789abcdef","dispatchAccepted":true,"completed":false}
```

`open-final` 是打开请求的唯一终态，使用四种封闭 outcome：

- `ready`：必须先有 accepted；`completed=true`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，只返回随机 opaque `sessionId=s2:bs:<32 lowercase hex>`，不返回 error。
- `not-dispatched`：不得先有 accepted；`completed=true`、`retrySafe=true`、`acceptedMayHaveOccurred=false`，必须返回安全 error，不返回 sessionId。
- `failed`：必须先有 accepted；`completed=true`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，必须返回安全 error，不返回 sessionId。
- `unknown`：必须先有 accepted；`completed=false`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，error code 固定为 `OUTCOME_UNKNOWN`，不返回 sessionId。

父进程只接受严格顺序 `accepted? -> final`。重复 accepted/final、final 后帧、关联漂移、字段组合冲突、超限输出、非法 UTF-8 或 stdout 噪声都属于协议失败；accepted 后失去可信 final 时必须保守聚合为 `OUTCOME_UNKNOWN`。

## 身份与生命周期

`sessionId` 只关联当前 worker 拥有的会话 registry 记录；它不是 endpoint、端口、PID、profile、WebSocket 地址或浏览器 target ID。后续 page/element ID 也必须由 Module 私有 registry 映射到会话与导航代际，公开值保持 opaque；导航后旧元素必须 stale，不能凭原生 target 或 DOM node ID 重新附着。

父进程 Job 持有 worker 与工具自启浏览器进程树。正常 close、取消、deadline、协议失败和父进程退出都必须有界回收自有资源。授权 endpoint 属于外部显式所有者时，关闭只释放本工具会话与 lease，不终止外部浏览器。工具自有隔离 profile 则由 worker 在进程树退出后回收；清理失败结构化报告，不得触碰不属于本次请求的路径。

## 实现状态

Vikunja #2027 已由提交 `fe1abf5`、`7f6fd17` 实现工具自启隔离来源：固定 Rust worker 负责认证 Chromium、空 profile、`DevToolsActivePort` 与回环 `/json/version` 探测，私有客户端负责 suspended-create、Job-before-resume、持久 stdio、cancel/deadline/断开和整树回收。授权 endpoint 在没有认证本机 broker 时只返回 `not-dispatched`，不扫描、不附着、不回退。固定 production/worker fixtures 已覆盖 ready/close、零帧、accepted-only `OUTCOME_UNKNOWN`、取消与 deadline 竞争、异常退出和 profile 清理；页面命令和公开能力仍不属于本版本打开握手。

## 明确不保证

该握手不定义导航、等待、DOM 查询、点击、输入或截图命令体；这些页面语义由后续 Browser Session Module 契约在 opaque session/page/element identity 上补充。它不提供 EventBus、跨 System 消息、自动重试、通用回滚、用户浏览器发现、凭据读取或前台输入。`ready` 只证明会话打开请求完成，不代表页面操作已经发生。
