# Legacy catalog / describe 兼容目录

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`legacy-public-catalog-v1.json` 是 C++ 发布产物的一部分，不需要 Cargo 或 Rust
二进制即可支持 `catalog [app]` 与 `describe <app> [operation]`。它固化迁移基线
的 9 个 app 与 21 个 operation descriptor。

`catalog` 保持 Rust 公开 JSON 的顶层 `ok/policy/apps` 形状；无参数返回全部 9 个
app，精确 app ID 返回单项，未知 app 返回 `INVALID_ARGUMENT`。C++ 迁移期的内部
能力状态使用独立 `capabilities [surface]`，不得占用或改变旧 `catalog` 语义。

`describe` 保持 Rust 公开 JSON 的顶层 `descriptor/ok` 和完整 descriptor。C++
目录状态由 `capabilities descriptor <app> [operation]` 独立返回；app 状态增加
`cppDirectoryStatus: compatibility-descriptor`。尚未迁移的 operation 状态为：

- `cppStatus: rust-compatibility-only`；
- `cppExecutionEnabled: false`。

已逐 capability 通过等价门禁的 operation 可报告
`cppStatus: available-confirmed[-opaque-target]` 与
`cppExecutionEnabled:true`。阶段 5 的 desktop/app screenshot、browser screenshot 与
desktop/app record 已退出 C++ 执行目录，固定报告 `cppStatus: retired-rust-primary` 与
`cppExecutionEnabled:false`；历史 C++ 源码仅待阶段 8 / #649 统一删除。其余仍在迁移清单中的
operation 不得因本目录存在而获得生产执行授权。

目录只描述兼容请求形状，不授权执行。每个 operation 后续通过 capability 等价
门禁时，必须从单一权威 capability catalog 更新状态；不得仅修改本文件就开放路由。

构建会把 JSON companion 复制到主 executable 同目录。`catalog` 与 `describe`
遇到缺失、版本错误或格式错误必须 fail closed 为 `OPERATION_FAILED`。
