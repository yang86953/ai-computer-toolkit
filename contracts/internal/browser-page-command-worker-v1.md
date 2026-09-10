# Browser page command worker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 定位与所有权

`act/browser-page-command-worker/v1` 是 Browser Session Module 与固定 Rust browser-session worker 之间的私有 JSON Lines 页面命令协议。Module 拥有公开 session/page/element identity、导航代际、stale 重解析、确认、deadline、取消和公开结果投影；worker 拥有私有 browser protocol endpoint、page/element ref、协议 dispatch 和浏览器资源。`ComputerControlSystem` 只协调 Policy、Module 与结果，不持有 CDP、DOM 或浏览器生命周期。

本协议由 Vikunja #2031 冻结。它不新增公开 capability，不接受 CSS、XPath、JavaScript、CDP method/params、target/node/execution-context ID、debug URL、端口、executable、profile path、argv、Cookie、凭据、用户 profile、native handle 或前台回退。

## 身份与导航代际

- `sessionId` 使用 `s2:bs:<32 lowercase hex>`，由 Browser Session Module 管理。
- worker 私有 `pageRef` 使用 `w1:bp:<32 lowercase hex>`，`elementRef` 使用 `w1:be:<32 lowercase hex>`；它们只能出现在私有 IPC，不能进入公开结果。
- `navigationGeneration` 是 `0..=u32::MAX` 的单调代际。成功导航必须返回新的 `pageRef` 和新代际；旧 page/element 映射随后 stale。
- Module 每次使用公开 element identity 时按 session、page 和导航代际重解析。缺失、重复或旧代际必须结构化失败，不得把原生 node ID 重新附着。
- Module 单代最多拥有 16 个 live session；容量门禁必须在启动 worker 前返回 `BROWSER_SESSION_REGISTRY_FULL`，不得驱逐现有会话。成功导航必须换发 `s2:bp:<32 lowercase hex>` 公开页面身份，并立即使旧页面与旧元素身份 stale。
- Module 当前页面代际最多保留 10000 个公开 `s2:be:<32 lowercase hex>` 元素映射；相同 worker 私有引用必须复用公开身份，容量耗尽返回 `BROWSER_ELEMENT_REGISTRY_FULL` 且不得驱逐旧映射。公开结果不得包含 `w1:*`。

## 命令输入

每条 `command` 包含固定 `contractVersion`、canonical `sessionId`、随机 `requestNonce`、`timeoutMs=1..=30000`、可选私有 `pageRef`、`navigationGeneration` 和一个强类型 `operation`。单行 UTF-8 上限为 65536 字节，总 deadline 覆盖身份重解析、浏览器协议 dispatch、等待与结果读取，不因内部阶段切换重置。

允许操作只有：

- `navigate`：有界 `http://` 或 `https://` URL；初始导航可不带 page ref。
- `wait`：`document-ready`、provider-neutral `element-present` 或有界 `text-present`。
- `query`：只用可选 `role`、`name`、`text` 与 `exact` 组合，至少一个语义字段非空；`maxResults=1..=100`。
- `click`：必须 `confirmed=true`，只接受当前代际的私有 element ref。
- `type`：必须 `confirmed=true`，只接受当前代际 element ref、1..=16384 UTF-8 字节文本和 `replace` 布尔值。
- `screenshot`：不接受路径、格式、质量或浏览器原生参数。

同一 stdin 可继续发送关联 `cancel-command`。取消只请求协作停止，不证明浏览器尚未接受命令。

## 双阶段输出

stdout 总上限为 16908288 字节（16 MiB Base64 + 128 KiB JSON envelope）。每项命令最多输出一条 `command-accepted` 和一条 `command-final`，全部帧必须逐字关联相同版本、nonce 和 operation。

`command-accepted` 必须在首次向浏览器协议提交可能产生页面状态或外部副作用的请求前写入并 flush。`command-final` 使用四种封闭 outcome：

- `completed`：必须先 accepted，`completed=true`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，只携带与 operation 匹配的 provider-neutral data。
- `not-dispatched`：不得先 accepted，`completed=true`、`retrySafe=true`、`acceptedMayHaveOccurred=false`，只携带安全 error。
- `failed`：必须先 accepted，`completed=true`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，只携带安全 error；`STALE_PAGE`、`STALE_ELEMENT`、零/多命中、timeout 和浏览器拒绝均在此封闭分类中表达。
- `unknown`：必须先 accepted，`completed=false`、`retrySafe=false`、`acceptedMayHaveOccurred=true`，error code 固定为 `OUTCOME_UNKNOWN`。

