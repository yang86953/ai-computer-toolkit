# 固定参数外部进程策略 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

契约名：`act/fixed-external-process-policy/v1`。

## 所有权与边界

- Browser Screenshot Module 拥有 Chromium 一次性隔离截图输入。Recording Module 已改用
  项目自有 Media Foundation Component，不再属于外部进程策略。`ComputerControlSystem`
  只协调确认、精确目标和 provider。
- Adapter/worker 只把 Module 已认证的强类型动态槽嵌入固定 argv 模板；公共接口不得
  暴露 shell command、argv、flag、codec、filter、script、executable 或 profile。
- URL、输出路径与 viewport 是版本化有界数据，不是任意命令行入口。

## Chromium 模板

公开输入只允许 `target.url`，以及 `path`、`width`、`height`、`timeoutMs`、`overwrite`。
URL 必须不超过 8192 字节并使用 `http`、`https` 或 `file` scheme。Rust Browser
Screenshot Module 和 `act/browser-screenshot-worker/v1` 都必须拒绝未知字段。

worker 自行构造 `--headless=new`、隔离 profile、固定 screenshot、viewport 和隐私保护
flag；调用方不能提供额外 Chromium flag。所有值作为独立 argv 或经过 Windows argv
引用规则编码，不经过 shell。

## 错误与验证

- confirmation-first 保持优先；确认后，任何未知字段在 runtime 发现和浏览器进程启动前返回
  `INVALID_ARGUMENT`。
- `tests/fixed_external_process_policy.rs` 核对版本化字段集与 Rust 生产源码门禁；
  Rust 单元测试动态覆盖 Chromium flag/argv 拒绝。
- `tests/browser_screenshot.rs` 使用仓库自有 HTML/浏览器 fixture 覆盖 Rust 正式路由、
  launcher、覆盖拒绝与 timeout 清理；C++ browser 路线已废弃且不再参与门禁。
