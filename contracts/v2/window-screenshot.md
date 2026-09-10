# UIX exact-window application-surface screenshot v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.screenshot@2` is an explicit opt-in UIX application-surface capture. It does not
extend the frozen Windows `window.screenshot@1` contract and does not claim compositor-wide,
arbitrary third-party-window, desktop, cursor, or system-capture-indicator semantics.

The public target remains the opaque exact `s2:w` generation. The Adapter first resolves that
target from a complete authenticated UIX inventory, captures only the provider `window_id`, and
then calls `list_windows` again on the same authenticated connection. The PNG is discarded unless
the same `window_id` is still open with the original monotonic generation and re-derives the exact
requested opaque target.

The capability is published for a window only when hello declares the `screenshot` request and
boolean capability together with bounded `max_screenshot_bytes` and `max_response_bytes`. Normal
Agent messages retain the 4 MiB limit; only the single screenshot response uses the separately
negotiated response bound. Base64, process identity, token, endpoint, provider window ID and
generation never cross the Adapter boundary.

Input follows `window-screenshot-input.schema.json`. Explicit confirmation precedes input parsing,
target discovery, provider I/O and filesystem writes. A validated PNG of at most 32 MiB is written
through same-directory exclusive staging and atomic commit. Existing regular files require
`overwrite=true`; links, directories and special files are rejected. Provider or generation-check
failure commits no final output.

The Rust protocol fixture and contract tests certify routing, bounds, identity revalidation and
atomic output behavior. Owner desktop or visual acceptance is intentionally deferred and is not
claimed by this contract.