accepted 后取消、deadline、断线、非法 UTF-8、超限输出、关联漂移或缺失可信 final 都必须由父 Module 保守聚合为 `OUTCOME_UNKNOWN`。零帧退出保持未派发语义。重复 accepted/final、final 后帧和 stdout 噪声均属于协议失败。

## 成功数据

成功 data 采用按 operation 封闭的字段集合：

- navigate：`kind=navigate`、私有 `pageRef`、`navigated=true`；Module 以新随机公开 page identity 替换私有 ref。
- wait：`kind=wait`、`conditionMet=true`。
- query：`kind=query`、有界 `matches`、`matchCount`、`truncated`；每个 match 只包含私有 element ref、可选 role/name/text 和 enabled。零命中与多命中是不同的可信数据，不做猜测。
- click：`kind=click`、`clicked=true`。
- type：`kind=type`、`typed=true`、有界 `utf8Bytes`。
- screenshot：`kind=screenshot`、`mimeType=image/png`、有界 `pngBase64`/`pngBytes`、尺寸与稳定 digest；不包含文件路径、surface 或浏览器原生对象。

## 明确不保证

该协议不定义公开 capability 名称、launcher 参数、跨进程持久 registry、自动重试、通用事务或凭据注入。#2032 实现私有 CDP 传输和 worker 页面循环，#2033 实现 Module identity registry 与语义聚合，#2029 才允许接入公开 registry、Policy 和 launcher。

## 实现状态

2026-08-12，Vikunja #2032 已实现本协议的固定 Rust worker 边界：纯 Rust loopback WebSocket/CDP 传输、工具自有 target、navigate/wait/query/click/type/screenshot、当前导航代际的私有元素 registry、accepted/final、总 deadline、取消、断线与 `OUTCOME_UNKNOWN` 均已接入确定性 runtime fixture。worker 不接受通用 CDP 或调用方脚本；点击、输入只解析 registry 中的 backend node ID，截图只返回有界 PNG 事实。公开 identity registry、Module 聚合和 launcher 路由仍不属于本协议实现，由 #2033、#2029 承担。

2026-08-12，Vikunja #2033 第一批让 parent `BrowserSessionProcess` 在 ready 后继续持有唯一页面帧 receiver，并按 request nonce、操作种类、accepted/final、总 deadline、cancel、进程退出与协议污染聚合单个页面命令；确定完成后会话可继续使用，unknown 或失联则整树回收。真实生产 worker/runtime fixture 已验证 parent 导航后仍能优雅关闭会话；该批尚未建立 Module 的公开 session/page/element registry，也未接入 launcher。

2026-08-12，#2033 第二批建立进程内 `BrowserSessionModule`：单代最多 16 个 live session，Module 接管 worker Job/stdio 的完整生命周期，导航前验证 URL 与单命令总预算，成功后把私有 `w1:bp` 转换为随机公开 `s2:bp` 并单调推进代际；再次导航立即使旧公开页面 stale。Component 级错误、协议漂移和 unknown 回收都会从 registry 失效会话。真实生产 worker/runtime fixture 已覆盖两次导航、私有引用零泄漏、旧页面 `STALE_PAGE`、显式 close 与关闭后 `STALE_SESSION`；元素 registry 与 wait/query/click/type/screenshot 的 Module 投影仍由 #2033 后续批次完成，launcher 保持未接入。

2026-08-12，#2033 最终批完成 Module 页面语义聚合：document/element/text 三种 wait、零/多命中 query、当前代际公开 `s2:be` registry、confirmation-first click/type 和有界 PNG screenshot 均已转换为强类型领域结果。重复 query 稳定复用公开元素身份；未知元素返回 `STALE_ELEMENT`，导航继续整体清空元素映射；所有结果只包含 role/name/text/enabled、计数、动作事实或 PNG 事实，不包含 `w1:*`、CDP 或 native identity。真实生产 worker/runtime fixture 已覆盖六操作、确认顺序、公开身份稳定、私有引用零泄漏与显式关闭；#2033 至此完成，公开 capability/Policy/launcher 仍严格留给 #2029。
