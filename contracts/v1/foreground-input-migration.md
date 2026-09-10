# Foreground key input Rust production route

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.input.key@1` 已由 Rust `app.apply` 生产入口直接实现；`desktop.press-key` 只保留旧 CLI
兼容，不再是统一 capability 的执行实现。生产 launcher 没有 C++ 路由或静默回退。

正式路线固定执行以下顺序：

1. 逐操作确认；
2. 显式前景同意和 strict isolation 拒绝；
3. `key-input-v1` 全量解析、状态配平和工作预算；
4. 私有 Windows 键表完整性验证；
5. canonical `s2:w:*` 精确目标 assessment 与实时重解析；
6. 静态进程元数据和完整性门禁；
7. 有界恢复、前景取得和逐效果前景复核；
8. 单事件 `SendInput`、请求内逆序安全释放和不可重试结果。

完整键集覆盖字母数字、F1-F24、导航/编辑键、数字区、左右 Win/Ctrl/Alt/Shift、主键区
标点位置键、锁定/系统键和常用媒体键。正式步骤支持 `key` press/down/up、按顺序按下并
逆序释放的 `chord`、有界长按/重复及 Unicode `text`。公开 JSON 不接受或返回 virtual
key、scan code、Windows 标志或 native handle。

显式 down/up 只能在同一请求内配平；取消、deadline、前景变化和中途失败不会阻止对工具
持有键的 best-effort 释放。恢复、激活或 dispatch 开始后的不确定结果固定为
`OUTCOME_UNKNOWN`、`retrySafe=false` 和 `automaticRetryProhibited=true`。Unicode 单元在
私有 Adapter 内成对调度，释放未确认时立即补偿并保留风险证据。

纯回归覆盖 parser、完整键表、确认/隔离/精确目标顺序、schema 与错误白名单。工具自有
窗口动态夹具通过正式 System 路由验证快捷键、长按、重复、Unicode、显式 down/up、取消、
超时和后续恢复；显式命令为
`cargo test --lib keyboard_input::dynamic_tests -- --ignored --test-threads=1`。真实视觉和交互
结论仍由 `yang86` 在 GC-VA-001 中独占，不由自动化证据代替。
