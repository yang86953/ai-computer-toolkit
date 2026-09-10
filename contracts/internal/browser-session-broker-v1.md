# Browser session broker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 定位、边界与所有权

`act/browser-session-broker/v1` 是固定同会话 Windows Browser Session Broker 的本机 D3/R0 Command/Query 协议，由 Vikunja #2044 冻结。broker 物理宿主只是部署/组合根与认证 IPC 宿主，只创建、启动和关闭 `ComputerControlSystem`；只有 System 创建、拥有并调用 Browser Session Module。Module 是 live session、page、element registry，以及 worker、Job 和 stdio 的唯一所有者。client 只调用 System 暴露的内部协议入口，并只拥有单次请求的总预算、关联和结果聚合；broker 不能成为独立领域所有者、直接创建 Module 或持有 Module 领域状态。

这里的“同会话”是 OS system session、principal 和完整性级别均与当前 System 相符的本机调用方，而不是调用方填写的字符串。协议不使用 EventBus：请求须经这个直接 Command/Query 边界，broker 不发布领域事件或跨 System 消息。

首批只冻结内部协议和同源 Rust 契约；不实现 IPC，不新增公开 capability，不接入 launcher、Policy 或生产路由。

## Windows 传输与对等认证

- broker 在每一代启动时只绑定一个固定、first-instance、message-mode named pipe；调用方不能在 request 或任何配置中指定 pipe。first-instance 创建失败必须终止 broker 启动，不能附着既有 pipe。
- 为允许在途 execution 之外的 attach/cancel 连接，broker 必须把真正 first-instance 作为不承载业务的进程内 guard 保留到 System 关闭之后，并在同名 pipe 上使用固定总量 16 的 secondary instances；所有 secondary 必须复用同一 protected DACL、local-only/message mode、不可继承属性与方向性帧预算。owner 关闭先禁止创建和接受 secondary，既有 secondary 在认证、读写与 relisten 前都必须核对 owner live；宿主停止 accept、回收连接并关闭 System 后，最后释放 first-instance guard。
- pipe 使用 protected DACL 并拒绝 remote client。broker 在读取业务 JSON 前从内核取得 client PID，并验证固定 client sibling image、当前 System session、current principal 与允许的 integrity；client 也必须在发送任何业务 JSON 前从内核取得 server PID，并验证固定 broker sibling image、同一 System session、current principal 与允许的 integrity。
- Windows 把当前用户的 pipe 写权限同时计入创建同名 instance 的权限，因此 protected DACL 与 first-instance guard 保证的是 endpoint 代际、远程拒绝和每连接认证完整性，不承诺抵御已经能以同一 principal 执行代码的本地进程所造成的可用性拒绝服务；该进程即使占用 secondary 名额或吸走连接，也不能通过固定 sibling image/session/SID/integrity 认证，双方不得发送或解释业务 JSON。instance 容量或连接预算耗尽必须结构化失败闭合，不得改用另一 pipe、网络、用户浏览器或 C++。
- 任一方向无法取得内核 peer PID、image/session/principal/integrity 不匹配或 first-instance 事实不成立，都必须在认证边界失败闭合；双方不得发送或解释业务 JSON，不得降级、重试另一 pipe 或改用网络传输。
- 双向 OS peer authentication 成功后，server 必须先发送唯一、封闭的 `broker-ready` control frame，只包含 `kind=broker-ready`、`contractVersion` 与本次随机 `brokerEpoch`。client 验证该帧后才可发送 request 或 cancel。
- endpoint、path、argv、shell、PID、profile、WebSocket、CDP、Cookie、credential、native handle/ID 等字段不属于 schema；有这些未知字段的帧因 `additionalProperties:false` 被拒绝。字符串内容也不得被解释为这些私有输入。

