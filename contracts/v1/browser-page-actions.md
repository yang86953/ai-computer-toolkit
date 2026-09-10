# Browser element actions and page screenshot contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`browser.element.click@1`、`browser.element.type@1` 与 `browser.page.screenshot@1` 是统一 App
facade 的 provider-neutral 页面操作候选。click/type 使用 generic `apply`，是 confirmation-first
Command；screenshot 使用 generic `read`，是无隐藏领域副作用的 Query。三项操作固定使用 Browser
Session Broker 的 `D3 + R0` 请求/响应路线，不使用 EventBus。

本文件由 Vikunja #2303 冻结公开候选。#2306 已接通 Browser Session Module、worker 与 Broker 私有
纵切，#2314 已登记 Rust registry/catalog、live assessment 与 Policy；#2316 已接通唯一 App provider、
client Module、直接 facade 路由与原样生产 launcher。click/type 保持 confirmation-first 和 strict，
screenshot 保持 standard/background-preferred，三者都只认证固定 Browser Session Broker。#2319 已用
原样生产 launcher 闭合业务前、accepted 后故障、隐私与资源恢复矩阵，因此 #2042/#2029 机器技术条件
已闭合；#2007 仍由 `yang86` 独占视觉/交互验收。

## 精确目标与输入

三项操作的 App facade 顶层 `targetId` 都是当前 fixed Broker 代际 live registry 中的 canonical
`s2:bs:<32 lowercase hex>`。严格 input 必须携带当前 `s2:bp:<32 lowercase hex>`；click/type 还必须
携带由当前 page query 签发的 `s2:be:<32 lowercase hex>`。Browser Session Module 在业务接受前按
session、page、element 和 navigation generation 重新解析，分别以 `STALE_SESSION`、`STALE_PAGE`、
`STALE_ELEMENT` 失败闭合，不猜测重绑身份或重新查询元素。

click input 只允许 `pageId`、`elementId` 与可选 `timeoutMs`。type input 还要求 1..16384 UTF-8 bytes
的 `text` 和显式 `replace` 布尔值；JSON Schema 提供可移植字符边界，Rust parser 对 UTF-8 字节数
继续权威判定。两项操作的确认来自统一 facade/Policy，不接受 input 内 `confirmed` 字段。

screenshot input 只允许 `pageId` 与可选 `timeoutMs`。它不接受路径、格式、质量、clip、full-page、
surface、CDP method/params 或 overwrite 控制；成功只返回固定 `image/png`、1..16777216 字符的标准
Base64、1..12582912 原始字节、1..10000 宽高和 16 位小写十六进制稳定摘要。

三项操作的 `timeoutMs` 都是 1..30000，缺省 5000，并覆盖 launcher、Broker 认证、business
acceptance、Module/worker 执行与可信 final 聚合，不因阶段切换重置。公开 input 不接受 sessionId、
worker/page/element ref、CSS、XPath、JavaScript、CDP、Cookie、credential、profile、endpoint、PID、
path、nonce、epoch、revision、provider、native handle 或 native ID。

## 成功结果与身份代际

三项成功都使用 `executionRealm=requiredExecutionRealm=isolated-worker`、
`executionRealmCertified=true`，且宿主前景保持不变。click/type 固定 strict/
`strict-no-interference`；screenshot 固定 standard/`background-preferred`。成功表示已 business
accepted 且取得可信 completed final，所以 `accepted=true`、`finalStateReached=true`、
`dispatchState=outcome=completed`、`retrySafe=false` 与 `automaticRetryProhibited=true`。

click/type 成功返回 request-bound page/element identity、当前正导航代际以及 `clicked=true` 或
`typed=true`；type 只额外返回 `utf8Bytes`，不回显 text、replace 或任何部分文本。二者是 Command，
成功固定 `targetMayHaveMutated=true`。screenshot 成功返回 request-bound page、当前正代际和上述有界
PNG 字段，固定 `readOnly=true` 与 `targetMayHaveMutated=false`。

所有公开结果禁止出现调用方 type text、Cookie、credential、`w1:*`、CDP、DOM/backend node、pipe、
epoch、nonce、fingerprint、revision、PID、SID、integrity、Job、stdio、profile、WebSocket、provider
路由键、文件路径、native error、handle 或 native ID。失败结果不携带成功或部分成功 data。

## 接受、失败、取消与恢复

click/type 的 confirmation-first Policy 必须在 target、input、Broker、page 或 element 访问前运行；
未确认固定返回 `CONFIRMATION_REQUIRED`、`accepted=false`、`outcome=not-dispatched`。screenshot 不要求
确认。三项操作只有能证明尚未 business accepted 时，才可返回 `finalStateReached=true`、
`retrySafe=true` 与 `targetMayHaveMutated=false`；适用错误包括 `INVALID_ARGUMENT`、三类 stale、
`BROKER_UNAVAILABLE`、业务前 `TIMEOUT` 和业务前 `CANCELLED`。

click/type accepted 后取得可信 failed 或 cancelled final 时，保持 `accepted=true`、
`finalStateReached=true`、`retrySafe=false`、`targetMayHaveMutated=true` 和禁止自动重派；没有可信 final
时只能返回 `OUTCOME_UNKNOWN`、`finalStateReached=false`，不得声称没有点击、没有部分输入、已取消或
可安全重试。screenshot accepted 后失败、取消或未知结果始终保持 `targetMayHaveMutated=false`，但同样
不得自动生成新意图。私有 client 只可在同一总 deadline、nonce 和 Broker epoch 内 attach/replay；
取消请求不等于已经 Cancelled，传输断开也不构成取消。
