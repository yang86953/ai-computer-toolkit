# Window target identity assurance v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`act/window-target-identity/v1` 记录当前 `s2:w:*` 能证明的身份强度，不改变 opaque ID 的
既有文本格式。Rust `Window Target Identity Component` 仍把进程 ID、完整当前窗口 token 和进程创建
FILETIME 组成稳定私有材料，再由通用 opaque hash 生成公开 ID；任何原生值都不进入 JSON。

## 已证明与未证明

当前材料能够区分进程创建代际可读时的 PID 回收、不同完整窗口 token，以及同时存活的两个窗口。
#2324 的重建 fixture 在旧窗口仍存活时创建不可见 replacement，因此 replacement 必然持有不同 token，
对应动态 `STALE_SESSION` 证据仍然有效。

当前材料不包含窗口自身的创建代际。Microsoft 的公开文档明确说明：

- [`IsWindow`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindow)
  只能判断当前 handle 是否指向现存窗口，并警告窗口可能在检查后销毁、handle 会回收并指向另一个窗口；
- [`GetWindowThreadProcessId`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid)
  只返回创建窗口的线程 ID 和可选进程 ID，不返回窗口创建时间；
- [`GetProcessTimes`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
  返回的是进程创建 FILETIME，不是窗口创建 FILETIME。

因此，同一进程仍存活、旧窗口完全销毁、Windows 随后把完全相同的完整窗口 token 回收给逻辑新窗口时，
当前三字段材料无法区分两者。标题、class、style、位置、UIA RuntimeId 或可见性都不是可信窗口代际；
它们可重复、可变化或属于 provider 私有事实。项目不得再把当前算法描述为绝对“抵抗窗口 handle 复用”。

## SMC 所有权与停止线

- Window Target Identity Contract Component 只拥有私有材料类别、公开保证强度和缺口。
- Windows Adapter 只读取当前窗口 token、进程关系与进程代际；短命 launcher 不拥有跨请求窗口代际。
- Window/Element/Input/Lifecycle Module 仍执行当前 inventory 唯一重解析，但该操作本身不能创造缺失的
  窗口创建事实。
- discovery 不得给第三方窗口写 property，不得使用未公开 API，不得用标题/class/style 猜测代际，
  也不得把 UIA provider identity 提升为通用窗口写目标。

公开 `targetIdentityStrength` 必须逐字报告
`sameProcessRecycledWindowToken=not-guaranteed` 与 `generationOwner=none`。特殊窗口矩阵对一般
`window-recreated` 的观察和 mutation 结论保持 `gap`；只有 #2324 的 distinct-token fixture 结论是
`supported`。这不撤销当前 capability，但禁止把 confirmation、使用时重枚举或 `IsWindow` 检查描述成
绝对 no-rebind 保证。

## 持久事件 owner 的平台停止线

`WINEVENT_OUTOFCONTEXT` 的事件是异步排队交付；顺序保证只约束事件彼此，不会把 broker 的最后一次
resolve 与随后 `SendMessage`、`SetWindowPos`、WGC、UIA 或输入调用合成原子操作。`IsWindow` 官方文档
还明确警告：检查返回后，另一线程仍可销毁窗口，handle 也可能回收。因此即使持久 owner 已经正确处理
此前的 destroy/create，在 resolve 返回后、平台调用开始前仍存在完全相同 token 被新窗口接管的
TOCTOU。公开 Windows API 没有为任意 HWND 提供可持有到 dispatch 完成的生命周期对象或原子
generation 条件写入。

在禁止写第三方窗口 property、注入 DLL、使用未公开 API 或软件专用协议的边界内，持久事件 owner
只能增强“已交付事件的历史代际”，不能成为绝对 mutation 授权。#2336 的候选仍要求：

1. 由认证同用户本机 broker 持有随机启动 epoch；broker 重启后旧 epoch 全部 stale。
2. 在初始快照屏障后，为窗口创建/销毁建立单调 generation；事件缺口、队列溢出、broker 失联或无法
   证明连续性时使整个受影响 epoch 失败闭合，而不是回退当前三字段 hash。
3. 若仅用于诊断或历史 freshness，公开 ID 仍不含 native token；不得据此升级 mutation 保证。
4. registry 有界、可取消、可回收；不得给目标进程写 property，不得使用未公开 Windows API。
5. 用工具自有同进程高频销毁/创建 fixture 证明旧 ID 永远 `STALE_SESSION`，并覆盖 broker restart、
   event gap、碰撞、超时、权限、前景不变和资源清理。

Vikunja #2337 冻结可行性结论：#2336 协议/状态机不接生产 mutation，#2338/#2339 的绝对
no-rebind 接线被平台停止线取代。只有未来 Windows 提供 lifetime-bound atomic dispatch，或目标 provider
主动提供可验证稳定对象协议时，才能重新打开该保证；当前一般 `window-recreated` 保持 gap。
