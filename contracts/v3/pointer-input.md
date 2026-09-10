# `ui.input.pointer@3`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.input.pointer@3` 是 Linux Wayland Portal 授权桌面会话的候选主机前台相对指针 Command。
它只在 `ai-computer-toolkit session-host desktop` 持有的精确 live `s2:i` 上执行，使用同一
线程内的 EIS sender；普通短命 `app.apply` 不接管 lease，也不得回退 X11、XWayland、
xdotool、Portal `Notify*` 或私有 compositor 协议。

输入由 `pointer-input.schema.json` 冻结。`coordinateSpace` 固定为
`relative-logical-px`，只表达 EIS logical px 增量，不接受或推导屏幕/窗口绝对坐标。步骤支持
相对 `move`、左/右/中键 `button`、单击/双击 `click`、横纵 `scroll` 与有界采样 `drag`；
`ticks` 表示带符号 wheel click，Adapter 私有映射为 EIS discrete scroll 的 120 单位。请求
最多 64 步、1024 work units、1..30000ms deadline，所有显式 down/up 必须在同一请求配平。

公开输入不接受 Linux button code、设备 ID、FD、Portal path、EIS 类型、窗口命中或前景窗口
假设。每次移动、按钮状态和滚轮事件独立提交 EIS frame；拖拽在按下与释放之间按相对总增量
分配样本。请求必须携带 `confirmed=true`、`foregroundConsent=true`，并拒绝 strict
isolation。

成功只证明事件已 flush 到 EIS socket，`effectConfirmed=false` 且
`finalPointerPositionConfirmed=false`，不证明应用消费、可见效果或最终绝对坐标。暂停、断开、
deadline、flush 失败或设备撤销会使 `s2:i` stale，并按已发送事实返回 `failed` 或
`unknown`；Adapter best-effort 释放请求持有按钮、停止 emulation、关闭 Session，禁止自动重试。

同 broker 的 `input-cancel` 可按原输入 `requestNonce` 协作取消双击间隔或拖拽等待；Adapter
最迟每 10ms 检查一次令牌并仍在 owner 线程释放按钮。取消不能撤回已 flush 的移动、按钮或
滚轮事件，原请求返回 `outcome=cancelled`、已完成步骤/事件和禁重试事实，并使会话 stale。

同一 checkpoint 还检查 Portal `Closed`/owner 换代，以及 systemd-logind 当前 Wayland 用户会话
的 `Active`、`LockedHint` 与 `CanLock` 投影；inactive、locked、状态未知或监视异常都使 `s2:i`
stale。`LockedHint=false` 只是公开 hint，不是绝对未锁屏证明。标准 Wayland 不提供跨应用全局
焦点 owner 或所属进程退出通知，因此相对输入始终发送到当时物理焦点，不绑定目标进程，也不以
X11 或 compositor 私有协议补洞。

本能力在用户完成同条件 Portal 授权与真实移动、左右中键、双击、滚轮、拖拽效果验收前保持
`live-acceptance-pending`、不广告 available；自动测试和合成 EIS fixture 不能代替该门禁。
