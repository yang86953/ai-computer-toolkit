# Browser session lifecycle contract candidate

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`browser.session.open@1` 与 `browser.session.close@1` 是统一 App facade 上两个独立的
provider-neutral 同步 Command。open 使用 generic `create`，close 使用 generic `close`；
风险与目标种类由 capability ID 决定，不能由 input、provider 或 timeout 改写。

本文件由 Vikunja #2095 冻结公开契约，由 #2096 接入 capability registry、catalog、Policy、
App provider 与通用生产 launcher 路由。当前 Rust 运行时已经可以调用这两个 capability；
#2097 仍负责真实独立 launcher 的 open→close、stale、deadline 与资源回收物理验收。registry
登记和纯 Rust 回归不能单独替代该生产验收，也不能据此放宽任何固定路由或失败语义。

## 输入、精确目标与确认

open 只接受当前 App session 聚合重新签发的 canonical `s2:h:<16 lowercase hex>` 主机
目标。成功时签发全新的 `s2:bs:<32 lowercase hex>` 浏览器会话身份。close 只接受当前
固定 Broker 代际 live registry 中的 canonical `s2:bs`；目标不存在、已关闭、属于旧
Broker 代际或无法重新认证时返回公开 `STALE_SESSION`，不得猜测恢复或重绑身份。

两个 Command 都要求逐操作 confirmation，但不要求前景影响同意。统一 Policy 必须在
解析 input、启动或连接 Broker、查找 registry 以及创建或关闭 worker 前评估确认与前景
同意策略。成功结果固定声明 `confirmationEvaluatedBeforeDispatch=true` 与
`foregroundConsentEvaluatedBeforeDispatch=true`；后者表示无需同意的策略已经评估，
不表示发生了前景操作。

input 必须严格匹配 `browser-session-lifecycle-input.schema.json`：只允许可选整数
`timeoutMs=1..=30000`，默认 5000 ms。该预算覆盖生产 launcher、固定 Broker 连接、双向
认证、请求接受、Browser Session Module 调用和可信 final 聚合。input 不接受 action、
session、confirmed、URL、profile、Cookie、credential、provider、endpoint、pipe、PID、
path、argv、shell、nonce、epoch、revision、CDP method/params 或 native handle/ID。

## 固定 Rust 路由与信息边界

唯一允许的实现路线是当前安装镜像中的 Rust App facade，经固定 Rust Browser Session
Broker client、固定本机会话 Broker 和 Browser Session Module 完成。调用方不能选择
endpoint、transport、worker、浏览器可执行文件或启动参数。能力缺失、认证失败、固定
Broker 不可达或 sibling 缺失必须结构化失败闭合；不得改用网络、用户浏览器、前台键鼠、
任意浏览器协议、C++ 或其他兼容实现。

公开请求、成功结果和统一 error envelope 都不得出现 Broker epoch、request/cancel nonce、
semantic fingerprint、request revision、pipe 名称、PID、SID、integrity、worker、Job、stdio、
profile、WebSocket、CDP、provider 路由键、native error 或句柄。`s2:h` 与 `s2:bs` 是唯一
公开身份；它们不可被调用方解析成原生对象或用于延长 Module 生命周期。

## 接受、完成与失败语义

生产 launcher 的 stdout 每次只输出一个最终 JSON 文档，不公开私有 Broker 的
`broker-ready`、`accepted`、`final` 或 cancel control frame。公开 success schema 只描述
已经取得可信 completed final 的结果；`accepted=true` 表示内部 Command 已越过业务接受点，
`finalStateReached=true`、`dispatchState=completed` 与 `outcome=completed` 表示最终事实
已由同一固定路线确认。success 顶层固定报告
`executionRealm=requiredExecutionRealm=isolated-worker` 与 `executionRealmCertified=true`；
`isolationRequirement=standard` 只允许 `hostImpactPolicy=background-preferred`，strict 只允许
`strict-no-interference`。open success 的顶层 `targetId` 仍是原 `s2:h`，
`data.sessionId` 是新 live `s2:bs`；close success 的顶层 `targetId` 是已关闭的 `s2:bs`，
data 不重复回显该身份。

open 与 close 都会改变外部生命周期，故 success 固定
`targetMayHaveMutated=true`、`retrySafe=false`、`automaticRetryProhibited=true`。open 只在
Module 已拥有 live session、worker、stdio 与 Job 时返回 `state=live`；close 只在目标已
stale 且 Module 已完成逆序资源回收时返回 `closed=true`、`state=closed`。两条路线都不得
改变宿主前景，success 的 data 与 `meta.foreground.unchanged` 必须同时为 true。

失败统一引用 `error-envelope.schema.json` 已登记的公开错误码。确认缺失使用
`CONFIRMATION_REQUIRED`，输入错误使用 `INVALID_ARGUMENT`，目标代际失效使用
`STALE_SESSION`，固定路线缺失或不可达使用现有 `CAPABILITY_UNAVAILABLE`、
`ISOLATED_WORKER_UNAVAILABLE` 或 `BROKER_UNAVAILABLE`，总预算耗尽使用 `TIMEOUT`。
内部 deadline、epoch/nonce 冲突和 transport 状态不得作为新的公开错误码泄漏。

统一 error envelope 的 `error.details` 必须保留请求的公开 `capability`；调用方已提供且与
capability 种类匹配的 canonical `targetId` 也必须逐字保留。若 target 缺失、非字符串、畸形或
kind 不匹配，则 input/confirmation 错误不得捏造 target identity，`targetId` 必须省略。其余字段按
最后可信业务事实使用以下封闭真值矩阵：

| 停止点 | 公开 code | outcome | accepted | finalStateReached | retrySafe | targetMayHaveMutated | automaticRetryProhibited |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 明确在业务接受前 | 已登记且适用于本阶段的公开错误，例如 `CONFIRMATION_REQUIRED` / `INVALID_ARGUMENT` / `STALE_SESSION` / `BROWSER_SESSION_REGISTRY_FULL` / `CAPABILITY_ASSESSMENT_UNAVAILABLE` / `BROKER_UNAVAILABLE` / `TIMEOUT` / `CANCELLED` | `not-dispatched` | false | true | true | false | 不出现 |
| 已接受且取得可信失败终态 | `OPERATION_FAILED` / `HOST_INTERFERENCE_DETECTED` | `failed` | true | true | false | true | true |
| 已接受但无可信 final | `OUTCOME_UNKNOWN` | `unknown` | true | false | false | true | true |

第一行只适用于能够证明未派发的错误；否则即使原始停止原因是 stale、timeout 或 cancel，
也必须使用后两行之一。open 的 `OUTCOME_UNKNOWN` details 不得包含或推测 `sessionId`，close 也
不得添加重复身份。任何 error details 都不得出现 success-only 的 state 或 closed，也不得
泄漏私有 Broker 字段。

在业务接受点前能够证明未派发的取消、超时或拒绝可以返回确定的公开错误。在 accepted
之后若 launcher 无法取得可信 completed、failed 或 cancelled final，只能返回
`OUTCOME_UNKNOWN`；不得伪造新 session identity、`closed=true`、失败、取消成功或安全重试。
调用方不得自动重复 open/close，公共 Adapter 也不得生成新 nonce 重派同一意图。内部
client 可以在同一总 deadline、同一 nonce 与同一 Broker epoch 内 attach/replay，但这些
恢复身份和状态始终留在私有协议边界。
