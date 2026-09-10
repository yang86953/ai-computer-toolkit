# ai-computer-toolkit

Capability-first computer-control toolkit for AI agents.

The toolkit drives a real desktop — screenshots, keyboard and pointer — and
exposes two control surfaces:

- **MCP (Model Context Protocol) stdio server** — started by launching the
  binary with no arguments. This is the surface agents normally use.
- **A versioned CLI** — `discover`, `sessions`, `inspect`, `assess`, `run`,
  `capabilities`, on both Linux and Windows.

UIX is an external dependency pinned in `Cargo.toml` and `Cargo.lock`.
Its source and client-specific extensions are maintained outside this repository.

## Build

```bash
cargo build --locked --bin ai-computer-toolkit
```

Rust 1.96 (see `rust-toolchain.toml`; rustup installs it automatically). Linux and
Windows are both supported.

All dependencies come from crates.io and the public UIX Git repository, so no
credentials or private registry configuration are required.

Requirements by host:

| Host | Requirement |
| --- | --- |
| Linux | PipeWire development files for linking (`libpipewire-0.3-dev` or equivalent, discovered through pkg-config). The resulting binary links `libpipewire-0.3.so.0` at runtime |
| Windows native | MSVC toolchain |
| Windows cross-compiled from Linux | `mingw-w64` (`x86_64-w64-mingw32-gcc`) |

Cross-compiling the Windows binary from Linux:

```bash
cargo build --locked --release --target x86_64-pc-windows-gnu --bin ai-computer-toolkit
```

The Windows binary only imports system DLLs (UCRT requires Windows 10 or later);
no MinGW runtime DLLs are needed alongside it.

## Platform support

| Platform | Desktop route | Control surface |
| --- | --- | --- |
| Linux / Wayland | XDG Desktop Portal (capture) + EIS (input) | CLI + MCP |
| Windows | UIX display/cursor + WGC capture + existing Win32 input executor | CLI + MCP |

Neither route provides background isolation, and neither returns structured
accessibility trees as a substitute for verified desktop actions.

## MCP

```json
{
  "mcpServers": {
    "computer-control": {
      "command": "/absolute/path/to/ai-computer-toolkit"
    }
  }
}
```

Initialization and `tools/list` do not touch the desktop. A session is opened
only by an explicit `computer_connect`. Before connecting, the server checks
that the broker advertises `postInputObservation` and fails with
`BROKER_FEATURE_UNAVAILABLE` rather than sending input to an incompatible
build.

Supported protocol versions: `2024-11-05`, `2025-03-26`, `2025-06-18`.

### Tools

| Tool | Purpose |
| --- | --- |
| `computer_connect` | Open this client's own session; returns `sessionId`. |
| `computer_status` | Report this client's session state; does not connect. |
| `computer_observe` | Return a real PNG, `frameId` and dimensions. |
| `computer_interact` | Move/click in screenshot coordinates, or a small batch of keys/text. |
| `computer_keys` | Send a full key or shortcut such as `["left-shift", "f5"]`. |
| `computer_pointer` | Relative movement, scroll and complete drags. |
| `computer_disconnect` | Close the session, read back empty `sessions`, release the broker. |

Operate as `connect → observe → confirm the target in the image →
interact/keys/pointer → verify the returned image → disconnect`.

- `interact` coordinates are **observation-px**, taken from the returned image.
  `pointer` deltas are **relative-logical-px**; do not mix them.
- Input must reference the most recent `frameId`. Any input attempt invalidates
  the previous frame, including a failed one.
- `interact`'s `text` synthesizes ASCII keystrokes; it is not Unicode paste.
- Only one tool call runs per client; a concurrent call returns `BUSY`.

The authoritative schema for every tool is the one returned by `tools/list`.

### Consent

`confirmed`, `foregroundConsent` and `strictIsolation` record authorization the
user has already given. They are not a permission an agent can award itself.
`strictIsolation=true` is rejected rather than downgraded.

### Failure signals

| Signal | Meaning |
| --- | --- |
| `STALE_SESSION` | Not this client's session; connect first. |
| `STALE_FRAME` | Observe again; frames are never reused. |
| `CONSENT_REQUIRED` | Required authorization is absent. |
| `OUTCOME_UNKNOWN` | The effect may have occurred. Verify before acting. |
| `OBSERVATION_FAILED_AFTER_INPUT` | Input was dispatched but its screenshot failed. |
| `BUSY` | Another call is in flight. |

Input is never replayed automatically. Protocol and offline tests passing is not
the same as a verified desktop action; report those separately.

CLI and MCP both use `session-host desktop`, the same session module and input
contracts. Platform leases provide only the OS boundary: Portal/EIS/PipeWire on
Linux; UIX display/cursor, existing WGC capture and existing keyboard/pointer
executors on Windows.
Client-specific extensions are kept outside this package.

Windows support has compile and protocol coverage; native WGC/input acceptance
still requires an interactive Windows desktop. Frame subscriptions currently
remain Linux-only and are advertised by the broker feature flag.

## Layout

| Path | Contents |
| --- | --- |
| `src/mcp/` | MCP stdio server, tool catalog, desktop session mapping, broker client |
| `src/desktop_session_broker.rs` | Long-lived JSONL desktop session host |
| `src/cli.rs`, `src/service.rs` | Cross-platform CLI and service layer |
| `src/cli_linux.rs`, `src/service_linux.rs` | Linux specialisations |
| `src/modules/`, `src/components/` | Domain modules and shared components |
| `src/adapters/` | Platform adapters (Portal, EIS, PipeWire, procfs, UIA, …) |
| `contracts/` | Versioned JSON Schemas for the broker and worker protocols |
| `cpp/` | Native Windows host components (`windows-workers` feature) |
| `tests/` | Integration and contract tests |

## UIX dependency

Cargo fetches UIX and its workspace crates from the pinned revision of the public
repository <https://github.com/yang86953/uix-app> (MIT licensed). No credentials
or registry configuration are required. UIX platform changes belong in the UIX
repository and are consumed by updating the dependency revision. No framework
source is copied into this package.

## Tests

```bash
cargo test
```

The special-window compatibility schema and evidence snapshot tests run on
all platforms; they do not require a Windows desktop. Binary inventory tests
verify that the default build includes only the main executable and that
workers, fixtures and release tools remain behind their respective features.

These contract tests do not certify real desktop capture or input on either
platform.

## License

MIT. See `LICENSE`. External dependencies retain their own licenses.
