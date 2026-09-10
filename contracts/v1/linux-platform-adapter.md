# Linux platform adapter v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本文冻结 `ai-computer-toolkit 0.0.1` 的首个 Linux 平台纵切。公开边界仍是
`act/control/v1` JSON over stdio、现有版本化 capability 和 `s2:*` opaque target；
Linux 不拥有也不复制第二套公开协议。

## SMC 与依赖边界

- `ComputerControlSystem` 组合根选择并持有 Linux provider registry，只协调命令；
- Process Adapter 拥有 `status/sessions/inspect` 的进程观察语义；
- Desktop Adapter 拥有 Wayland/Portal 就绪度与交互式截图路由；Portal Screenshot Module
  拥有确认、前景同意、主机目标、PNG 验证和原子输出，Wayland Portal Adapter 独占 D-Bus
  请求状态机；平台无关 System/Module 不接触 zbus、Wayland 或 compositor 类型；
- Desktop Session Module 拥有 `s2:i` 会话代际、确认/前景/strict 门禁、活动 lease 上限和
  open/inspect/input/close 生命周期；单线程长期 Desktop Session Broker 是候选生产 owner，Portal
  Adapter 私有持有 zbus、EIS event stream、ready/resumed 键盘设备与 PipeWire fd。键盘 Module
  只拥有 provider-neutral 配平步骤和失败投影，Linux EIS Component 私有拥有 keycode 映射、
  分帧调度及安全释放。公开 JSON Lines 只发布中立事实，
  不发布 Portal 路径、request handle、PipeWire node/fd 或 libei 类型；
- Linux Application Discovery Module 通过私有 DesktopEntryInventory Adapter 组合 XDG
  Desktop Entry 中立记录，并与 procfs 清单独立投影 `application.discover@2`；parser、路径、
  Desktop File ID、`Exec` 与 `TryExec` 类型不跨 Module 边界；
- Linux Window Observation Module 只拥有 partial accessibility exporter discovery、使用时唯一
  resolve 与 stale；Linux Accessibility Module 只拥有有界 BFS、snapshot、deadline/cancel 和
  公共中立投影。AtspiBusConnector/InventoryReader/AccessibleReader/Identity 均为私有 Adapter/
  Component，zbus、bus owner 与 object path 不跨边界；
- UIX Agent Adapter 独占私有发现文件、Unix socket、token、PID、UIX window/node 原生 ID、
  `SO_PEERCRED` 与 `uix.agent.v1` JSON Lines；UIX Window Module 只接收中立记录并拥有
  `window.*@3` / `accessibility.tree.read@3` 的限制、opaque 投影和 parent-first BFS；独立 UIX
  Key Input Module 拥有 `ui.input.key@2` 的确认、键名限制与部分处理语义，Pointer Input Module
  拥有 `ui.input.pointer@2` 的 logical 客户区 move/click 与部分处理语义，Lifecycle Module 拥有
  `window.lifecycle@2` 的确认、前景同意、动作限制与结果语义。
  ComputerControlSystem 只把 `app`、`window`、`accessibility` surface 装配到生产 Adapter；
- procfs Adapter 只读取 `/proc/<pid>/comm` 与 `/proc/<pid>/stat`，返回 System 私有的
  中立记录；PID 与启动时钟不得跨公共边界；
- opaque target Component 继续使用既有 FNV-1a `s2` 格式。Linux 私有材料为
  `linux:<procfs_owner_epoch>:<pid>:<start_ticks>:<process_name>`；owner epoch 由 canonical boot ID
  与当前 PID namespace 的 nsfs device/inode 组成，防止与 Windows 身份材料、跨重启、跨
  PID namespace 旧目标或 PID 复用混淆；owner epoch 只参与 hash，不进入公共 JSON；
- `windows` 与 `windows-future` 只属于 Windows Cargo target。平台无关 System、Module、
  capability、catalog 与 domain 不依赖 Win32、COM、UIA、WinRT 或 Windows crate 类型。

## 已认证能力

