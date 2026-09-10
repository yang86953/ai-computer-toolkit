# Opaque target validity and collision policy

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`s2:<kind>:<fnv1a-64>` is a versioned, provider-neutral routing fingerprint. It
is not an authorization token, a capability grant, a proof of possession or a
cryptographic identity. Confirmation, foreground consent, permission and
capability assessment remain independent gates.

Every inspect, assess or execute operation must rebuild the candidate target
set from a current inventory and compare the caller's canonical `s2` value with
IDs regenerated from current private identity facts. Resolution is fail closed:

- zero matches means the target is stale, unavailable or belongs to another
  provider;
- one match is the only resolvable target;
- two or more matches mean `AMBIGUOUS_TARGET`, including an FNV collision or
  overlapping providers; an implementation must never select the first match.

Lifecycle-bound identities include the strongest available generation fact. Windows processes
bind the process creation FILETIME; windows and controls additionally bind their current native
token, but public Windows facts do not prove a new same-process window when the exact token is recycled.
A structured-image application and its documents bind the same process generation. Provider facts
such as HWND, PID, native document ID and source path never cross the public JSON
boundary. Fixed virtual providers instead bind a stable provider key and must
recheck their current availability before use.

独立交互会话 `s2:i:*` 同时绑定 worker 私有的系统 session 代际与一次授权代际。重新授权、
session 注销后重建、endpoint lease 更换或认证事实变化都会使旧目标 stale；每次发现和执行
仍须重新验证 active、unlocked、普通 input desktop、与 host session 不同及当前授权。
`s2:i` 只是路由指纹，不是 endpoint secret、访问令牌或 OS 对等身份证明。

长操作句柄 `s2:o:*` 不是可重新发现的 provider 资源，而是同一登录会话内固定 broker
registry 的 opaque 索引。其私有身份同时绑定 broker 会话代际与系统随机 nonce；句柄不是
权限凭据，不得跨会话搜索或解析。终态记录按 `long-operation.md` 固定保留 86,400 秒，
到期、broker 代际变化或 registry 零/多命中均返回 `OPERATION_NOT_FOUND`。该显式保留期
只适用于任务记录，不改变窗口、进程、控件等 provider 目标的使用时重发现规则。

Provider resource targets deliberately have no wall-clock `expiresAt` field or fixed TTL. A target
can disappear immediately after discovery, while an unchanged target can remain
valid across many short-lived CLI processes. Its validity therefore expires
when the private identity disappears, changes or enters a new process generation,
and that condition is evaluated at the time of use. Caching a native handle or
accepting a fingerprint without rediscovery violates this contract.

窗口身份强度与完全相同 token 回收停止线由 `window-target-identity.md` 和
`window-target-identity.schema.json` 唯一冻结。重新发现、confirmation、class/style 核对或
`IsWindow` 只能缩小竞态窗口，不能提升为持久窗口创建代际。

Accessibility tree 的 `s2:e:*` 例外地只标识单次 inspection snapshot 中的节点事实，
不是可跨调用重新解析的执行目标。结果必须同时返回
`identityFreshness: inspection-snapshot`；任何后续读取都从父 `s2:w:*` 重新执行有界
检查，不得缓存 UIA element 或把 `s2:e:*` 送入写路径。

Rust uses the shared opaque-target matcher for provider-local zero/unique/
duplicate classification and separately rejects duplicate provider matches in
the app facade. The C++ main process and isolated observation, capture and
recording worker sources use their own platform-neutral shared matcher for
application, process, window, Standard Edit and structured-image document target
resolution. C++ media workers use explicit zero/one/many counting with the same
error mapping. No C++ slice passes migration acceptance until compiler,
candidate and compatibility gates verify the source changes.

Rust contract and integration tests are the authoritative regression gates for
resolver coverage, Missing/Unique/Ambiguous behavior, and media explicit
counting. The former C++ static and `clang++` matcher scripts are retirement
assets and are not part of the current acceptance path.
