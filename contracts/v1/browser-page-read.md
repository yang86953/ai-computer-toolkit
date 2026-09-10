# Browser page navigation and read contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`browser.page.navigate@1`、`browser.page.wait@1` 与 `browser.page.query@1` 是统一 App
facade 上三个 provider-neutral 同步操作。navigate 使用 generic `apply`，是 confirmation-first
Command；wait 与 query 使用 generic `read`，是无隐藏领域副作用的 Query。三项操作均使用
固定 Browser Session Broker 的 `D3 + R0` 请求/响应路线，不使用 EventBus。

本文件由 Vikunja #2265 冻结公开契约；#2269 接通 Broker/System/Module 私有纵切，#2277 完成
registry、catalog、assessment 与 Policy，#2283 接通 App provider、facade、client Module 和原样生产
launcher，#2290 闭合公开隐私、错误与恢复矩阵。#2007 仍由 `yang86` 独占视觉/交互验收。

2026-08-16 实现状态：三项 capability 已可经统一 App facade 执行。facade 对 live `s2:bs` 直接选择
唯一 Browser Session provider，不另发 freshness Query；Broker 的 accepted 前预检是 session/page
stale 权威。公开 input 在任何 Broker I/O 前严格解析，wait/query completed 将 request-bound `pageId`
与 Module 报告的正 `navigationGeneration` 逐层投影。原样 `Invoke-ComputerControl.ps1`、生产 Rust
Broker/worker 与工具 runtime fixture 已完成 open→navigate→wait→query→close，1 passed、0 failed；
错误与恢复 fixture 另行覆盖业务前拒绝、stale、可信失败、超时未知结果、真实进程中断和资源回收。
这些机器证据闭合 #2041 的技术条件，但不替代 #2007 用户验收。

## 精确目标与输入

三项操作的 App facade 顶层 `targetId` 都是当前 fixed Broker 代际 live registry 中的 canonical
`s2:bs:<32 lowercase hex>`。navigate 以该会话为写目标；wait/query 还必须在严格 input 中提供
当前 `s2:bp:<32 lowercase hex>`。Browser Session Module 在业务接受前重新核对 session、page 与
navigation generation；未知、已关闭、旧 Broker 代际或旧导航代际分别以 `STALE_SESSION`、
`STALE_PAGE` 失败闭合，不猜测重绑身份。

navigate input 只允许 `url` 与可选 `timeoutMs`。URL 为 1..8192 字符、仅小写 `http://` 或
`https://` scheme、具有非空 authority，且不得含 userinfo、反斜线、控制字符或空白。schema
执行可移植的外层门禁，Rust parser 继续权威验证 DNS/IPv4/方括号 IPv6 host 与可选 u16 port。

wait input 只允许 `pageId`、`condition` 与可选 `timeoutMs`。condition 是以下封闭集合：

- `document-ready`：不接受其他字段；
- `element-present`：只接受 provider-neutral selector；
- `text-present`：只接受 1..1024 字符文本与可选 `exact`。

query input 只允许 `pageId`、selector、可选 `maxResults=1..100` 与可选 `timeoutMs`。selector
只允许 role/name/text/exact，至少一个语义字符串存在且长度为 1..1024 字符；`exact` 缺省 false，
`maxResults` 缺省 100。三项操作的 `timeoutMs` 均为 1..30000，缺省 5000，并覆盖 launcher、
Broker 认证、business acceptance、Module/worker 执行与可信 final 聚合，不因阶段切换重置。

公开 input 不接受 confirmed、sessionId、elementId、CSS、XPath、JavaScript、CDP method/params、
Cookie、credential、endpoint、WebSocket、profile、PID、path、argv、shell、nonce、epoch、revision、
provider、worker ref、native handle 或 native ID。navigate 的确认由统一 facade/Policy 控制，不是
业务 input 字段；Policy 必须在 URL、target、Broker 连接或 worker I/O 前评估确认与前景策略。

## 成功结果与身份代际

