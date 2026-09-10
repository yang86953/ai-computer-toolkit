# Rust 输出覆盖门禁内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该契约约束 Rust 写入路径在进入 provider 或 writer 前的统一只读门禁。它不创建、
删除、移动或修改任何产物，也不替代 writer 在最终提交点的竞态复核。

## SMC 边界

- `output_guard` 是 Artifact 边界的窄 Component，只读取文件系统目标状态；
- Policy、Recording 与 Application Capability Module 决定公开错误、参数顺序和
  provider 调度，不把 `Metadata`、`ErrorKind` 或路径检查错误泄漏出领域边界；
- `overwrite=true` 只能表示调用方在基础 `--confirm` 之外提供的独立覆盖许可，
  Component 不推断、缓存或扩大该许可；
- Atomic File、PNG、browser 和 provider writer 仍须在提交点重复检查竞态。

## 单文件门禁

`guard_file_output(path, overwrite_confirmed)` 使用 `symlink_metadata`：

1. 目标缺失时允许首次输出；
2. 目标为非符号链接普通文件且 `overwrite_confirmed=false` 时返回
   `ConfirmationRequired`，不得修改原文件；
3. 同一普通文件只有在 `overwrite_confirmed=true` 时允许进入 writer；
4. 符号链接、目录或其他特殊目标返回 `InvalidTargetType`，覆盖许可不得放宽类型；
5. 除 `NotFound` 外的检查错误返回 `InspectionFailed` 并失败闭合。

## 分析目录门禁

`guard_directory_output(path, overwrite_confirmed)` 返回封闭状态：

- 缺失目录：`Missing`，由 Recording Module 继续验证父目录；
- 空的真实目录：`Empty`，无需覆盖许可；
- 非空真实目录：无许可返回 `ConfirmationRequired`，有许可返回
  `NonEmptyConfirmed`；
- 符号链接、普通文件或特殊目标返回 `InvalidTargetType`；目录枚举失败返回
  `InspectionFailed`。

`NonEmptyConfirmed` 只授权既有 recording 分析产物按其独立白名单规则处理，不授权
删除任意目录内容，也不证明 MP4 与 analysis bundle 已形成多产物原子事务。

## 当前消费者与错误映射

- System Policy：`desktop.screenshot` 与 `browser.screenshot`；
- Recording Module：主 MP4 与 analysis directory；
- structured-image provider：`artifact.save@1` PSD 与 `image.export@1` PNG。

`ConfirmationRequired` 统一映射为 `OVERWRITE_CONFIRMATION_REQUIRED`；目标类型错误
映射为 `INVALID_ARGUMENT`；无法可靠检查映射为公开 envelope 内的
`OPERATION_FAILED`。基础写确认仍先于目标、参数和覆盖检查返回
`CONFIRMATION_REQUIRED`。

## 验证

自有临时文件 fixture 覆盖缺失目标、未确认保留原件、确认普通文件、目录冒充文件、
检查失败闭合、缺失/空/非空分析目录、三个公开输出 surface，以及 PSD/PNG 共享门禁。
测试不启动 WGC、浏览器、Photoshop 或任何用户应用。