| capability | Linux 状态 | 执行域 | 公开保证 |
| --- | --- | --- | --- |
| `process.discover@1` | available | `host-headless` | 运行进程、opaque ID、名称、状态；不公开 PID/路径/UID/token |
| `process.metadata.read@1` | available | `host-headless` | 每次重新读取 procfs 并唯一匹配；退出或代际变化返回 stale |
| `process.terminate.graceful@2` | available（保护姿态满足时）/机器验证 | `host-background` | direct Process surface；确认优先，同 UID 非 root、无 CAP_KILL、精确代际与保护域门禁，pidfd SIGTERM+退出等待，无 SIGKILL、原生身份或自动重试 |
| `process.terminate.force@2` | available（保护姿态满足时）/机器验证 | `host-background` | direct Process surface；显式 critical 路线，procfs owner epoch + pidfd 精确代际、同 UID/保护域门禁，固定强制终止+退出等待，无温和前置、fallback、原生身份或自动重试 |
| `application.discover@1` | unavailable | `none` | 冻结的 Windows v1 枚举与 identity 语义不扩展到 Linux |
| `application.discover@2` | available-partial | `host-headless` | XDG Desktop Entry 应用与 procfs 进程独立枚举；关系 none，窗口为空 |
| `application.discover@3` | verified / fixture-scope | `host-headless` | 保持 @2 清单与隐私，只为 use-time 可认证的 toolkit 自有夹具发布 `available-confirmed` |
| `application.open@2` | verified / target-conditional | `host-foreground` | 确认与前景同意优先；只经 `/proc/self/exe` 启动固定 toolkit 夹具 argv，空环境、无 shell、无用户应用 |
| `application.session.discover@1` | unavailable | `none` | 冻结 Windows provider 聚合语义；Linux `sessions app` 不扩写 |
| `application.session.discover@2` | available | `host-headless` | 显式版本选择；当前 host + XDG/procfs 独立清单，零关系；该聚合未接入独立 UIX 窗口来源 |
| `application.session.discover@3` | verified（真实 Wayland） | `same-session-no-focus` | 显式版本选择；在 @2 独立清单上增加 opt-in UIX 窗口及认证 peer 对应的精确进程关系；不推断应用关系或全局窗口覆盖 |
| `application.session.discover@4` | verified / launch-aware | `same-session-no-focus` | 保持 @3 窗口—进程关系；同一 XDG 快照按 `application.discover@3` 发布夹具启动状态，普通应用 unavailable，仍不推断应用关系 |
| `media.session.discover@2` / `media.playback.state.read@2` / `media.playback.control@2` | candidate | `none` | discover/state 与 control 已由 feature-gated 读/写分离 worker 在显式私有 D-Bus fixture 内验证；control 使用 accepted→final 区分 dispatch 前失败与 OutcomeUnknown；生产仍不构建/启动 worker、不连接真实 MPRIS、不自动激活、不回退 @1 |
| `desktop.screenshot.interactive@1` | live-acceptance-pending（Portal v2+ 就绪时） | `host-foreground` | 当前 `s2:h`、系统交互选择、有界 PNG 原子输出、无 X11 fallback；真实成功路径未通过 |
| `desktop.session.open@1` | live-acceptance-pending，未发布 | `none` | 候选 launcher 已接线；确认与前景同意后先验证公开 logind Wayland user session，再创建进程内不可持久会话；严格隔离拒绝、无 fallback、真实生产验收未通过 |
| `desktop.session.close@1` | live-acceptance-pending，未发布 | `none` | 只消费同一 broker owner generation 的精确 `s2:i`；先 stale 再关闭，未确认清理时返回未知结果且禁止重试 |
| `screen.capture@1` | live-acceptance-pending，未发布 | `none` | 只在同一 broker 的精确 live `s2:i` 上 confirmation-first 消费一帧；有界 PipeWire packed raw 转 PNG 并原子提交，无原生目标/Base64/DMA-BUF/连续录制/自动重试；真实视觉验收未完成 |
| `ui.input.key@3` | live-acceptance-pending，未发布 | `none` | 精确 live `s2:i` EIS 命名键；配平、deadline、request-nonce 取消、安全释放与 Portal/logind 活动失效监测已接线，不支持文本或全局焦点进程绑定，不声明应用消费，无 fallback |
| `ui.input.pointer@3` | live-acceptance-pending，未发布 | `none` | 精确 live `s2:i` EIS relative logical px 移动、三键、单双击、滚轮与拖拽；取消、安全释放与 Portal/logind 活动失效监测已接线，不接受绝对坐标或焦点进程身份，不声明应用消费或最终位置，无 fallback |
| `window.discover@2` | candidate/private-fixture-only | `none` | partial accessibility exporters；只承诺 Visible+Showing，不代表 compositor 全局窗口目录 |
| `window.metadata.read@2` | candidate/private-fixture-only | `none` | owner/bus 代际 use-time resolve；同 owner object-path 复用不保证，mutationAllowed=false |
| `accessibility.tree.read@2` | candidate/private-fixture-only | `none` | 中立节点、有界 BFS、终态一次发布；默认生产路由零 dispatch |
| `window.discover@3` | verified（真实 Wayland） | `same-session-no-focus` | 只发现显式启用 `uix.agent.v1` 的 UIX 应用窗口；不是 compositor 全局目录 |
| `window.metadata.read@3` | verified（真实 Wayland） | `same-session-no-focus` | token/PID/window generation 绑定的 `s2:w` 使用时重新认证；不公开传输或原生身份 |
| `accessibility.tree.read@3` | verified（真实 Wayland） | `same-session-no-focus` | 有界脱敏 UIX 语义快照；合法空名称归一化，不发布 bounds/text/value/selection，不调用 mutation |
| `ui.element.action@2` | verified（真实 Wayland） | `same-session-no-focus` | 精确快照节点语义动作经双层确认门禁；无主机激活、桌面输入或 fallback |
| `ui.input.key@2` | verified（真实 Wayland） | `same-session-no-focus` | 对精确 UIX 窗口发送应用内部成对 press；无主机激活/桌面输入，down/up、长按、重复和文本不可用 |
| `ui.input.pointer@2` | verified（真实 Wayland） | `same-session-no-focus` | 对精确 UIX 窗口发送 logical 客户区内部 move/click；无主机激活/桌面指针，down/up、拖拽、滚轮、其他按钮不可用 |
| `window.revision.wait@1` | verified（真实 Wayland） | `same-session-no-focus` | 精确 `s2:w` 总 deadline 内重新认证并等待 revision/presentedRevision；同代际关闭为终态，无 compositor、输入或 fallback |
| `window.closed.wait@2` | verified（真实 Wayland） | `same-session-no-focus` | 同一 Agent 连接跨过普通 revision，直到精确 generation 返回 closed；不轮询标题或全局窗口 inventory |
| `window.close@2` | verified（真实 Wayland） | `same-session-no-focus` | 逐操作确认后只提交 UIX 平台关闭请求；终态由 window.closed.wait@2 独立证明，未知结果禁止重试 |
| `window.lifecycle@2` | verified（真实 Wayland） | `host-foreground` | 确认与前景同意后对精确 UIX 窗口恢复、最小化、最大化或 logical 客户区缩放；无 Wayland move、最终状态夸大或 fallback |

