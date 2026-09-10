# Special window and input compatibility matrix contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`act/special-window-input-compatibility/v1` 是 GC-COMP-001 的 provider-neutral 机器证据契约。
JSON Schema 位于 `special-window-input-compatibility.schema.json`，当前证据快照位于
`special-window-input-compatibility.json`。快照只描述已经绑定到 Rust contract、原样生产 launcher
fixture 或具名 human gate 的结论，不是生产运行时 capability assessment，也不授权新的写路径。

本契约由 Vikunja #2322 冻结。#2324 已补齐最小化、隐藏与窗口重建的原样生产 launcher 动态
fixture；#2344 已补齐无标准子控件自绘窗口的截图与语义失败动态证据；#2346 已冻结 Raw Input
合成消息与物理设备来源的分层边界；#2350 已用真实 DXGI 独占入口确认当前会话环境不可用，实际独占
发现与截图仍保持 gap；#2354 已核对当前活动显示拓扑只有一个 96-DPI 显示器，生产几何读回一致但
实际多屏/mixed-DPI 仍保持 gap。当前 bot 可执行的机器矩阵项已经用真实入口闭合或冻结停止线；#2004
保持 Blocked，等待可用独占/多屏/mixed-DPI 环境与 #2006 人类门禁。#2006 仍由 `yang86` 独占真实 DPI、
多屏、前景输入与视觉/交互验收。本批不得关闭 #2004 或 #2006。

## SMC 所有权

- Compatibility Matrix Contract Component 只拥有八类状态、五个能力维度、前景要求、结构化失败和证据引用。
- Capability registry、assessment 与 Policy 继续拥有运行时能力和门禁，矩阵快照不能覆盖它们。
- 工具自有 fixture 与探针只产生测试证据，不进入生产 Adapter，也不形成软件专用 capability。
- 真实前景输入、物理显示器和视觉结果只由具名 human gate 接受，机器快照不得继承其批准。

## 封闭状态与结论

状态集合固定为 `custom-drawn`、`raw-input`、`exclusive-fullscreen`、`minimized`、`hidden`、
`window-recreated`、`mixed-dpi` 与 `multi-monitor`。每项必须记录 `discoverable`、`observable`、
`semanticAction`、`pointerInput`、`keyboardInput`、`foregroundRequirement` 和 `structuredFailure`。

结论集合固定为：

- `supported`：当前引用的机器证据足以证明该窄结论；
- `blocked`：精确目标或权限门禁主动阻止调用；
- `unavailable`：当前状态不提供可认证结果，但不是永久平台否定；
- `unsupported`：公开契约明确不承诺该能力；
- `gap`：尚无足够证据，不得解释为支持或不支持；
- `human-gate`：只能由具名用户验收完成。

## 2026-08-16 证据快照

| 状态 | 发现 | 观察 | 语义动作 | 指针 | 键盘 | 当前结论 |
| --- | --- | --- | --- | --- | --- | --- |
| 自绘 | supported | supported | unavailable | human-gate | human-gate | 固定自绘像素可截图；未暴露语义节点时返回 `ELEMENT_NOT_FOUND`，真实键鼠仍待验收 |
| Raw Input | supported | gap | unsupported | human-gate | human-gate | 生产 SendInput 可产生无设备来源的 `WM_INPUT`；不承诺物理设备身份或目标消费 |
| 独占全屏 | gap | gap | unsupported | human-gate | human-gate | 真实 DXGI 入口在当前会话报告环境不可用；不得外推实际独占发现或截图 |
| 最小化 | supported | unavailable | unavailable | human-gate | human-gate | 定位与捕获明确不可用，不猜测命中或自动显示 |
| 隐藏 | unavailable | unavailable | unavailable | unsupported | unsupported | 不显示、不激活；失去精确发现时按 stale 关闭 |
| 窗口重建 | supported | gap | gap | gap | gap | distinct-token fixture 为 stale；同进程完全相同 token 回收仍缺 generation owner |
| Mixed DPI | supported | supported | gap | human-gate | human-gate | 当前主机仅一个 96-DPI 显示器；真实跨 DPI 窗口仍缺证据 |
| 多显示器 | supported | supported | gap | human-gate | human-gate | 当前主机仅一个显示器且无负原点；真实跨屏交互仍缺证据 |

## #2324 动态状态闭合