认证前断线、业务接受前断线和普通 client response 连接断线都不会使既有 session/page/element stale，也不隐式取消请求。只有 worker、Job 或 Module 生命周期失信，broker/System 终止，或者 accepted mutation Command 丢失结果且 System 无法证明 registry 一致性时，System 才使对应 session 或导航代际 stale。Query 断线只让该次结果未知，`targetMayHaveMutated=false`，目标与 registry 保持不变。每个启动 epoch 产生随机 `brokerEpoch`；`broker-ready`、accepted、final、cancel receipt 和 cancel rejection 都回显其 32 位小写十六进制值。broker/System restart 后旧的 `s2:bs`、`s2:bp`、`s2:be` 不可重绑且返回结构化 stale，不从 registry、浏览器或外部状态猜测恢复。旧 epoch 中 outcome unknown 的 request 不能在新 epoch 以同一 nonce 自动重发；调用方必须明确创建新的用户意图和新的 nonce。

## 请求、身份与预算

每个 `request` 固定包含：`contractVersion`、32 位小写十六进制 `requestNonce`、16 位小写十六进制 `semanticFingerprint`、从 `broker-ready` 取得的 `expectedBrokerEpoch`、`remainingTimeoutMs=1..=30000`、一个封闭的 `operation` 字符串标签及其对应的 root 操作字段。`requestNonce` 是稳定 command id 与响应关联。System 在整个 live broker epoch 内以固定容量 4096 的 execution ledger 保留每个 nonce 的完整 canonical semantic key、首次单调绝对 deadline、最新 accepted/final snapshot 和 `requestRevision`；terminal entry 与 cancellation tombstone 一直保留到 epoch 结束，不得静默淘汰。容量已满时，新 nonce 必须在 business acceptance、Module 调用和 dispatch 前以 `BROKER_REQUEST_LEDGER_FULL` 结构化拒绝，不能驱逐旧 entry。

canonical key 是规范化的不可变语义（version、expected epoch、operation、target 和业务参数；排除 nonce、remaining timeout、confirmed）；System 重算它，并只将 `semanticFingerprint` 作为 wire 校验/快速摘要。64 位 fingerprint 绝不能代替完整 key 的同义证明。wire fingerprint 与该 frame 自身 canonical key 不一致时，必须在进入 ledger 前拒绝为 `INVALID_ARGUMENT`；相同 nonce、相同 canonical key 的重送只 attach 到原 execution 并 replay 最新 snapshot，绝不重新 dispatch；相同 nonce 携带另一个内部自洽的 fingerprint 或不同完整语义时，必须拒绝为 `NONCE_SEMANTIC_CONFLICT`，不能改变原 entry。

canonical key 与 fingerprint 的逐字算法属于 v1 契约：key 先写入 `act/browser-session-broker/v1`，其余每个字符串按 `|<UTF-8 字节数>:<原字符串>` 追加；依次追加 expected epoch、operation，再按字段顺序追加 target 与业务参数。字段顺序固定为：close=`sessionId`；session.inspect=`sessionId`；navigate=`sessionId,url`；wait=`sessionId,pageId,condition`；query=`sessionId,pageId,selector,maxResults十进制`；click=`sessionId,pageId,elementId`；type=`sessionId,pageId,elementId,text,replace`；screenshot=`sessionId,pageId`；open 无后续字段。布尔值用 `1`/`0`。selector 先追加 `selector`，再按 role/name/text 各追加存在位 `1` 与值、或仅追加不存在位 `0`，最后追加 exact；condition 先追加 `document-ready`、`element-present` 或 `text-present`，后两者再按各自 selector 或 text/exact 规则追加。不得做大小写、URL、Unicode 或空白归一化。`semanticFingerprint` 是该完整 key 的 FNV-1a 64-bit：offset basis=`0xcbf29ce484222325`、prime=`0x100000001b3`，逐 UTF-8 字节 xor 后 wrapping multiply，最后编码为 16 位小写十六进制。ledger 仍必须保存并比较完整 key，不能以该摘要代替同义判断。