进程观察不连接图形会话，因此 `foregroundUnchanged=true` 表示该 provider 没有执行任何
前景 API。版本二没有窗口关系来源，其 `windowSessionIds` 为空且以
`complete.windows=false` 和 coverage unavailable 约束解释。版本三只关联显式 opt-in UIX
窗口；空且 complete 的 UIX 清单仍不能解释为主机没有其他真实桌面窗口。

## Wayland/Portal 就绪度诊断

`status desktop` 本身是无权限、无桌面内容读取的诊断入口。
它只检查当前 UID 的 `/run/user/<uid>/wayland-N` Unix socket 是否存在，并通过固定绝对路径
`/usr/bin/busctl` 查询用户 D-Bus 当前已注册名称及 Portal 公开 `version` 属性。调用参数完全
固定、不经过 shell，属性查询设置 `--auto-start=no` 与一秒 deadline。

公开结果必须保持：

- `wayland.connectionAttempted=false`，不握手、不枚举 compositor global；
- `portal.requestIssued=false`、`permissionPrompted=false`，不调用 CreateSession、SelectSources、
  Start、Screenshot 或 RemoteDesktop 动作；
- Screenshot v2+、Wayland socket、用户总线与已注册 Portal 全部就绪时，发布
  `desktop.screenshot.interactive@1`；否则 capability 列表为空；
- RemoteDesktop v2+ 与 ScreenCast v5+ 就绪时只发布 `desktopSessionCandidate` 诊断事实；
  `productionRouteWired=true` 不等于真实验收或 availability，普通 capability 列表仍不包含
  `desktop.session.*@1` 与 `screen.capture@1`；
