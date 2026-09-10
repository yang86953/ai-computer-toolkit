# Text document create compatibility v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`text.document.create@1` creates one new UTF-8 `text/plain` artifact and opens
that new artifact in the system Notepad. It never attaches to, reads, types
into, or changes an existing user document.

## Public routes

```text
sessions app
inspect app --target sessionId=s2:a:<opaque>
assess app --capability text.document.create@1 --target sessionId=s2:a:<opaque>
run app create --input <file> --confirm
run notepad open-and-write-text --arg text=<utf8> --confirm
```

The app route accepts:

```json
{
  "target": {"sessionId": "s2:a:<opaque>"},
  "args": {
    "capability": "text.document.create@1",
    "input": {"text": "content"}
  },
  "confirmed": true
}
```

## Mandatory order and boundaries

1. Missing confirmation returns `CONFIRMATION_REQUIRED` before artifact
   creation or application launch.
2. The facade requires the exact current `s2:a:*` creator session. Unknown
   targets return `TARGET_NOT_FOUND`; old `s1:*` targets return
   `TARGET_ID_MIGRATION_REQUIRED` from the C++ core and remain a launcher-only
   Rust fallback during migration.
3. Input must be valid UTF-8 and no larger than 1 MiB.
4. Before creating a file, the backend performs a read-only process snapshot.
   If Notepad is already running, execution returns
   `BACKGROUND_OPERATION_UNAVAILABLE` with reason
   `existing-application-session-attachment-not-certified`,
   `artifactCreated:false`, and `safeToRetryAutomatically:false`. Modern
   Notepad may otherwise absorb the file into a pre-existing single-instance
   process, which is outside the certified exact launch boundary. The launcher
   must not bypass this result through Rust.
5. The Windows backend creates a toolkit-named file in the system temporary
   directory with `CREATE_NEW`, writes and flushes all bytes, then reads the
   artifact back and requires byte-for-byte equality.
6. Only the fixed `%WINDIR%\System32\notepad.exe` runtime may be launched.
   The first window show request is `SW_SHOWNOACTIVATE`; keyboard, mouse,
   clipboard, UIA write patterns, injection, elevation, and arbitrary
   executables are forbidden.
7. The process starts suspended, enters a temporary Windows Job rollback
   boundary, and only then resumes. A foreground change returns
   `HOST_INTERFERENCE_DETECTED`; the whole toolkit-owned launch tree is
   terminated and its new artifact is removed. On success the Job is released
   without terminating Notepad.
8. The provider-neutral app result exposes artifact domain fields and
   `foreground.unchanged` only. It never exposes provider identity, executable
   path, native handle, or process ID. The legacy `notepad` result retains the
   launcher PID solely because it is part of the existing compatibility shape.

`assess` returns `confirmation-required`, realm `host-background`,
`requiresForegroundConsent:false`, `readOnly:false`, and never probes by
creating a file or launching Notepad.

## Equivalence evidence

`tools/Test-RustCppTextDocumentEquivalence.ps1` verifies one Rust and one C++
execution with separate toolkit-owned temporary artifacts: discovery fields,
confirmation, stale target, UTF-8 byte readback, foreground reporting, and
sanitized facade mapping all match. The test tracks the exact CLI descendant
tree, terminates only its own Notepad processes, removes only validated
toolkit-prefixed temporary files, and reports zero user documents modified.

The Rust provider derives the creator target from the same private identity
and FNV-1a `s2:a:*` algorithm as the C++ module. Stage 4A adds a Rust Text
Document Module and a narrow Windows launch adapter: confirmation and the
1 MiB limit precede runtime/process discovery, existing Notepad is refused
before artifact creation, `create_new` + `sync_all` + byte readback protects
the artifact, and a suspended Job-bound `SW_SHOWNOACTIVATE` launch is rolled
back on foreground interference. Both `run app create` and the legacy
`notepad.open-and-write-text` launcher route now select Rust. C++ remains
direct compatibility evidence and is not deleted or used as a silent fallback.
