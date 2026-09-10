# Structured image save/export C++ candidate

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`artifact.save@1` and `image.export@1` are implemented as closed C++
candidates for the existing structured-image provider.

The provider is attach-only. C++ resolves the fixed internal
`Photoshop.Application` ProgID through `GetActiveObject`; it never starts the
application and public input cannot select a ProgID, COM member, script,
action, menu item or native document ID. Read-only status and document-session
discovery run in `ai-computer-toolkit-structured-image-worker.exe`, bounded by
a Windows Job, a 5-second deadline and cancellation cleanup. The worker emits
only opaque `s2:a:*` and `s2:d:*` targets and explicitly reports whether the
foreground changed.

Rust and C++ derive the application target from the fixed
`structured-image-editor` provider key, process ID and process creation
FILETIME. They derive each document target from that same process generation,
the native document ID, current name and current source path. A process restart,
document rename or source-path change therefore invalidates the previous target.
Resolution always re-enumerates the same complete inventory shape and rejects
legacy `s1`, wrong-kind, stale and ambiguous targets.

The process ID, creation FILETIME, native document ID, source path and fixed COM
ProgID remain private provider facts. `sessions app` and `inspect app` may expose
the document name and save state but never its source path. A successful
save/export result may echo the caller-supplied output path because that path is
an artifact result, not a discovered provider identity.

Write candidates are synchronous because the provider's COM call cannot be
safely cancelled. Reporting a timeout while a detached write continues would
violate the operation outcome contract. Confirmation is checked before target
resolution. The module then re-enumerates the exact current document, validates
an absolute existing-parent `.psd` or `.png` path, and refuses an existing
output unless overwrite is explicit. Only fixed internal save/export scripts
can reach COM.

After dispatch, provider failure or failed output verification is treated as
an unknown outcome with `retrySafe=false`. Save additionally re-reads the
provider document state and verifies that the document is saved at the
requested canonical path. Export verifies a non-empty PNG output.

The installed provider is currently not running. Automated evidence therefore
covers pure identity goldens, Rust/C++ session-set equivalence for the observed
inventory, attach-only observation, worker isolation, confirmation ordering,
exact target rejection, path and overwrite policy, native-identity
non-disclosure, and zero user-document writes. Real document output equivalence
still requires an explicitly authorized, tool-owned provider document.
