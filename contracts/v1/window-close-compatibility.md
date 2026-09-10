# Exact window close compatibility contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.close@1` 只接受 `sessions app|window` 当前返回的 `s2:w:<opaque>` 精确窗口。
公共 `app.close` payload 必须包含 capability、空或仅含 `timeoutMs` 的 input 和逐操作
确认；不得接受 HWND、PID、class name、窗口标题选择器或任意 message。

执行顺序固定为：

1. confirmation-first；
2. `timeoutMs` 1–30000 校验；
3. 重新枚举并唯一解析精确 opaque target；
4. 以窗口进程代际执行无主动写探针的静态完整性预检；
5. 若目标当前为前景窗口，在任何写入前返回
   `BACKGROUND_OPERATION_UNAVAILABLE`；
6. 只发送系统固定 `WM_CLOSE`；
7. 有界等待原窗口句柄失效；
8. 验证前台身份不变并返回 provider-neutral facade。

目标 stale 返回 `STALE_SESSION`。`PostMessageW` 被 UIPI 拒绝时返回
`PERMISSION_DENIED`。等待超时不能解释为“没有处理”：返回
`outcome:unknown`、`retrySafe:false`、`targetMayCloseLater:true`，不得自动重试。
消息后前台变化同样返回 outcome unknown，因为目标可能已关闭或显示应用自己的确认
界面，任何实现均不得尝试撤销、激活或发送输入。

正式 Rust Module 会在 `PostMessageW` 前以窗口进程 PID 与创建时间重新关联完整进程
inventory，并复用静态权限 Component。只有 metadata 可读且目标相对完整性为 same/lower
时才继续；higher 或明确 metadata 权限拒绝返回 `PERMISSION_DENIED`，unknown/unavailable
返回 `CAPABILITY_ASSESSMENT_UNAVAILABLE`。失败证据固定
`activeWriteProbePerformed:false`、`closeRequested:false`。

成功 facade 只返回 `window.close@1`、原 opaque target、closed 状态和
`foreground.unchanged`，并固定返回
`permissionPreflight:no-static-integrity-block-observed`、
`activeWriteProbePerformed:false`；不返回 provider、HWND、PID、class 或 executable。
迁移 launcher 的 `sessions`/`inspect`、`run app close` 与 `desktop.close` 已固定
切回 Rust 主实现；缺少 Rust 运行时时失败闭合，不回退 C++。旧 `s1:c2:*` 在 Rust
入口 fail closed。

Rust 真实门禁关闭一个工具自有、`SW_SHOWNOACTIVATE` 的顶层窗口；精确目标消失、
前台不变、用户窗口关闭数为 0。timeout 和 mapper 另由工具自有拒绝关闭 fixture
验证，C++ 动态等价不再参与验收。