- ScreenCast、RemoteDesktop、Screenshot 的 advertised/version 只证明标准接口已注册，不能
推断用户已授权、PipeWire 流可读、屏幕可捕获或输入可注入。

用户 D-Bus、`busctl`、Wayland runtime socket 或 Portal 缺失时，status 仍返回诚实的 false/null
诊断；不得启动服务、安装组件或将缺失降级到其他桌面协议。

## 缺口与失败闭合

以下能力没有认证 Linux provider：应用启动、compositor 全局 Wayland 窗口、
任意第三方应用窗口控制、真实 AT-SPI 会话与 mutation、精确窗口截图、经用户真实验收的 PipeWire 像素读取、
桌面键盘/指针事件、通用窗口生命周期、文件/应用写入、媒体与 Windows worker/broker。UIX v3/v2
只覆盖明确 opt-in 的协作式 UIX 应用，不能被解释为这些通用缺口已经关闭。

通用 Wayland/Portal 不提供任意第三方窗口的完整发现、稳定跨请求身份或 lifecycle Command；
`window.lifecycle@1`、`window.close@1` 与通用 active-window 固定不可用。不得把 RemoteDesktop
输入、KWin 私有协议、KWin script 或 Portal 包装成窗口生命周期 fallback。
`ext_foreign_toplevel_list_v1` 未来只能在 compositor 实际广告时作为条件只读 discovery/inspect
来源，且不能提供 state mutation；当前实现没有连接 Wayland registry，因此不把本机是否支持
该协议写成实测，也不在截图纵切内顺带实现。

assessment 或执行必须返回结构化缺口，至少包含：

```json
{
  "code": "CAPABILITY_UNAVAILABLE",
  "details": {
    "platform": "linux",
    "executionRealm": "none",
    "fallback": "none"
  }
}
```

不得 panic、链接 Windows 库、启动 Windows worker、安装桌面组件、连接 Wayland compositor、
发送桌面输入、向非 UIX 应用派发事件或把缺口降级为前台路径。普通公开 dispatch 只有
`desktop.screenshot.interactive@1` 在确认与前景同意齐备后可以请求 Screenshot Portal；
`desktop.session.*@1` 与 `screen.capture@1` 在真实验收前仅允许通过固定隐藏 launcher 做用户明确批准的 L1/L3 候选验证；L1 打开仍保持零输入、
零像素验收，不得广告为 available，也不得被其他缺口借用。

Linux `capabilities` 对 `capabilities::ALL` 的每一项逐项给出且只给出一个封闭分类：`partial`、
`verified`、`live-acceptance-pending`、`candidate` 或
`conditional-or-permanent-unavailable`。当前 verified 包含 procfs 两项、pidfd 温和/强制终止 v2 与
`application.discover@3`、toolkit 夹具范围的 `application.open@2`、
`application.session.discover@2/@4` 以及 UIX 协作式路线；`application.discover@2` 为 partial，
截图、两项 `desktop.session.*@1` 与 `screen.capture@1` 为 live acceptance pending，其中 session 路线仍不可广告 available；AT-SPI 与 MPRIS 各三项 v2
为 private-fixture-only candidate，其余均为 conditional/permanent
unavailable；未知 capability ID 必须返回 `INVALID_ARGUMENT`，不得因为
文本形似版本化 ID 而降级成普通 unavailable。