每个 wire accepted/final 都带 u64 `requestRevision`。某 nonce 的首个可观察 snapshot revision 为 0，每次合法状态推进严格加 1；若 operation 越过 business acceptance，首个 snapshot 必须是 `accepted(revision=0)`，其业务 final 必须是 `revision=1`，不得以业务 final revision 0 跳过 accepted。`rejected`、`expired-before-acceptance` 或 `cancelled-before-acceptance` 可作为唯一的业务前 revision 0 final。final 是 terminal-wins，任何低 revision、同 revision 不同内容、final 后 accepted/final 或其他状态回退都必须拒绝为协议污染，不能覆盖 ledger。相同 nonce 重送只返回当前最高 revision 的同一事实。

request 或 cancel 的 `expectedBrokerEpoch` 与当前 epoch 不匹配时，transport 可以完整收取该帧，但 System 必须在 business acceptance 与任何 target/payload/domain 解析前结构化拒绝为 `STALE_BROKER_EPOCH`，并回显当前 `brokerEpoch`；不得写入 execution ledger、cancel ledger 或调用 Module。领域 request 使用 `final outcome=rejected`，cancel 使用 `cancel-rejected` control frame。

wire 还定义在独立认证连接上发送的 `cancel` control frame，不计入九项领域 operation。它必须携带固定版本、稳定且独立的 `cancelRequestNonce`、作为 target correlation 的原 `requestNonce`，以及 `expectedBrokerEpoch`；`cancelRequestNonce` 不得与 target request nonce 互换语义。System 在单一线性化点把 cancel 与 target execution ledger 状态串行决定。cancel ledger 固定最多 4096 个 entry，并以 `cancelRequestNonce` 保存完整 canonical cancel key（依次为 version、`cancel` 标签、target request nonce、expected epoch）和最新 revision直到 epoch 结束；同 nonce、同语义重送幂等返回当前事实，不得重复请求停止。相同 cancel nonce 携带不同语义必须以 `NONCE_SEMANTIC_CONFLICT` 的 `cancel-rejected` 拒绝且不改变既有 cancel 状态；cancel ledger 容量不足也必须在业务接受前结构化拒绝，不得静默淘汰或漏记 cancel identity。

`cancel-receipt` 必须回显 `cancelRequestNonce`、target `requestNonce`、当前 `brokerEpoch`、从 0 开始且只在状态推进时增加的 `cancelRevision` 与状态。若线性化点尚无 target request，System 必须在 execution ledger 为 target nonce 安装 epoch 内 terminal cancellation tombstone，再返回 `unknown-request`；首个稍后到达的同 target nonce request 必须在同一线性化点把 tombstone 绑定到该 request 的完整 canonical key 与 operation，并缓存 `cancelled-before-acceptance` final（`transportAccepted=true`、`businessAccepted=false`、`completed=true`、`retrySafe=false`、`targetMayHaveMutated=false`、error code `CANCELLED_BEFORE_ACCEPTANCE`）。之后同 nonce 同语义只能 replay 该 final，不同语义必须得到 `NONCE_SEMANTIC_CONFLICT`；任何后到请求都不得执行、attach 到新工作或移除 tombstone。若 execution ledger 已满而不能原子安装 tombstone，cancel 必须以 `BROKER_REQUEST_LEDGER_FULL` 的 `cancel-rejected` 在业务接受前失败，绝不能返回未受 tombstone 支撑的 `unknown-request`。若 target 已 transport accepted 但尚未 business accepted，System 在同一线性化点把其 ledger 推进为同一 `cancelled-before-acceptance` final，cancel receipt 可为 `cancelled`。只有 target 已 business accepted 时，cancel 才进入 `cancellation-requested`，随后只能推进为 `cancelled` 或 `too-late`。

