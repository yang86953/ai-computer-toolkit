# `ui.input.key@3`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.input.key@3` 是 Linux Wayland Portal 授权桌面会话的候选主机前台键盘 Command。它只在
`ai-computer-toolkit session-host desktop` 持有的精确 live `s2:i` 上执行，使用同一线程内的
EIS sender；普通短命 `app.apply` 不接管 lease，也不得回退 X11、XWayland、xdotool 或私有
compositor 协议。

输入由 `key-input.schema.json` 冻结。它复用 provider-neutral 命名键、最多 128 个步骤、
1..30000ms deadline、成对 press、同请求配平 down/up、有界 chord/长按/重复；不接受文本、
Linux keycode、scan code、keymap、设备 ID 或原生接口字段。Adapter 私有映射到
`linux/input-event-codes.h` keycode，每次状态变化独立提交 EIS frame，避免同帧按下与释放成为
logical noop。

请求必须携带 `confirmed=true`、`foregroundConsent=true`，并拒绝 strict isolation。发送前
完整解析并验证键集、配平与映射；发送后只证明事件已经 flush 到 EIS socket，不证明应用消费或
可见效果。暂停、断开、deadline、flush 失败或设备撤销会使 `s2:i` stale，按已发送事实返回
`failed` 或 `unknown`，报告安全释放与 Session 清理结果，禁止自动重试。

同 broker 的 `input-cancel` 可按原输入 `requestNonce` 协作取消长按、重复间隔或其他等待；
Adapter 最迟每 10ms 检查一次令牌并仍在 owner 线程逆序释放已持有键。取消不能撤回已 flush
事件，原请求返回 `outcome=cancelled`、已完成步骤/事件和禁重试事实，并使会话 stale。

同一 checkpoint 还检查 Portal `Closed`/owner 换代，以及 systemd-logind 当前 Wayland 用户会话
的 `Active`、`LockedHint` 与 `CanLock` 投影；inactive、locked、状态未知或监视异常都使 `s2:i`
stale。`LockedHint=false` 只是公开 hint，不是绝对未锁屏证明。标准 Wayland 不提供跨应用全局
焦点 owner 或所属进程退出通知，因此本能力不绑定焦点进程，也不以 X11 或 compositor 私有协议
补洞。

本能力在用户完成同条件 Portal 授权与真实按键效果验收前保持
`live-acceptance-pending`、不广告 available；自动测试和合成 EIS fixture 不能代替该门禁。
