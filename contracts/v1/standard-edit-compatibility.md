# Standard Edit mutation compatibility contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

Rust 唯一生产 mutation provider 仅认证系统标准 `Edit` 控件的固定 `WM_SETTEXT`；历史
C++ 实现只作为待退役资产，不再参与构建、回退或验收。认证路径不接受
任意 message ID、WPARAM/LPARAM、指针协议、class name、HWND 或 PID。公共目标使用
`s2:c:<opaque>`，每次写前重新枚举并验证控件 class、process identity 和 session。
它是 `ui.text.input@1` 的标准 Edit provider，兼容 operation 名仍为
`win32-control.set-text`。

Module 顺序固定为：

1. 逐操作 `confirmed`；
2. 输入 UTF-8、64 KiB 和 1–30000 ms timeout；
3. 重新解析精确 opaque target；
4. 静态进程元数据与相对完整性必须允许同会话操作；
5. `SendMessageTimeoutW(WM_SETTEXT)`；
6. 有界 `WM_GETTEXTLENGTH/WM_GETTEXT` 回读；
7. 前台前后不变与无原生标识结果。

若 `SendMessageTimeoutW` 超时，错误码保持 `TIMEOUT`，details 必须报告
`outcome:unknown`、`retrySafe:false`、`targetMayHaveMutated:true`。同步消息可能在
调用方超时后才由目标处理，因此兼容层不得把 timeout 表述为“确定未写入”，也不得
自动重试。

compatibility mapper 的成功结果保留旧 `app:win32-control` 与
`operation:set-text`，但用 opaque `sessionId` 替代旧 HWND，并只报告
`foreground.unchanged`。旧 `TARGET_HUNG_OR_UNAVAILABLE` 错误码继续提供给兼容
调用者，同时在 details 保留 provider `TIMEOUT` 和 outcome-unknown。兼容不允许
恢复 HWND 或前景 native handle。

统一 `app.apply` mapper 只返回 `ui.text.input@1`、领域状态、回读验证、
`foreground.unchanged` 和原 opaque target；不返回 provider、HWND、PID 或 class。
Rust 自有隐藏 Edit fixture 已验证 UTF-8 固定写入、逐值回读和前台不变，用户应用
写入数为 0；主 CLI 与逐 capability launcher 固定选择 Rust。C++ 动态等价已退出
验收要求，旧二进制不得替代 Rust 正式证据。旧
`win32-control:window:*` 仍只由 Rust 有限兼容入口处理，新调用必须使用 `s2:c:*`。

Vikunja #1974 用 Module 私有 `StandardEditErrorCode` 统一目标歧义、后台不可用、确认、
宿主干扰、输入、操作、权限、stale 与 timeout 九项错误定义。二十三个构造点（其中五个
带安全 details）不再分别维护公开字符串；上述顺序、逐字 error envelope、outcome-unknown
和 Windows Adapter 的封闭失败生命周期保持不变。