`unknown-request`、`cancelled` 与 `too-late` 都是不可倒退的 cancel 终态；`cancellation-requested` 只能单向推进到后两者。重复读取同一 revision 必须得到同一状态。`CancellationRequested` 不等于 `cancelled`，receipt 不是原 operation 的 final，连接断开也不隐式取消。deadline 在 business acceptance 前可权威终结为 `expired-before-acceptance`，其稳定错误码为 `REQUEST_EXPIRED`；open 的 Module registry 已满必须在 worker/Job 创建前以 `BROWSER_SESSION_REGISTRY_FULL` 的 `rejected` 终结；引用的 session 不属于当前 live Module 代际必须在任何领域状态变化前以 `STALE_SESSION` 的 `rejected` 终结；wait/query 引用的 page 不属于该 session 当前导航代际时必须同样在接受前以 `STALE_PAGE` 的 `rejected` 终结。这些领域预检失败都固定为 revision 0、`businessAccepted=false`，不得伪装为参数、ledger 容量或 accepted 后执行失败；business acceptance 后，如果尚未取得可信 final，则 Command 必须终结为 `unknown`，不得用 cancel 或断线证明未执行。

client 在本机单调时钟上拥有一次请求的绝对总 deadline，覆盖 connect、双向 peer authentication、`broker-ready`、确认、opaque target 解析、Module 调用、worker/Job/stdio 和结果聚合。client 只在 ready 验证后、发送完整 request 时计算 `remainingTimeoutMs`；若剩余值不在 `1..=30000`，不得发送 request。System 收到完整 frame 后用本机单调时钟建立首次绝对 deadline；同 epoch、同 request nonce 的 ledger 固定该首次 deadline，重送只能取 `min(既有绝对 deadline, 当前时刻 + 新 remainingTimeoutMs)`，因此只能维持或缩短，绝不能延长、重置或反向覆盖认证前时间。任何内部阶段、重试或重连都不得重置该 deadline。schema 的 `semanticFingerprint` 形状只建立可互操作的字段边界；Rust 同源实现计算 canonical fingerprint 并拒绝不匹配值。

只允许以下九项操作，且后续页面操作始终使用由同一 Module 生命周期签发的 opaque identity：

- Command：`open`、`close`、`navigate`、`click`、`type`。
- Query：`session.inspect`、`wait`、`query`、`screenshot`。
- `open` 只签发新的 `s2:bs`，不接受 URL 或其他浏览器启动参数。
- `close` 只接受仍 live 的 `s2:bs`，成功后 Module 逆序释放 worker、stdio、Job 和整个 registry。
- `session.inspect` 只接受 canonical `s2:bs`，不接受 `confirmed`；Module 在同一 live registry 中只读判定会话，存活时必须与其他 Query 一样先进入 execution ledger 并返回 `accepted(revision=0)`，再以 `completed(revision=1)` 仅返回 `{sessionId,live:true}`；不存活时在 business acceptance 前返回 `STALE_SESSION`，不允许绕过 ledger 直接返回单帧 completed，也不泄漏 worker、Job、stdio、provider 或任何未来公开 capability 定义。
- 任一页面 operation 的生产 broker allowlist、dispatcher、System 和 Windows authenticated client 必须同时接线；不得只让 strict parser 接受后静默断开连接。wait/query/screenshot 在 accepted 前验证 session 与当前 page 代际，旧 page 返回 `STALE_PAGE`；click/type 还必须验证当前 element，旧 element 返回 `STALE_ELEMENT`。任何 Query 的 accepted、unknown 或本地投递后未知投影都必须保持 `targetMayHaveMutated=false`。
- `navigate` 只接受 live 的同会话 `s2:bs` 与有界 `http://`/`https://` URL；URL 必须有非空 authority，并拒绝 userinfo/`@`、反斜杠、空白与控制字符。Module 对当前页面完成导航，成功后返回轮换的 `s2:bp`。`wait`、`query`、`screenshot` 只接受 live 的同会话 `s2:bs` 与 `s2:bp`；`click`、`type` 还只接受当前代际 live `s2:be`。
- selector 与 wait condition 是 provider-neutral 的封闭语义，不接受 CSS、XPath、脚本、CDP method/params 或原生 node/target/execution-context ID。文本和所有计数、尺寸、结果均有 schema 上限；`type.text` 除 schema 的 16384 字符保守上限外，Rust 必须验证 UTF-8 字节不超过 16384。
- wait/query/screenshot completed final 都回显原 request 的当前公开 `pageId` 与 Module 报告的正 `navigationGeneration`；click/type 还回显原 request 的 `elementId`，供公开 App facade 忠实投影而不另发 freshness Query。query `matches` 最多返回 100 项，但 `matchCount` 是 `0..=u32::MAX` 的总命中数。Rust 同源验证必须保证 `matchCount >= matches.len()`，且 `truncated` 当且仅当 `matchCount > matches.len()`；JSON Schema 只冻结字段形状与各自边界，不能表达这些跨字段关系。
- 所有 Command（`open`、`close`、`navigate`、`click`、`type`）都是 confirmation-first，固定要求 `confirmed=true`；所有 Query（`session.inspect`、`wait`、`query`、`screenshot`）不得携带 `confirmed`。对 Command，运行时必须在任何 target、payload 或 domain 校验、registry 查找、worker 创建或浏览器 dispatch 前拒绝未确认请求；未确认只能以 `rejected` 终结，绝不泄漏 target 存在性。

