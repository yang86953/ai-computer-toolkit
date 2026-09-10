# Provider-neutral keyboard input v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.input.key@1` 是 `app.apply` 上的确认式、主机前台同步 Command。它只接受实时重新发现的
canonical `s2:w:*` 精确窗口、逐操作确认和显式前景同意；严格零打扰请求在 provider
解析前返回 `ISOLATION_REQUIRED`。公开边界不接受 HWND、PID、virtual key、scan code、
原生输入标志、任意消息或特定软件命令。

## 输入与按键所有权

正式输入由 `key-input.schema.json` 冻结，一次请求按顺序执行最多 128 个步骤：

- `key`：完整命名键的 `press`、`down` 或 `up`，`press` 可有界长按和重复；
- `chord`：按调用方顺序按下 2 至 8 个不重复命名键，再按逆序释放，可有界长按和重复；
- `text`：最多 4096 个 UTF-16 单元的非 NUL Unicode 文本，按 Unicode scalar 边界调度。

命名键覆盖字母、数字、F1-F24、编辑/导航键、数字区、Win/Ctrl/Alt/Shift 左右修饰键、
主键区标点位置键、锁定/系统键和常用媒体键。显式 `down` 取得的释放责任只属于当前请求，
必须在同一序列中由同键 `up` 配平；重复按下、无所有者释放、持键期间执行 chord/text 或
返回时仍持键都在任何窗口发现和输入前失败。旧 `{key:"CTRL+S",holdMs:...}` 只兼容安全
的 `press`，映射为同一正式状态机；旧跨请求 `down/up` 不再允许遗留系统级卡键。

## SMC 与平台边界

`ComputerControlSystem` 只负责 Policy、精确目标、确认、前景同意和结果证明。Keyboard
Input Module 拥有 provider-neutral 键集、步骤顺序、deadline、取消、部分执行和请求内
安全释放；Keyboard Windows Adapter 私有拥有 virtual-key/扩展键映射、UTF-16 packet 和
单事件 `SendInput`。键鼠共用的 Windows 窗口 Component 只拥有存在性、恢复、激活和前景
核对。该同步 D3/R0 Command 不使用 EventBus，也没有跨 System 消息或跨请求按键状态。

## 权限、前景与结果

确认必须先于 input、target、进程权限和 Adapter 解析。Module 在任何写调用前验证全部
键位映射，读取目标进程代际、元数据访问和相对完整性；更高完整性、明确访问拒绝、未知
关系或截断 inventory 都失败闭合，不提权、不主动试写。通过门禁后，目标必须有界恢复并
成为前景；每个新按下和 Unicode scalar 前都重新解析 canonical 窗口并核对前景。释放不
会被前景变化、取消或 deadline 阻止。

成功结果由 `key-input-result.schema.json` 冻结，明确请求内按键零残留、完整步骤摘要、
Unicode 支持和禁止自动重试。恢复、激活或 dispatch 开始后的 timeout、取消、前景变化、
目标 stale 和平台拒绝统一返回 `OUTCOME_UNKNOWN`，包含当前阶段、已完成步骤、
`acceptedMayHaveOccurred`、`retrySafe:false` 和安全释放结果。Module 对仍持有的命名键按
逆序 best-effort 发送 `up`；Unicode Adapter 对已接受但未确认释放的单元立即补偿。任何
未确认释放都会保留风险证据，调用方不得自动重试或假定通用回滚。

`outcome:completed` 只证明 Windows 接受了有界 `SendInput` 调度、目标/前景门禁在每一步通过且工具按键
所有权已配平；它不证明目标应用消费了对应消息，也不证明事件具有物理设备来源。#2346 的
`RIDEV_NOLEGACY` Raw Keyboard 夹具经原样生产 launcher 收到 `WM_INPUT`，但消息头没有设备来源。
因此真实应用若要求物理设备身份，当前通用 capability 不承诺其接受效果，运行时也不能把未消费
检测为结构化错误。

## 验证边界

纯回归冻结键集、三类步骤、同请求配平、工作预算、旧兼容映射、确认/隔离优先级、私有
Windows 键表完整性、公开错误白名单与 schema。项目自有窗口夹具负责技术验证快捷键按下
和逆序释放、长按、重复、Unicode、取消/超时安全释放及后续请求恢复。真实窗口视觉和交互
效果仍由 `yang86` 独占验收；自动化结果不能批准或关闭该人类门禁。
