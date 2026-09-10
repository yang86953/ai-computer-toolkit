# Rust 单文件原子输出内部契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该契约约束 Rust Artifact 边界的单文件 staging 与提交，不是新的公开 capability，
也不替代多产物 recording transaction 的 staging、验证与回滚责任。

## 边界与所有权

- `StagedFile` Component 以目标路径为输入，在目标同目录取得唯一 staging 生命周期；
- staging 名称、Win32 类型、原生错误和临时路径均为 Component 私有实现；
- writer 只借用 staging 路径，不能取得清理或提交所有权；
- reservation、未提交失败和取消由 `StagedFile` 的 RAII 所有者清理；
- 上层 Adapter/Module 把封闭 Component 错误映射为领域错误。

## 提交流程

1. 目标必须有文件名，父目录必须存在且不是符号链接；
2. 以目标同目录的唯一 `.part` 名称执行 `create_new`，碰撞时有界重试；
3. 固定 writer 完成并关闭自身句柄，上层验证候选格式、大小和领域不变量；
4. Component 重新打开 staged file 并执行 `sync_all`，随后关闭句柄；
5. `overwrite=false` 时目标已存在立即返回 `TargetExists`；
6. `overwrite=true` 只允许替换非符号链接普通文件；
7. 使用 `MoveFileExW` + `MOVEFILE_WRITE_THROUGH` 在同目录原子提交，只有
   `overwrite=true` 才附加 `MOVEFILE_REPLACE_EXISTING`；
8. 成功提交后 Component 放弃 staging 所有权；任意失败保留既有目标，并清理 staging。

Linux 组合根使用同职责 `atomic_file_linux.rs`：`overwrite=false` 以同目录 hard-link
原子建立最终名称后删除 staging，`overwrite=true` 仅对已确认的真实普通文件执行同目录
rename；两条成功路径均 `sync_all` staged file，并 best-effort 同步父目录。平台实现差异
不得改变覆盖确认、既有目标保留、RAII 清理和不泄漏 staging 路径的公共语义。

## 原子边界与失败语义

- 单文件 `FactEstablished` 是 write-through rename 成功返回；此前只有私有候选；
- 未确认覆盖和提交竞态返回 `TargetExists`，不得预先删除目标；
- staging 创建、丢失、同步或提交失败均不建立公开结果；
- Component 不保证多个文件组成事务；MP4 与 analysis bundle 的整体一致性仍由
  Recording Module/Workflow 的 staging、验证、rollback 与 outcome-unknown 契约负责。

fixture 必须覆盖首次提交、未确认覆盖保留原件、确认替换、CREATE_NEW 名称隔离、
提交前失败保留原件和 `.part` 残留为零。
