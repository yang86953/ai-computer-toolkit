# Host session identity compatibility

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

C++ application inventory uses one opaque `s2:h:*` target for the current Windows login
host. Its private identity input is the decimal Windows session ID returned for the current
process, followed by `:`, followed by the UTF-8 current user name. Query failures retain the
C++ sentinels `0` and an empty user name before hashing.

Rust `HostProvider` uses the same two read-only public Win32 queries and the same byte layout.
It recomputes the target for every session resolution, accepts only the current exact target,
and rejects legacy `s1:c1:*` and stale `s2:h:*` IDs. Public JSON contains only the canonical
opaque fingerprint; it never contains the Windows session ID, user name, process ID, token,
SID, or any other native identity component.

This identity migration does not certify a new launch path. Rust `application.open@1` keeps its
existing confirmation, fixed application ID allowlist, and compatibility execution boundary.
The C++ exact installed-application candidate continues to require a discovered `s2:a:*` target
as specified by `application-launch-migration.md`.

`tools/Test-DiscoveryEquivalence.ps1` requires Rust's one host session to equal the C++
`hostTargetId` byte-for-byte, verifies Rust inspect re-resolution and stale rejection, and checks
that neither result leaks the private host identity inputs.