`ai-computer-toolkit-capture-fixture` 只接受固定标题和 `--minimized`、`--hide-on-command`、
`--recreate-on-command`、`--custom-drawn` 四种测试模式。状态转换模式只从 stdin 接受一次逐字 `transition`，输出只允许固定
完成标记；它不能接收原生目标、任意动作或窗口参数。隐藏路线不显示、不恢复、不激活；重建路线先
创建不可见替代代际，销毁旧窗口后才无激活显示替代窗口，因此新旧句柄不会在同一夹具内复用为同一
opaque identity。

`tests/special_window_dynamic_compatibility.rs` 原样调用生产 PowerShell launcher 与 Rust 主程序：

- 最小化窗口仍可发现，元素定位只返回 `window-minimized` 不可用命中区域；截图返回
  `CAPTURE_TARGET_MINIMIZED`、不创建文件且不恢复窗口。
- 隐藏后公开窗口清单不再发现标题，旧目标的 metadata 与截图均返回 `STALE_SESSION`，不重新显示。
- 重建后同标题窗口取得不同 opaque ID；旧 ID 的 metadata 与元素定位均返回 `STALE_SESSION`，新 ID
  可独立读取，禁止按标题静默重绑定。

三个场景均只操作 fixture 自有进程，串行核对前景不变并在 Drop 中回收子进程和精确临时文件；没有
调用指针或键盘 capability。该证据只闭合三个窄机器状态，不替代 #2006 的真实前景交互与视觉门禁。

## #2344 自绘无输入动态闭合

`ai-computer-toolkit-capture-fixture --custom-drawn` 使用 no-activate 顶层窗口和固定 GDI 像素块，不创建
标准子控件，不接收任意绘制内容、原生目标或动作参数。`tests/custom_drawn_window_dynamic_compatibility.rs`
经原样生产 PowerShell launcher 发现 canonical `s2:w` 目标，并通过 `window.screenshot@1` 原子提交 PNG；
解码后的截图必须包含固定自绘颜色区域。随后同一公开目标的 `ui.element.action@1` 以不存在的
provider-neutral 名称完整搜索，稳定返回 `ELEMENT_NOT_FOUND`，不产生指针 fallback 或屏幕坐标。

该证据只说明可见自绘顶层窗口仍可通过通用窗口捕获观察，以及没有暴露 Accessibility 语义的自绘内容
不会被猜测为可操作控件；它不否定主动暴露标准语义模式的其他自绘窗口。整个场景保持前景不变并回收
夹具进程、请求文件和截图目录，没有执行指针或键盘 capability，不能替代 #2006 的真实交互验收。

## #2346 Raw Input 分层动态闭合

`ai-computer-toolkit-raw-input-fixture` 只接受固定 `--mouse` 或 `--keyboard` 模式；每个进程仅注册
Generic Desktop Usage Page 的一个固定 usage，并使用 `RIDEV_NOLEGACY` 禁止对应 legacy 消息。夹具只在
收到 `WM_INPUT` 后读取 `RAWINPUTHEADER`，以无设备身份的标题后缀区分“消息到达但没有设备来源”和
“消息携带设备来源”；它不读取或公开具体设备身份、按键、坐标或原生窗口事实。

`tests/raw_input_dynamic_compatibility.rs` 是显式 ignored 的真实前景测试。它经原样生产 PowerShell launcher、
canonical `s2:w`、逐操作确认与前景同意，分别执行一次窗口 client 左键单击和 F6 press。两个公开
capability 均报告 dispatch `completed`，两个 NOLEGACY 夹具也都收到 `WM_INPUT`，但消息头均没有设备
来源。测试在断言前恢复原前景和光标，Drop 继续回收夹具与精确请求文件。

因此矩阵只能证明当前 Windows 主机的合成 Raw Input 消息到达；它不能外推为物理 HID 数据，也不能证明
应用业务逻辑实际消费该消息。公开输入结果本来只认证 `SendInput` dispatch 与工具所有权配平，运行时
没有 provider-neutral 回执可把“应用未消费”结构化失败，所以 `structuredFailure` 明确为 unsupported 且
不伪造错误码。真实行为与视觉效果继续由 #2006 验收。