`process.terminate.graceful@1` 在 Linux 永久 unavailable，SIGTERM 不是顶层窗口关闭协议。
独立 `process.terminate.graceful@2` 与 `process.terminate.force@2` 已分别接入
`process.run terminate-graceful` / `terminate-force`：确认先于 input、target 与 procfs/pidfd；
目标必须由绑定 boot ID + PID namespace identity 的 procfs owner epoch 有界完整清单唯一命中，打开 pidfd 后重核
owner epoch、session ID、start ticks、四项 UID 与 NSpid。当前工具必须是同 UID 非 root 且无
`CAP_KILL`，目标不能是当前工具、检查时 procfs 可见祖先或任一命名空间 PID 1。温和路线只发送
一次固定 SIGTERM 且不升级；强制路线必须显式选择，只提交一次固定强制终止且不先尝试温和路线。
两者都以同一 pidfd 的 POLLIN/POLLHUP 证明退出；不公开 boot ID/namespace identity/PID/UID/fd/signal，accepted 后
unknown 禁止自动重试。首批只有总 deadline，不提供独立异步 cancellation。
`process.terminate.force@1` 当前 conditional unavailable 且不得由 Linux 承接，因为其成功契约
冻结了 Windows critical/integrity 语义；Linux 只发布独立 provider-neutral `force@2`，不填充
Windows integrity 字段，也不形成跨版本 fallback。
Linux browser 九项未来必须以独立 `@2` 和分代身份整体交付；当前 archive 不包含 browser
worker，也不发布 candidate。其认证运行时还需要用户另行授权的系统级 S3 cgroup/service
边界，本阶段不安装 root launcher、system service、PolicyKit 或 D-Bus policy。
未来固定 browser runtime 只能作为独立受管 immutable Chrome for Testing payload 交付，不能
扩容当前主 archive、查询系统/用户浏览器、读取 PATH/env 或运行时下载；在取得受控下载授权、
项目 SHA-256/逐文件 hash、许可/SBOM 与真实 sandbox E2E 前保持 unavailable，禁止
`--no-sandbox`，非 x86_64 也不作兼容承诺。
`application.open@1` 保持冻结的 Windows 语义，Linux 当前 unavailable；Linux 默认
`application.discover@2` 的 `launchCapability` 永久 unavailable。显式
`application.discover@3` 与 `application.open@2` 已原子接线，但首批只认证 Desktop Entry
中绝对 Exec 指向当前 toolkit 映像且唯一参数为固定私有夹具子命令的目标。use-time 重新枚举
仍需唯一命中同一内容代际，随后通过 `/proc/self/exe`、固定 argv、空环境、固定工作目录与
null stdio 直接启动；不执行 Desktop Entry 路径，不接受 field code、调用方 argv、URI 或
环境，也不使用 gio/gtk-launch/xdg-open/shell/PATH/Portal fallback。普通用户应用和标准
D-Bus activation 继续 unavailable；扩容前必须独立补齐完整 Exec 语义、executable FD 绑定、
权限/所有权策略与真实应用验收。

## Wayland-only 与权限停止线

Linux 桌面只采用标准 Wayland protocol、Desktop Portal、PipeWire 或桌面环境公开接口。
不新增 X11/XWayland、xdotool、XTest、Xlib/XCB 路线，不把 XWayland 作为兼容层或 fallback；
compositor 私有协议也不得成为未声明依赖。任何需要用户选择窗口、共享屏幕、批准 portal、
修改会话、读取真实屏幕、注入输入或产生可见前景影响的真实验收，都必须先取得用户决定。

procfs provider 不提权。`/proc` 根不可读时返回公共 enum 已登记的
`PROCESS_SNAPSHOT_FAILED`；`PROVIDER_UNAVAILABLE` 不属于 v1 error envelope，禁止发出。
单个短命或不可读
进程使快照 `complete=false`，不会把缺失记录伪报为完整。当前不公开 Linux UID、namespace、
cgroup、可执行路径或内核错误文本。

XDG Desktop Entry 纵切只解析标准数据目录和公开显示名，处理 XDG 优先级、Hidden、
NoDisplay、桌面环境、TryExec 与 locale；NoDisplay 仍属于 installed inventory，OnlyShowIn 与
NotShowIn 同时出现时按 `XDG_CURRENT_DESKTOP` 左到右首个匹配决定，同名同时存在于两表才
视为非法。不执行或公开 `Exec`、`TryExec`、Desktop File ID、文件路径。

DesktopEntryInventory Adapter 从可信 `applications` root fd 做 descriptor-relative 遍历：
目录逐段 `NOFOLLOW|NONBLOCK` 打开，文件用 `openat2` 的 `BENEATH|NO_SYMLINKS|NO_MAGICLINKS`
约束并保持 nonblocking；目录/文件 symlink 与 FIFO/socket/device 一律拒绝。候选在打开前、
打开后、读取后用 fd metadata 复核 regular、大小上限和 device/inode/mode/size/mtime/ctime
代际；替换竞态只会令本次清单不完整，不会读取根外内容或阻塞 FIFO。`warnings` 仅使用 schema
登记的无路径稳定枚举；`applicationsReturned`、nullable `totalEligibleApplications` 与
`applicationsTruncated` 区分返回项、可证明总量及截断。所有配置根均缺失时来源 unavailable、
`complete.applications=false`；部分根不可读、不安全或发生竞态时保留安全结果但完整度为 false。

