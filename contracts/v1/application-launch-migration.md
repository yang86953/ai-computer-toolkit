# Exact installed-application launch migration

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

Rust is the production implementation for `application.open@1`. The host
session no longer publishes three static application IDs. `discover app`
merges bounded Registry, Shell AppsFolder, and the fixed current-user/common
Start Menu Programs sources. It marks only records with a private identity
from one of the two closed Shell sources as `available-confirmed`; assessment
and execution use that exact `s2:a:*` target. Start Menu identities bind the
Known Folder entry, stable file generation, display name, timestamps, and
size, while the path remains private. Every execution rebuilds the bounded
installed-application inventory, rejects zero or multiple matches, accepts an
empty input object only, and calls the narrow Shell launch component after the
normal confirmation gate. The public result contains only dispatch,
process-observed, and foreground boolean evidence.

The Rust launcher gate has verified 136 discovered applications, 120
separately launchable targets, and 69 targets carrying the fixed Start Menu
source. Exact inspect, `confirmation-required/host-foreground`, and
`CONFIRMATION_REQUIRED` before any dispatch all passed. Native identity leaks
and launches dispatched during that gate were both zero. One explicitly
confirmed real target is still required by the existing owner-only acceptance
task; agents do not choose or launch a user application for that gate.

## Historical C++ compatibility baseline

This section is retained only as migration history. C++ is retired and no
longer participates in current acceptance, routing, build, or fallback.

`application.open@1` and the secured migration shape for `desktop.launch` are
implemented as a closed C++ candidate.

The candidate does not accept an executable path, command line, AUMID, Shell
parsing name or arbitrary URI from public input. It requires an exact current
`s2:a:*` record produced by application discovery. Only records originating
from the public Windows AppsFolder and carrying an internal Shell item
identity are launchable.

The internal identity is never serialized. The Windows backend resolves it to
a PIDL and calls `ShellExecuteExW` with no caller arguments and no shell
command string. Confirmation is checked before discovery. A missing,
non-launchable or changed target fails before launch.

Foreground change is allowed and explicitly reported because a launched
application controls its own presentation. The result never exposes a process
ID, executable path, AUMID, PIDL or Shell identity.

The backend has launched a toolkit-owned, no-window executable through the
same PIDL path; it created the expected owned evidence file, did not change
foreground and left no process. The public route remains closed until one
actual AppsFolder-discovered exact target is validated with explicit
per-operation user confirmation. Legacy path-based `desktop.launch` continues
through Rust during this interval.