## 接受、完成和恢复

领域 request 的响应只可能是 `accepted` 或 `final`；`broker-ready`、`cancel-receipt` 与 `cancel-rejected` 是控制帧，不是领域 operation 响应。accepted/final 分别表达：

- `transportAccepted`：broker 已完整收取、认证并关联此帧；
- `businessAccepted`：Module 已允许此 operation 越过业务接受点；
- `completed`：具有可信 final，而不是仅观察到接收或中断；
- `OutcomeUnknown`：在 business acceptance 后丢失 final、断线、崩溃、deadline、取消或协议污染，不能证明操作没有发生。

`accepted` 固定为 transport/business 均 true、completed false；其 `targetMayHaveMutated` 对 Command 固定 true、对 Query 固定 false。broker wire `final` 的封闭 outcome 由 schema 固定：`rejected`、`expired-before-acceptance`、`cancelled-before-acceptance`、`completed`、`failed`、`cancelled`、`unknown`。`not-dispatched` 只允许 client 在本地投影：broker 未完整接收 frame 时无法可信回显未接收的 nonce、operation、epoch 或 revision，因此绝不能在 wire 发出 `not-dispatched`。`rejected` 或 `expired-before-acceptance` 可安全重试；`cancelled-before-acceptance` 虽未修改 target，但 tombstone 禁止同一用户意图/nonce 重试，故 `retrySafe=false`；其余 final 一律不可自动 retry。`cancelled` 表示已接受的协作取消获得权威终态：`businessAccepted=true`，但不证明原 operation 没有发生。`unknown` 固定具有 `outcomeUnknown=true`、`completed=false` 且 error code 为 `OUTCOME_UNKNOWN`；已接受的 Command 的断线、崩溃、deadline、取消或丢失可信 final 必须保守映射为这个结果，而不能伪造 `cancelled`。

所有 final 必须带 `targetMayHaveMutated`。Query 的值固定为 false；Command 在 business acceptance 后的 `completed`、`failed`、`cancelled` 或 `unknown` 固定为 true，只有确认未接受的终态才为 false。这是投影的安全证据，不表示 browser 状态可回滚。

成功 `data` 按 operation 封闭：open 仅返回 session identity，close 仅返回关闭事实，session.inspect 仅返回原 session identity 与 `live:true`，navigate 仅返回新的 page identity 与代际，wait 仅返回当前 page/代际与满足事实，query 仅返回当前 page/代际和有界 provider-neutral element projection，click/type 仅返回 request-bound page/element、正代际与动作事实，screenshot 仅返回 request-bound page、正代际和有界 PNG Base64/字节数/尺寸/摘要。任何非 completed final 的 `data` 均为 null，只能携带安全 error；type 不回显文本，三项操作都不回显 private registry、pipe、安全主体或浏览器实现细节。

## 生命周期不变量

