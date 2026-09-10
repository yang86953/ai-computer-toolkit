# PNG 原子输出内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该契约约束 Rust `window.screenshot@1` 的最终文件提交。正式 Module 持有
`StagedFile` 生命周期，隔离 capture worker 只写候选，主进程完成前景复核后才提交。

## 输入

- UTF-8 输出路径，必须包含父目录且扩展名严格为 `.png`；
- worker 编码的 PNG 候选，signature 必须正确，总长不超过 64 MiB；
- `overwrite` 布尔值只能来自 facade 已完成的明确覆盖确认。

## 行为

1. 使用 Rust 路径类型和 Windows 文件 API，不通过 shell；
2. 父目录必须已存在，不由截图操作递归创建；
3. 已有目标且 `overwrite=false` 时返回
   `OVERWRITE_CONFIRMATION_REQUIRED`，不创建临时文件；
4. 在目标同目录以 `CREATE_NEW` 创建唯一 `.part.png`；
5. 完整写入后 `FlushFileBuffers` 并关闭句柄；
6. 使用 `MoveFileExW` + `MOVEFILE_WRITE_THROUGH` 原子提交；只有
   `overwrite=true` 才加 `MOVEFILE_REPLACE_EXISTING`；
7. 写入、flush 或 commit 失败时删除本次临时文件，不删除既有目标。

## 结果与错误

成功结果只含规范化路径、写入字节数和是否替换既有文件。公共边界不得出现 HANDLE、
Win32 error、临时路径或原生文件标识。

- 参数、扩展名、PNG signature、父目录无效：`INVALID_ARGUMENT`；
- 已有目标或提交竞态且未确认覆盖：`OVERWRITE_CONFIRMATION_REQUIRED`；
- 临时创建、写入、flush、关闭或原子提交失败：`SCREENSHOT_WRITE_FAILED`。

门禁必须只使用工具自有 no-activate 窗口，验证首次写入、未确认覆盖拒绝且原内容不变、
确认覆盖、desktop/app 正式 Rust 路由、无 `.part.png` 残留和测试目录清理。自动证据不
替代用户对系统捕获指示器的肉眼验收。

## Linux Portal 同会话单帧

`screen.capture@1` 复用相同的 64 MiB PNG、覆盖确认与单文件原子提交原则，但原始像素
来自既有 `s2:i` 对应的 Portal ScreenCast/PipeWire remote。Linux Module 在像素消费前
预留同目录 staging，Adapter 只返回完成上限校验的 owned PNG；PipeWire FD、node ID、
serial、mapping、buffer 指针与临时路径均不得进入响应。Linux `StagedFile` 使用同目录
`create_new` staging、`sync_all` 与 rename/hard-link 提交，并同步父目录作为掉电持久性
增强；`pixelDigest` 固定取逐行归一化后的 RGBA8 字节摘要，不取编码后 PNG 摘要。单帧失败、
超时或像素消费事实不明时清理 staging、保留既有目标并禁止自动重试；
这些机器证据不替代真实 Portal 授权后的 PNG 视觉验收。
