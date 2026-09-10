# Linux Portal EIS 连通 spike

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本 spike 是 L1 Portal 桌面会话底座进入生产 Module 前的一次性证据门禁。它只验证
标准 Portal 会话与 `reis` sender 握手是否能在真实 Wayland 会话中连通，不注册
capability，也不进入发布制品。

## 固定范围

调用顺序固定为：

1. `RemoteDesktop.CreateSession`；
2. `RemoteDesktop.SelectDevices` 请求 keyboard + pointer；
3. 在同一 session 上调用 `ScreenCast.SelectSources` 请求一个 monitor；
4. 由用户在 Portal 对话框中完成 `RemoteDesktop.Start` 授权；
5. `RemoteDesktop.ConnectToEIS` 取得 FD，并以 `reis` 的 sender context 完成握手；
6. `ScreenCast.OpenPipeWireRemote` 取得同会话 PipeWire remote FD；
7. 显式调用 `Session.Close`；客户端主动关闭以成功方法回复为确认，若后端先关闭则
   接受预订阅的 `Closed` 或 Portal owner 换代证据。

spike 不发送任何 EI 输入事件，不调用 RemoteDesktop `Notify*`，不建立 PipeWire
client，不读取或编码像素。`SelectDevices` 与 `SelectSources` 均不携带
`persist_mode` 或 `restore_token`；`Start` 即使返回 restore token 也只记录“已丢弃”
事实，不保存、不输出、不复用。

## 成功与停止条件

只有一次真实运行同时满足下列条件才算连通：

- 当前会话为 Wayland，Portal 已公开 RemoteDesktop v2 与 ScreenCast v5 或更高版本；
- `Start` 实际授予 keyboard 与 pointer，并返回至少一个同会话 ScreenCast stream；
- stream 至少包含一个 `mapping_id`，但输出不得公开其值或 PipeWire node ID；
- `reis` sender 握手完成，PipeWire remote FD 已取得；
- `Session.Close` 以成功方法回复或异步关闭证据被确认为完成；最终 JSON 固定报告
  `inputEventsSent: 0` 与
  `pixelsConsumed: 0`。

任何取消、驳回、超时、Portal owner 换代、返回形状漂移、EIS 握手失败或关闭未确认
都结构化失败并停止，不自动重试，不在已经调用 `ConnectToEIS` 的同一 session 中
透明切换到 D-Bus `Notify*`。

## 运行

先运行无副作用测试：

```bash
cargo test --manifest-path spikes/linux-portal-eis/Cargo.toml
```

真实验收必须同时给出逐次确认与前台许可；命令会弹出系统 Portal 授权窗口：

```bash
cargo run --manifest-path spikes/linux-portal-eis/Cargo.toml -- \
  --confirmed --allow-foreground --timeout-ms 120000
```

缺少两个许可标记中的任意一个时，程序必须在连接 D-Bus 之前拒绝执行。

## 验收记录

- 2026-08-28：9 项单测通过，`cargo clippy --all-targets -- -D warnings` 零诊断；
  缺许可且 Wayland/总线环境无效时仍在 preflight 返回 `CONFIRMATION_REQUIRED`。
- 首次真实 Wayland 运行在 `RemoteDesktop.Start` 等待用户授权时 120 秒超时；输出固定
  `inputEventsSent: 0`、`pixelsConsumed: 0`，且没有自动重试。这不构成 EIS 或 PipeWire
  连通证据。
- 该次失败发现 Session `Closed` 监听建立过晚，无法区分“Request.Close 已连带关闭
  session”与“关闭未确认”；随后改为在 `CreateSession` 调用前预订阅预测 session path。
- 事后只读复盘显示，请求时 KDE Portal 后端记录了全新 `remote-desktop` 权限查询缺失；
  这证明请求到达授权后端，但不能证明弹窗可见或权限已授予。进程退出后 Portal 对象树
  仅剩 `/org/freedesktop/portal/desktop` 根节点，不存在残留 request/session，且探针进程
  已退出；该事后状态不能替代新的同次运行验证。
- 第二次真实运行已完成授权、EIS 握手与 PipeWire FD 取得，却因等待主动关闭后的
  `Closed` 而误报 `SESSION_CLOSE_UNCONFIRMED`。本机 xdg-desktop-portal 1.22.1 的一手源码
  显示客户端 `Close` 路径以 `notify_closed=FALSE` 清理后直接回复，`Closed` 只用于后端
  主动关闭通知；探针已据此把成功方法回复作为主动关闭证据，并保留异步关闭兜底。
- 修正后 10 项单测通过，`cargo clippy --all-targets -- -D warnings` 零诊断。最终真实
  Wayland 运行返回 `outcome=verified`：RemoteDesktop v2、ScreenCast v5、keyboard +
  pointer、1 路 stream/映射、EIS sender 握手、PipeWire remote FD 与 session close 均已
  确认；`restoreTokenRetained=false`、`inputEventsSent=0`、`pixelsConsumed=0`。运行前后
  Portal 对象树没有新增 session，探针进程已退出；既有 RustDesk session 保持不变。

## 标准依据

- [XDG RemoteDesktop v2](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
- [XDG ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [XDG Session](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Session.html)
- [xdg-desktop-portal 1.22.1 Session 实现](https://github.com/flatpak/xdg-desktop-portal/blob/1.22.1/src/xdp-session.c#L170-L230)
- [`reis` 0.7.1](https://docs.rs/reis/0.7.1/reis/)

RemoteDesktop v2 的公开输入路径只有推荐的 `ConnectToEIS` 与 D-Bus `Notify*`；
没有 `ConnectToUInput` Portal 方法。生产 L2 若需要兼容路径，只能在会话建立前按
接口能力选择 `Notify*`，EIS 建立后不得混用。
