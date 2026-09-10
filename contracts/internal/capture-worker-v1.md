# Capture worker 协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

协议名：`act/capture-worker/v1`。Rust companion 只认证
`window-frame-metadata` 与 `window-screenshot`：前者服务公开
`window.capture.frame.probe@1`，后者服务公开 `window.screenshot@1`。worker 不接受历史
C++ operation，也不承担浏览器截图或录制。

## Request

```json
{
  "contractVersion": "act/capture-worker/v1",
  "operation": "window-frame-metadata",
  "sessionId": "s2:w:<opaque>",
  "confirmed": true,
  "timeoutMs": 5000
}
```

截图请求把 `operation` 设为 `window-screenshot`，并额外携带父 Module 以 `CREATE_NEW`
独占的 `stagingPath`。worker 只向该普通文件写候选，不接收最终路径或覆盖许可；最终原子
提交仍由持有 `StagedFile` 的父 Module 完成。

- `sessionId` 必须是 canonical `s2:w:<16-hex>`；不接受 HWND、PID 或 provider ID。
- `confirmed` 必须为 JSON `true`，且 worker 在解析目标前独立核对。
- 元数据 `timeoutMs` 必须在 `1..30000`，截图必须在 `250..30000`；父进程 Job
  deadline 仍是最终边界。
- 未知字段、协议版本或 operation 必须失败闭合。

## 执行边界

主 Module 固定执行：

1. 确认优先门禁；
2. canonical opaque 形状验证；
3. 当前窗口快照唯一重解析；
4. 零帧 eligibility 预检；
5. 固定 sibling worker 的 Job 约束启动；
6. deadline、取消、stdout/stderr 和退出码核验；
7. 前景不变门禁；
8. 严格 worker envelope 投影。

worker 重复执行确认和当前窗口快照唯一重解析，防止调用 companion 绕过 facade。
元数据 operation 只读取 `ContentSize()` 后关闭 frame，禁止访问 `Surface()`、像素或
文件。截图 operation 在 4096px 单边上限内读取 RGBA、计算 FNV-1a 64 位摘要、编码
不超过 64 MiB 的 PNG，并验证 signature；只写父 Module 独占的 staging，不激活窗口、
发送输入或抑制系统隐私指示器。

进程 Component 必须在 `CREATE_SUSPENDED` 后先绑定 `KILL_ON_JOB_CLOSE` Job，再恢复
worker。timeout 或取消时终止并回收整个 Job 树。stdout 只能有一个 JSON envelope，
stderr 必须为空，输出受 64 KiB 上限约束。

## Worker 自有失败码

Capture Worker 协议入口自身直接产生的失败只从私有封闭
`CaptureWorkerErrorCode` 映射，失败 envelope 仍只包含稳定 `code` 与安全 `message`：

- `INVALID_ARGUMENT`：请求 JSON、字段组合、deadline、canonical 目标或 stdin 边界无效；
- `CONFIRMATION_REQUIRED`：缺少逐操作显式确认；
- `CAPABILITY_GAP`：协议版本或 operation 不在认证集合；
- `INVALID_OUTPUT_PATH`：父 Module 预留的 staging 不满足普通文件或 `.png` 契约；
- `SCREENSHOT_WRITE_FAILED`：PNG 候选大小、读取或 signature 验证失败。

上述五项只归 worker 协议边界所有。窗口枚举、opaque 唯一重解析、前景检查与 WGC
Adapter 产生的错误继续原样传播，再由父 Module 的既有白名单失败闭合。Vikunja #1979
只统一十八个直接错误构造点，不改变公开码、消息、空 details、退出码或资源生命周期。

## Success data

成功 data 只能包含：

- `frameWidth`、`frameHeight`：正整数；
- `deviceDriver`：`hardware` 或 `warp`；
- `frameSurfaceAccessed=false`；
- `pixelsPersisted=false`；
- `fileWritten=false`；
- `foregroundUnchanged=true`；
- `privacyIndicatorMayHaveAppeared=true`。

主 Module 只把这些封闭事实投影到公开 schema；任何额外字段、未知驱动分类、安全常量
漂移或退出码/envelope 不一致都返回 `WORKER_PROTOCOL_VIOLATION`。

截图成功 data 恰好包含 `frameWidth`、`frameHeight`、`pngBytes`、`deviceDriver`、
`pixelDigest`、`candidateWritten=true`、`cursorCaptured=false`、
`foregroundUnchanged=true` 与 `privacyIndicatorMayHaveAppeared=true`。它不得返回最终或
staging 路径。父 Module 复核实际 staging 长度和主进程前景后，才执行 durable 原子提交。

真实用户应用探针仍要求用户逐操作明确确认。自动门禁在没有该确认时只验证
confirmation-first、stale、协议、静态禁止项和工具自有测试范围。