平台边界依据 Windows 官方文档：[RegisterRawInputDevices](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerrawinputdevices)
要求应用先注册才接收 Raw Input；[Raw Input Overview](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input)
把该模型定义为设备原始数据；[SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
只承诺把合成事件插入键鼠输入流。因此机器结果不能升级为物理设备来源承诺。

## #2350 DXGI 独占环境停止线

`ai-computer-toolkit-exclusive-fullscreen-fixture` 先创建 windowed D3D11 swap chain，再以
`IDXGISwapChain::SetFullscreenState(TRUE)` 尝试真实独占；它不使用无边框窗口冒充全屏。夹具只接受固定
`enter`/`exit` 控制命令，通过 `GetFullscreenState` 核对成功状态，并在退出、控制通道断开、独占丢失或
十秒上限到达时先调用 `SetFullscreenState(FALSE)`，核对 windowed 后才释放 swap chain。

`tests/exclusive_fullscreen_dynamic_compatibility.rs` 是显式 ignored 的真实前景与显示模式测试。它先经原样
生产 PowerShell launcher 发现 canonical `s2:w`，只为工具自有窗口建立前景，再请求真实独占。本次主机
会话由 DXGI 明确报告环境不可用，夹具随后恢复 windowed 并以成功状态退出；因此测试没有进入截图分支，
也没有把初始 windowed 发现计为独占发现。未来环境若实际进入独占，测试才会经正式
`window.screenshot@1` 路线要求结构化结果，并在操作后再次通过生产发现核对独占没有被静默破坏。

矩阵据此只新增“当前环境不可用”的真实入口证据，`discoverable`、`observable` 与
`structuredFailure` 仍保持 gap；真实指针、键盘和视觉效果继续由 #2006 验收。平台边界依据 Windows
官方文档：[SetFullscreenState](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiswapchain-setfullscreenstate)
允许在 Terminal Server、窗口被遮挡、失去键盘焦点或其他应用已独占时返回
`DXGI_ERROR_NOT_CURRENTLY_AVAILABLE`，并要求应用保留 windowed 回退；
[DXGI best practices](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/dxgi-best-practices)
要求先以 windowed 创建 swap chain，再显式进入独占。

## #2354 显示拓扑环境停止线

`tests/display_topology_dynamic_compatibility.rs` 是显式 ignored 的当前真实显示拓扑测试。测试私有探针以
`EnumDisplayMonitors` 只保存活动显示器的 provider-neutral 物理矩形，不保留显示器句柄、设备名或
provider identity。它启动 no-activate 工具自有自绘窗口，经原样生产 PowerShell launcher 发现
canonical `s2:w`，再通过正式 `window.lifecycle@1` 把窗口移入每个活动显示器；每次都核对最终位置、
有符号虚拟桌面、Per-Monitor-V2 坐标声明和前景不变，并以移动后 `GetDpiForWindow` 交叉核对生产 DPI。
正常路径最后经同一生产 capability 恢复原位置，Drop 只回收精确夹具与请求文件，不执行指针或键盘。

本次主机只枚举到一个活动显示器，DPI 为 96，虚拟桌面没有负原点。生产读回与该事实完全一致，但没有
实际发生跨显示器移动或 DPI 切换。因此两项只新增“当前环境不具备目标拓扑”的生产纵切证据，不把
`semanticAction`、`pointerInput` 或 `keyboardInput` 升级为 supported；真实多屏、mixed-DPI、负原点与
视觉效果继续由 #2006 验收。

## #2329 窗口身份强度停止线

`act/window-target-identity/v1` 已核对 #2324 的重建证据只覆盖 distinct-token replacement。当前
`s2:w` 私有材料使用进程 ID、完整当前窗口 token 与进程创建代际：它能够区分不同 token 和进程代际，
但没有窗口创建时间或 lifetime-bound dispatch。Windows 把完全相同 token 回收给同进程逻辑新窗口时，
旧 ID 可能再次生成同一指纹；`IsWindow`、标题、class、style、UIA identity 与 confirmation 都不能
补出缺失代际。#2337 进一步确认异步 WinEvent owner 只能在事件交付后记录历史，无法原子绑定最后
resolve 与窗口 API 调用。因此一般 `window-recreated` 的观察与 mutation 四维保持 `gap`，structure failure 只保留
已实现 stale/ambiguous 错误集合而不声称覆盖 token 回收。后续必须由持久、认证、断线/重启失败闭合的
generation owner 补齐；本批不把设计状态计为实现。

## 证据与隐私边界

每条 evidence 只能使用 `rust-contract`、`production-launcher-fixture` 或 `human-gate`，并绑定仓库引用
或 Vikunja 任务。应用名称数量、静态实现猜测和旧候选批准都不是证据。矩阵不得包含 native handle、
UIA RuntimeId、provider 私有对象、任意路径/参数或软件专用路由。平台固有限制必须形成结构化失败、
`gap` 或 `human-gate`，不得静默回退前台、提权、注入或 C++。