成功结果固定使用 `executionRealm=requiredExecutionRealm=isolated-worker`、
`executionRealmCertified=true`，且宿主前景不变。navigate 使用 strict/
`strict-no-interference`；wait/query 使用 standard/`background-preferred`。三项 success 都表示
内部请求已 business accepted 并取得可信 completed final，因此 `accepted=true`、
`finalStateReached=true`、`dispatchState=outcome=completed`、`retrySafe=false` 与
`automaticRetryProhibited=true`。

navigate 成功后 Module 必须推进恰好一代、签发全新随机 `s2:bp`，立即使旧 page 与全部旧
`s2:be` stale，并返回 `navigated=true`、新 `pageId` 和 1..u32::MAX 的
`navigationGeneration`。wait 只返回当前 page、当前代际与 `conditionMet=true`。query 返回当前
page、当前代际、有界 matches、worker 报告的 `matchCount` 与 `truncated`；零命中和多命中都是
可信结果。每个 match 只允许随机公开 `s2:be`、可选 role/name/text 与 enabled，同一当前代际
私有元素引用必须稳定复用同一公开 ID。

navigate 是 Command，success 固定 `targetMayHaveMutated=true`。wait/query 是 Query，success
固定 `readOnly=true` 与 `targetMayHaveMutated=false`。所有公开结果禁止出现 `w1:*`、CDP、DOM/
backend node、pipe、epoch、nonce、fingerprint、revision、PID、SID、integrity、Job、stdio、profile、
WebSocket、provider 路由键、native error、handle 或调用方 URL/selector 文本回显。

## 接受、失败、取消与恢复

生产 launcher 最终只输出一个公开 JSON 文档，不公开 broker-ready、accepted/final 或 cancel frame。
传输接管、business accepted 与 completed/failed/cancelled 必须保持独立。只有能证明尚未 business
accepted 时，才可返回 `outcome=not-dispatched`、`accepted=false`、`finalStateReached=true`、
`retrySafe=true`、`targetMayHaveMutated=false`，适用错误包括 `CONFIRMATION_REQUIRED`、
`INVALID_ARGUMENT`、`STALE_SESSION`、`STALE_PAGE`、`BROKER_UNAVAILABLE`、`TIMEOUT` 与
`CANCELLED`。

navigate accepted 后取得可信失败 final 时，公开 `OPERATION_FAILED`、`outcome=failed`、
`accepted=true`、`finalStateReached=true`、`retrySafe=false`、`targetMayHaveMutated=true` 与
`automaticRetryProhibited=true`。accepted 后丢失可信 final 时只能返回 `OUTCOME_UNKNOWN`，
`finalStateReached=false`，不得伪造新 page identity、代际、失败、取消成功或安全重试。

wait/query accepted 后的可信失败或取消仍保持 `targetMayHaveMutated=false`；没有可信 final 时也
使用 `OUTCOME_UNKNOWN`，但不得声称页面已改变。为保持 request identity、deadline 与结果顺序，
三项操作的 accepted/unknown 均禁止公开 Adapter 自动生成新意图。私有 client 只可在同一总 deadline、
同一 nonce 与同一 Broker epoch 内 attach/replay；取消请求不等于已经 Cancelled，传输断开也不构成取消。

2026-08-16 的 #2290 回归矩阵确认：未确认 navigate、standard navigate、非法 URL/selector 和私有字段
均在业务及不应发生的 Broker/target 访问前失败；旧会话和二次导航后的旧页面保持 stale；业务前 deadline
保持 not-dispatched。accepted 后可信 failed/cancelled final 按上述真值投影，navigate 失败仍保守标记
可能已修改，wait/query 始终只读；accepted 后 deadline、Broker 断开或真实 Ctrl+Break 中断只投影
`OUTCOME_UNKNOWN`，不伪造 Broker 取消、不自动重派。页面失效后，Browser Session Module 将原进程、Job、
stdio、open nonce 和 profile 所有权转交同一 Module 的 close task；close、未知结果、Broker 重启与 stale
恢复测试确认 worker/runtime/profile 最终收敛，且公开 JSON 隐私扫描未发现调用方 URL/selector、Cookie、
credential、`w1:*`、CDP、endpoint、PID、profile、worker 或 native 事实。
