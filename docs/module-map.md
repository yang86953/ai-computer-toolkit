# 模块地图

按子系统说明 `src/`、`tests/`、`contracts/` 的组织方式，供定位实现与测试时导航。
本文件是导航说明，不是架构规范原文。项目声明的规范为 **SMC v2.0**
（`Cargo.toml` 的 `[package.metadata.smc]`，`spec_version = "2.0"`、
`spec_revision = 64f55bc…`），规范检出见其中的 `spec_repository`。

## 顶层目录

| 路径 | 内容 |
| --- | --- |
| `src/` | Rust 实现：库入口、平台适配、MCP 服务面、各子系统 worker 与 broker |
| `tests/` | 集成测试，按子系统分组；每个 `.rs` 是一个独立测试目标 |
| `contracts/` | 版本化 JSON Schema：`v1/`–`v4/`、`internal/`、`compat/`、`release/` |
| `cpp/` | Windows 原生主机组件（`windows-workers` feature）：`include/`、`src/`、`tests/` |
| `docs/` | 接口与实现说明 |
| `tools/windows/` | Windows 侧打包、安装与回归脚本（PowerShell） |
| `tools/benchmarks/` | 传输与媒体测量脚本（Python） |
| `skills/computer-control/` | 供 agent 使用的 computer-control 技能与参考 |
| `spikes/` | 探索性验证（语言评估、Portal/EIS、WGC-C++），不参与发布路径 |

## `src/`：入口与平台选择

- `lib.rs` 声明库模块，`main.rs` 是 CLI 入口。
- 平台差异用**同名模块的按平台文件**表达，而不是把 `cfg` 散进实现：

  ```rust
  #[cfg_attr(not(target_os = "windows"), path = "cli_linux.rs")]
  pub mod cli;
  ```

  六个模块各有 Linux 特化文件：`adapters`、`cli`、`policy`、`service`、`components`、
  `modules`（对应 `adapters_linux.rs`、`cli_linux.rs`、`policy_linux.rs`、
  `service_linux.rs`、`components_linux.rs`、`modules_linux.rs`）。
- 其余顶层模块按 `#[cfg(target_os = …)]` 门控，例如 `capture_worker` 仅 Windows、
  `linux_release` 仅 Linux。
- 子模块普遍以 `#[path = "…"]` 显式挂载，所以文件**平铺在 `src/` 顶层**而不收进同名
  目录。移动这些文件时必须同步对应的 `#[path]`。

## `src/`：子系统归属

| 子系统 | 顶层模块 |
| --- | --- |
| 核心与装配 | `domain`、`components/`、`modules/`、`adapters/`、`capabilities`、`catalog`、`methods`、`policy`、`task_control` |
| MCP 服务面 | `mcp/`（stdio 服务、工具目录、桌面会话映射、broker 客户端） |
| CLI 与宿主服务 | `cli`、`cli_error`、`cli_host`、`service`、`service_error` |
| Linux 桌面会话 | `desktop_session_broker`、`desktop_session_interaction`、`desktop_session_request` |
| 浏览器会话 | `browser_worker`、`browser_session_broker`（含 `_dispatcher`、`_response_transport`）、`browser_session_worker`、`browser_session_*_fixture` |
| 捕获、录制与观察 | `capture_worker`、`recording`、`recording_worker`、`observation_worker`、`observation_worker_location` |
| 媒体（MPRIS / Linux 媒体） | `mpris_candidate`、`mpris_candidate_client`、`mpris_control_candidate_client`、`mpris_runtime_client`、`linux_media_control_worker`、`linux_media_observation_worker` |
| 交互、序列与长操作 | `interactive_command_worker`、`interactive_session_broker`、`sequence_step_worker`、`long_operation_broker`、`semantic_action_worker` |
| 访问树（AT-SPI） | `atspi_candidate_client`、`atspi_observation_worker` |
| Linux 发布 | `linux_release` |

`*_error` 与 `*_tests` 跟随父模块，例如 `cli_error.rs` 由 `cli.rs` 以
`#[path = "cli_error.rs"]` 挂载。

## `tests/`：按子系统分组

Cargo 只自动发现 `tests/*.rs` 与 `tests/<目录>/main.rs`；本仓库把测试移入分组目录后由
`Cargo.toml` 的 `[[test]]` 显式声明（100 条，与分组目录内的文件一一对应）。

| 分组 | 数量 | 内容 |
| --- | --- | --- |
| `linux/` | 45 | Portal/EIS 桌面会话、UIX agent 契约、MPRIS、Linux 发布 |
| `core/` | 17 | MCP stdio、能力元数据、指针与键盘、语义动作、进程终止 |
| `browser/` | 13 | CDP 会话、broker 传输、页面动作与读取 |
| `policy/` | 8 | 隔离策略、安全边界、错误语义、源码行数上限 |
| `sequence/` | 7 | 序列绑定、前置条件、工作流与模板 |
| `platform/` | 5 | 动态兼容性（自绘窗口、显示拓扑、独占全屏、原始输入） |
| `contracts/` | 5 | 契约信封、窗口生成、目标身份 |
| `support/` | 11 | 被各测试以 `#[path = "../support/…"]` 复用，**不是**测试目标 |

`tests/contracts/*.json` 与 `tests/fixtures/` 是运行时数据，经 `CARGO_MANIFEST_DIR`
定位，因此不受测试文件所在目录影响。

## `contracts/`

`v1/`–`v4/` 是按版本演进的公开契约（能力升级另立文件，不原地改语义）；`internal/`
放进程间协议；`compat/` 与 `release/` 放兼容与发布相关 schema。
`src/mcp/tools.rs` 通过 `include_str!` 直接读这些文件生成 MCP 工具 schema，
所以它们同时是**工具面与 broker 的同一权威来源**——改契约即改工具参数面。

## 相关文档

- `README.md`：构建、平台支持、MCP 工具表
- `docs/computer-control-mcp.md`：MCP 工具与操作序列
- `docs/desktop-interact-observe.md`：动作后观察
- `docs/desktop-frame-changes.md`、`docs/desktop-frame-subscription.md`：帧差分与订阅