`s2:a` 私有材料绑定 provider domain、胜出项、优先级与打开后文件/内容代际，替换或胜出项
变化后旧 ID stale；公开 `act/application-target-identity/v1` 只声明该边界。
AT-SPI v2 只读候选的 worker 只在非默认 feature 下接受显式私有 session/accessibility bus 地址；
默认 CLI 不读取环境 bus 地址、不调用真实 `org.a11y.Bus`。公开只允许有界 window title 与 node
Name；bus address/GUID/name/object path、PID/UID、toolkit/version、Text/Value/坐标等均禁止。
真实 KDE 验收会读取全局 UI 元数据，仍需用户另行授权，且不得把 AT-SPI identity 升级为 mutation 目标。

## 兼容与验收

Windows 源文件、provider、worker 和测试由 `cfg(target_os = "windows")` 保持原实现；
Linux 只替换组合根和平台 Adapter。`version` 与 `build-info` 字段保持原 schema；新增平台
事实只出现在 Linux status、capability matrix 和结构化缺口中。

最低回归为：

```text
cargo check --all-targets
cargo test --test linux_process
cargo test --test linux_schema_contracts
cargo test --test linux_wayland_desktop
cargo test --test linux_desktop_session_contract
cargo test --test linux_atspi_contracts
cargo test --features linux-atspi-candidate --test linux_atspi_candidate
cargo test --test linux_uix_agent_contracts
cargo test --test build_info
```

测试必须真实命中至少一个当前 Linux 进程，使用返回的 `s2:p` 完成 inspect 与
`process.metadata.read@1` assessment，并验证 Wayland/Portal hermetic fixture、真实 status 的零请求证据，
交互式截图的确认/前景门禁、状态机、取消/超时和原子 PNG fixture，以及未支持窗口和
输入 surface 的无 fallback 失败。headless CI 不要求 Portal-ready/`host-foreground` 或真实
Desktop Entry 非空；真实清单测试只验证 schema、统计和隐私且不输出应用名。2026-08-26 的
一次用户授权验收以 `TIMEOUT` 闭合，输出
路径不存在且未重试；因此真实 Portal Screenshot 成功路径仍未验证。L1 Desktop Session 的
schema、Module、Portal Adapter 与真实 stdio launcher 回归属于生产候选；2026-08-28 两次经用户
明确同意的正式 open 均在 `RemoteDesktop.Start` 到达 120 秒 deadline，返回
`acceptedMayHaveOccurred=true`、`retrySafe=false`、`automaticRetryProhibited=true`，并确认
session cleanup、活动会话为零和无新增 Portal 对象。它们没有自动重试，尚不能发布 availability。
2026-08-30 的 L2 机器子批增加公开 systemd-logind 活动投影与 Portal `Closed`/owner 监视，
inactive、locked hint、属性/服务/流未知均拒绝输入，且最终 flush 后复检；`LockedHint=false` 不作
绝对安全证明。标准 Wayland 没有跨应用全局焦点进程生命周期信号，因此该项仍是明确协议缺口。
本批没有触发 Portal、锁屏或输入，不改变 live acceptance 门禁。

## 规范依据

检索日期 2026-08-26：

- [Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry/latest-single/)
  定义 Desktop File ID、Hidden/NoDisplay、OnlyShowIn/NotShowIn、TryExec 与 locale；
- [XDG Base Directory Specification 0.8](https://specifications.freedesktop.org/basedir/0.8/)
  定义 `XDG_DATA_HOME`、有序 `XDG_DATA_DIRS` 与默认值；
- [Desktop Menu Specification](https://specifications.freedesktop.org/menu/latest-single/)
  定义 `applications/` 下 Desktop Entry 的组合与优先级；
- [openat2(2)](https://man7.org/linux/man-pages/man2/openat2.2.html) 定义 descriptor-relative
  `RESOLVE_BENEATH`、`RESOLVE_NO_SYMLINKS` 与 `RESOLVE_NO_MAGICLINKS` 解析边界；
- [jsonschema crate](https://docs.rs/jsonschema/latest/jsonschema/) 用于 Draft 2020-12
  公共实例验证。
