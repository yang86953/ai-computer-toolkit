# Provider-neutral pointer input v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.input.pointer@1` 是 `app.apply` 上的确认式、主机前台 Command。它只接受实时重新发现的
canonical `s2:w:*` 精确窗口、逐操作确认和显式前景同意；严格零打扰请求在 provider
解析前返回 `ISOLATION_REQUIRED`。公开边界不接受 HWND、PID、原生鼠标标志、虚拟键码、
任意消息或特定软件命令。

## 输入与所有权

正式输入由 `pointer-input.schema.json` 冻结。一次请求统一选择
`screen-physical-px` 或 `window-client-physical-px`，并按顺序执行最多 64 个步骤：

- `move`：移动到一个精确点；
- `button`：左、右或中键的 `down` / `up`；
- `click`：同三类按钮的单击或双击；
- `scroll`：垂直或水平的有符号刻度滚动；
- `drag`：有界持续时间和采样数的按下、移动、释放宏。

显式 `down` 取得的按钮所有权只属于当前请求，必须在同一序列中由对应 `up` 释放；重复
按下、无所有者释放或返回时仍持有按钮都在任何窗口发现和输入前失败。该约束避免短命
CLI 返回或退出后遗留系统级卡键。旧 `{x,y}` 形状继续等价为屏幕物理像素左键单击，
但统一 capability 不再委托旧 `desktop.click` 实现。

## SMC、坐标与精确目标

`ComputerControlSystem` 只负责 Policy、执行计划冻结和结果证明。Pointer Input Module
拥有请求内按钮状态机、deadline、取消、部分执行和安全释放；Windows Pointer Adapter
只拥有 Per-Monitor-V2 DPI 上下文、客户区坐标转换、虚拟桌面范围、窗口命中测试、前景
激活和单事件 `SendInput` 调用。该同步 Command 不使用 EventBus，也没有跨 System 消息。

全部屏幕坐标都是允许负值的带符号虚拟桌面物理像素。窗口相对点会在每次移动前按窗口
当前客户区重新转换，因此窗口在序列或拖拽期间移动时不会复用旧屏幕点。首次接触、点击、
滚轮和拖拽起点必须实际命中精确窗口或其子窗口；只有当前请求已经持有按钮时，移动和
释放才可越过窗口边界。这样既允许真实拖拽，又不会把任意屏幕点静默路由到其他软件。

## 权限、前景与结果

确认必须先于 input、target、进程权限和 Adapter 解析。Module 在任何写调用前读取目标
进程代际、元数据访问和相对完整性；更高完整性、明确访问拒绝、未知关系或截断 inventory
都失败闭合，不提权、不主动试写。通过门禁后，目标必须有界恢复并成为前景；每个点和
输入事件前都重新解析 canonical 窗口并核对前景。

成功结果由 `pointer-input-result.schema.json` 冻结，并明确按钮零残留、逐点重新解析、
Per-Monitor-V2 坐标和禁止自动重试。恢复、激活或 dispatch 开始后的 timeout、取消、
前景变化、目标 stale、坐标变化和平台拒绝统一返回 `OUTCOME_UNKNOWN`，包含当前阶段、
已完成步骤、`acceptedMayHaveOccurred`、`retrySafe:false` 及安全释放结果。Module 会忽略
前景和 deadline 对仍持有按钮逐一发送 best-effort `up`；若释放没有被 Windows 确认，
`buttonsHeldByTool` 保留风险。调用方不得自动重试 unknown outcome，也不得假定通用回滚。

`outcome:completed` 只证明 Windows 接受了有界 `SendInput` 调度、目标/前景门禁在每一步通过且工具按钮
所有权已配平；它不证明目标应用消费了对应消息，也不证明事件具有物理设备来源。#2346 的
`RIDEV_NOLEGACY` Raw Mouse 夹具经原样生产 launcher 收到 `WM_INPUT`，但消息头没有设备来源。
因此真实应用若要求物理设备身份，当前通用 capability 不承诺其接受效果，运行时也不能把未消费
检测为结构化错误。

## 验证边界

纯回归冻结输入封闭性、按钮配对、负坐标多屏边界、三类按钮、拖拽插值、确认优先级、
错误白名单与 schema。项目自有窗口夹具负责技术验证真实光标移动、Adapter 接受左右中键、
单双击、水平/垂直滚轮、窗口移动后的客户区转换、拖拽和取消安全释放。真实多显示器、DPI、
视觉和交互
效果仍由 `yang86` 独占验收；自动化结果不能批准或关闭该人类门禁。

2026-08-12 的冻结证据为核心提交 `585ec0d`、夹具/中断提交 `6b9b1d6` 和 Vikunja 评论
#1388。显式桌面夹具命令
`cargo test --lib pointer_input::dynamic_tests -- --ignored --test-threads=1` 为 3 passed、
0 failed；常规 `cargo test --all-targets` 为 473 passed、0 failed、3 个前景夹具按设计
ignored，`cargo clippy --all-targets -- -D warnings` 通过。ignored 只避免普通并行测试擅自
改变用户前景和光标，不表示跳过技术验收。
