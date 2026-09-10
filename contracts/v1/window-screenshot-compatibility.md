# Window screenshot v1 兼容映射

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本文件定义正式 Rust `window.screenshot@1` 与 legacy
`desktop.screenshot` / `app.screenshot` 的兼容目标。Rust Module、隔离 worker、
PNG 管线、原子输出、secured mapper、实际像素事实和 opaque 路由均已实现；历史
C++ 实现只保留为完成记录，不再是生产路线或验证门禁。

## 兼容请求

```text
run desktop screenshot \
  --target sessionId=s2:w:<opaque> \
  --arg path=<existing-parent/output.png> \
  --confirm \
  [--arg overwrite=true] \
  [--arg timeoutMs=5000]
```

策略顺序固定：

1. 没有 `--confirm`：在 target、path 或 capture 前返回
   `CONFIRMATION_REQUIRED`；
2. 重新解析 opaque 精确窗口；stale 返回 `STALE_SESSION`；
3. timeout 必须为 250..30000；
4. `.png`、父目录和路径规范化；
5. 目标已存在但无 `--overwrite`：
   `OVERWRITE_CONFIRMATION_REQUIRED`；
6. read-only preflight；
7. Job worker 再次解析 target，capture、encode、atomic output；
8. 前台变化时结果不认证，返回 `HOST_INTERFERENCE_DETECTED`。

不得在后台失败后降级前台，不恢复最小化窗口，不隐藏系统捕获指示器，也不得请求
borderless capture access 或把 border-required 属性设为 `false`。共享策略见
`../internal/capture-privacy-indicator-policy-v1.md`。

## 新结果

权威数据形状见 `window-screenshot.schema.json`。输出只含 opaque target、规范化路径、
bytes、尺寸、driver、像素摘要和安全证据；不含 HWND/PID、临时路径、COM/WIC/D3D
对象或 provider 路由。

## 旧入口映射

旧 `run desktop screenshot` 成功结果必须继续提供：

- `ok: true`、`app: "desktop"`、`operation: "screenshot"`；
- `executionMode: "background-windows-graphics-capture"`；
- `captureMethod: "Windows.Graphics.Capture"`；
- `path`、`bytes`、`width`、`height`、`deviceDriver`；
- `cursorCaptured: false`；
- `systemCaptureIndicatorMayAppear: true`；
- 兼容 facade 的精确 target 与前台不变证据。

`app.screenshot` 继续通过统一 capability facade 返回 provider-neutral artifact
字段；兼容 adapter 不允许调用方选择 WGC/WIC 或原生目标。

错误映射在兼容入口保持旧语义：

| 正式 Rust Module | 旧 desktop 入口 |
| --- | --- |
| `CONFIRMATION_REQUIRED` | `CONFIRMATION_REQUIRED` |
| `STALE_SESSION` | `TARGET_NOT_FOUND` |
| `BACKGROUND_OPERATION_UNAVAILABLE` + hidden | `CAPTURE_TARGET_HIDDEN` |
| `BACKGROUND_OPERATION_UNAVAILABLE` + minimized | `CAPTURE_TARGET_MINIMIZED` |
| `TIMEOUT` | `CAPTURE_TIMEOUT` |
| `RESOURCE_LIMIT_EXCEEDED` / readback failure | `CAPTURE_READBACK_FAILED` |
| `OVERWRITE_CONFIRMATION_REQUIRED` | `OVERWRITE_CONFIRMATION_REQUIRED` |
| `SCREENSHOT_WRITE_FAILED` | `SCREENSHOT_WRITE_FAILED` |
| `HOST_INTERFERENCE_DETECTED` | `FOREGROUND_CHANGED` |

Rust 测试必须覆盖请求顺序、stdout 单 JSON、退出码、字段、文件摘要、stale、hidden、
minimized、timeout、无覆盖确认、确认替换、失败清理和前台不变。工具自有真实窗口的
Rust PNG 解码像素事实通过后即可认证该 capability，不等待 C++ 输出。

当前自动证据覆盖 Module 和 worker 的 confirmation-first、stale、timeout、path、
overwrite 门禁以及实际 self-owned Win32 窗口的 Rust WGC 输出。PNG 解码事实和
测试后清理均已验证；没有读取用户应用帧或写入用户路径。历史 C++ 像素对照只作为
已完成记录，不再是现行门禁。`window.screenshot@1` 曾在 C++ catalog 标为
`available-confirmed`，assessment 为 `confirmation-required`/
`isolated-worker`；desktop 与 app facade 的 `s2:w:*` 路由已开放。

纯 mapper fixture 已覆盖旧 desktop 成功字段与
stale/timeout/hidden/minimized/readback/foreground 错误映射；成功 target 只保留
`s2:w:*` 和 `application-window`，不恢复 HWND/PID。该证据不替代真实 PNG 像素
等价，也不授权放松确认、路径或原子输出门禁。

生产 launcher 已结束入口并存：canonical `s2:w:*` 固定路由 Rust；native
`window:<hwnd>`、`s1:*` 与其他非 canonical 目标失败闭合，不得恢复 HWND 或静默回退
C++。历史 C++ 目标迁移错误只作为兼容记录，不再决定生产路由。

app facade mapper 也已用同一合成结果覆盖：外层保留
`app/verb/capability/targetId/data/meta`，artifact data 不暴露 capture method、
device driver 或 provider 类型，前台只保留 `unchanged`。

Vikunja #1976 用共享 Window Capture Adapter 私有封闭的 `WindowCaptureErrorCode` 统一
WGC 可达性、目标资格/状态/尺寸、MTA 启动、D3D11 设备、首帧元数据、像素读回、超时、
尺寸变化、资源清理和 PNG 候选十六项错误定义。`window_capture.rs` 的三十六个直接
`AppControlError` 构造表达式归零，二十五个 `windows::core::Error` mapper 调用改为
封闭类型入参；上述确认、worker、原子输出、前景不变、opaque target 和隐私指示器
契约保持不变。