1. Browser Session Module 是唯一能创建、读取、失效或释放 live `s2:bs`/`s2:bp`/`s2:be` 的边界；只有 `ComputerControlSystem` 能创建、拥有和调用 Module，client 不能制造或延长身份生命周期。
2. 成功 navigate 轮换 page identity 并使旧 page/element stale；close 越过 business acceptance 时立即使对应 session stale，Module 继续独占后台回收资源，未取得可信 final 时按 accepted 后 `unknown` 投影。worker/Job/Module 失信使受影响 session stale；其他 accepted mutation Command 丢失结果时，只有 System 无法证明 registry 一致性才使对应 session 或导航代际 stale。Query response 断线或 unknown 不改变 target，也不使 registry stale。
3. broker 物理宿主只创建、启动和关闭 `ComputerControlSystem`，不直接创建、拥有、托管或请求 Module；它不拥有独立 browser state，也不持久化或跨 System restart 恢复 Module registry。

每条 request/cancel/control 输入 frame 的 UTF-8 上限为 65536 字节；每条 broker response frame 的 UTF-8 上限为 16908288 字节（16 MiB + 128 KiB JSON envelope）。execution ledger 的 retained replay snapshot 总预算固定为 64 MiB；System 必须在 business acceptance 前按 operation 最大响应上限原子预留 entry 与 replay bytes，任一预算不足均返回 `BROKER_REQUEST_LEDGER_FULL`，不得接受后再丢弃 terminal payload。超限输入在 business acceptance 前失败闭合；超限输出不能截断成可信 final。System shutdown 先停止接收新业务，再在线性化点冻结 ledger：已 accepted 的 mutation Command 若不能取得可信 final，则推进为 `unknown` 并按 registry 一致性规则失效相应生命周期；Query 推进为 target 不变的 `unknown`；尚未 business accepted 的 entry 只能形成可信 pre-acceptance final。ledger/tombstone 必须保留到 epoch 终止，不得借 shutdown、drain 或容量压力静默重派、回退 revision 或淘汰 nonce。
4. 不定义自动重试、事务、回滚、Cookie/credential 注入、用户 profile、任意浏览器协议或前台输入回退。

## Schema 与实现范围

`browser-session-broker-v1.schema.json` 使用 JSON Schema draft 2020-12，所有协议对象均以 `additionalProperties:false` 或组合对象的 `unevaluatedProperties:false` 封闭。Rust parser、fingerprint 计算、状态机和契约测试必须与该 schema 同源；schema 不能替代 OS peer 认证、UTF-8 字节计数、canonical 语义比较、deadline 计时或 stale registry 验证。

2026-08-16，Vikunja #2305 已补齐 click/type 的 confirmed-only strict builder 和 screenshot Query builder，三者的 canonical fingerprint 逐字绑定完整业务语义并由同源 parser 复核。completed success 改为 request-bound page/element/正代际投影；screenshot 同时验证并返回 `pngBytes`，worker stdout 与 broker response/replay 都为 16 MiB Base64 另预留 128 KiB envelope。本批只闭合 Protocol/Wire Component，不接通尚未实现的 System dispatcher，也不表示公开 capability 已可用。

2026-08-16，Vikunja #2306 已接通 click/type/screenshot 的生产 broker allowlist、串行 dispatcher、ComputerControlSystem、BrowserSessionModule 与 Windows authenticated client。Module 的纯预检固定 `STALE_SESSION`→`STALE_PAGE`→`STALE_ELEMENT` 优先级，stale element 在 revision 0 business rejection 前不派发 worker；accepted 后 click/type 只形成 mutation=true 的可信完成或保守未知结果，screenshot 始终保持 mutation=false。仓库物理纵切以真实 production broker、认证 launcher、production worker 和 CDP runtime fixture 完成 open→navigate→query→click→type→screenshot→close，验证当前 page/element/正代际、有界 PNG、宿主前景不变、私有字段零泄漏与资源回收。本批仍不登记公开 Registry、Policy、App provider 或 launcher capability，不能据此关闭 #2042、#2029 或 #2007。
