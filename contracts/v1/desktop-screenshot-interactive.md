# Wayland Portal interactive desktop screenshot v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`desktop.screenshot.interactive@1` 是 Linux Wayland-only 的主机级交互式截图能力。它复用
`act/control/v1`、`CapabilityAction::Screenshot` 和 canonical `s2:h`，不复制公开契约，也不
冒充 `window.screenshot@1` 的精确既有窗口语义。

## 请求与安全边界

- operation 固定为 `desktop.screenshot-interactive`，目标必须等于当前发现结果中的 `s2:h`；
- 输入只接受 `.png` `path`、`timeoutMs=1000..30000` 和布尔 `overwrite`；
- 必须先有逐操作 `--confirm`，再有 `--allow-foreground`；两道门禁都先于目标、路径、文件系统
  和 Portal 访问，缺失时 `portalRequestIssued=false`；
- `executionRealm=host-foreground`。系统会显示来源选择器，可能改变可见前景状态、显示捕获
  指示并读取用户选定的真实屏幕内容；不注入键鼠、不控制窗口；
- 结果固定 `oneShot=true`、`coordinateMapping=none`、`controllableSurfaceIdentity=none`；
  不输出 window target、`foregroundUnchanged`、`screen-physical-px` 或可点击 surface 身份，
  不得从 PNG 像素或尺寸反推输入坐标；
- 不使用 X11、XWayland、xdotool、XTest、Xlib/XCB 或 compositor 私有协议，`fallback=none`。

## Portal 状态机

实现采用标准 `org.freedesktop.portal.Screenshot` v2+，而不是 ScreenCast+PipeWire：本机
Screenshot v2 已提供 interactive 选择与 PNG URI，足以保持本 capability 的主机选择语义；
ScreenCast 会引入 session、PipeWire 节点和流生命周期，却仍不能在调用前绑定既有 opaque
窗口，不能满足 `window.screenshot@1`。

生产 Adapter 使用当前 UID 的固定用户总线地址并禁止自动启动 Portal。请求状态为：

```text
prepared -> connected -> subscribed -> requested -> handle-verified
  -> responded -> completed
  -> closed (timeout)
```

调用 `Screenshot` 前先按 `handle_token` 预测 Request object path 并订阅 `Response`，避免快速
响应竞态；返回 handle 必须是有效 object path。请求固定 `interactive=true`、`modal=true`、
空 parent window，不接受调用方原生句柄或 Portal 私有选项。超时调用 `Request.Close`，取消和
超时都回收 staging，不留下最终文件。Portal well-known owner 换代或总线断开返回独立
`portal-owner-disconnected` 结构化阶段；Screenshot API 不创建 Session，因此“Session 断开”
不适用于本 capability，不能伪造 session 生命周期。

## 结果与失败

成功只接受本机 `file:` URI 指向的非符号链接普通文件，打开前后核对 device/inode，读取上限
64 MiB，并按 PNG 解码验证 `1..=16384` 像素尺寸。源 URI 不进入公开 JSON；合法 PNG 通过同目录
`CREATE_NEW` staging、文件同步和原子最终名称提交，overwrite 必须显式为 true。

| Portal/输出结果 | 公开错误 | 保证 |
| --- | --- | --- |
| response 0 + 有效 URI | success | 原子 PNG、`sourceUriExposed=false` |
| response 1 | `CANCELLED` | 无最终输出，可安全重试 |
| response 2 | `OPERATION_FAILED` | 无最终输出，不猜测原因 |
| deadline | `TIMEOUT` | 已调用 `Request.Close`，无最终输出 |
| Portal owner/总线断开 | `OPERATION_FAILED` | owner-disconnected 阶段，无最终输出 |
| 协议/URI/PNG 非法 | `OPERATION_FAILED` / `CAPTURE_READBACK_FAILED` | fail closed，无 fallback |
| Portal 不就绪 | `CAPABILITY_UNAVAILABLE` | `executionRealm:none` 语义由 assessment 闭合 |

协议依据（检索日期 2026-08-26）：[Screenshot](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html)、
[Request](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Request.html)、
[异步请求模式](https://flatpak.github.io/xdg-desktop-portal/docs/requests.html)、
[parent window identifier](https://flatpak.github.io/xdg-desktop-portal/docs/window-identifiers.html)。

## 验收状态

生产路由、状态机、错误映射、取消/超时、PNG readback、原子输出、fixture、capability、catalog、
assessment 与 schema 已完成。本机无副作用探测确认 KDE Wayland、Screenshot v2 就绪；真实成功
仍需用户批准系统选择器、屏幕读取和输出文件写入后单独执行，未执行前不得宣称真实授权通过。

可坐标控制必须是另一条未来 capability：只有同一 RemoteDesktop session 中 ScreenCast 的
`mapping_id` 与 EIS absolute `region` 建立可验证映射，并新增版本化 session/visual-surface
identity 与真实授权验收后，才可能发布。当前 RemoteDesktop、AT-SPI mutation 与所有 pointer
fallback 均保持 `CAPABILITY_UNAVAILABLE`、realm `none`、fallback `none`。
同理，Portal 截图不建立任意第三方窗口的稳定身份；`window.lifecycle@1`、`window.close@1` 与
active-window 继续不可用，禁止借用 RemoteDesktop、KWin 私有协议或 KWin script 回退。
