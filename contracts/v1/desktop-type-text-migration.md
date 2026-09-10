# Background desktop type-text C++ migration

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`desktop.type-text` now has a certified C++ route for exact `s2:c:*` standard
Edit control targets.

The migrated route intentionally narrows the legacy window-oriented contract.
Callers discover the unique standard Edit control first and pass its opaque
control session. Confirmation is checked before target resolution. The
existing `StandardEditModule` then re-enumerates the exact control, verifies
same-session process metadata and integrity, sends only the fixed system
`WM_SETTEXT` message with `SendMessageTimeoutW`, and performs bounded readback.
No arbitrary message, pointer protocol, HWND, PID or class name is accepted or
returned.

A timeout is an unknown mutation outcome because the target may process the
message after the caller's deadline. Compatibility maps it to
`TARGET_HUNG_OR_UNAVAILABLE` with `retrySafe=false`. Success reports
`background-wm-settext`, verified readback and unchanged foreground.

The migration launcher selects C++ only for exact `s2:c:*` targets. Legacy
`s1:*`, native window targets and `s2:w:*` requests that may need foreground
fallback remain on Rust. The closed foreground Unicode input candidate is not
implicitly authorized by this background migration.

The same toolkit-owned standard Edit fixture used for `ui.text.input@1`
validated the desktop compatibility shape, UTF-8 content, readback, timeout
semantics, foreground invariance and zero user-application writes.
