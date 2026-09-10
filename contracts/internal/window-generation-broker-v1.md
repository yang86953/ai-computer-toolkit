# Window generation broker private contract v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`act/window-generation-broker/v1` 冻结持久 Window Target Generation Module 与 Windows
事件 Adapter 之间的私有 JSON 协议、快照屏障和永久失败闭合状态机。它不改变公共
`s2:w:*` 文本格式，也不把 HWND、PID、FILETIME、broker epoch、pipe 或 owner generation
直接放入公共 JSON。

## 公开 Windows 事实

- [`SetWinEventHook`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
  说明 `WINEVENT_OUTOFCONTEXT` 会跨进程排队事件并保证顺序，安装线程必须保持 message loop；
- [`Generating Appropriate WinEvents`](https://learn.microsoft.com/en-us/windows/win32/winauto/generating-appropriate-winevents)
  说明 USER 为标准 HWND 窗口对象提供默认 WinEvent，并定义 create/destroy 通知；
- [`EnumWindows`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumwindows)
  是 desktop 顶层窗口初始与验证快照来源；
- [`UnhookWinEvent`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-unhookwinevent)
  要求在安装 hook 的同一线程释放。
- [`IsWindow`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-iswindow)
  明确警告检查返回后窗口仍可销毁，且 handle 会被回收并指向另一个窗口。

因此后续 Windows Adapter 必须使用 out-of-context hook，不注入第三方进程；同一线程按
hook → 完整 EnumWindows → 顺序排空事件 → 第二次完整 EnumWindows 验证的屏障建立 live
owner。回调只分配内部单调 sequence 并写入固定 65,536 项队列；领域 Module 独占 registry、
generation、屏障和 poison 状态。任何队列溢出、sequence 未追平、事件分类失败、重复 live
create、未知 live destroy、快照不一致或计数器/registry 容量耗尽都使整个 epoch 永久
`poisoned`，不能回退旧三字段 hash。

## 私有 wire

固定同用户本机 broker 启动时生成 128 位随机 `brokerEpoch`，并先发送 `broker-ready`。
client 的每个 `health` 或 `resolve-snapshot` 请求都携带 canonical request nonce 和
`expectedBrokerEpoch`。快照最多 16,384 项；窗口 token 与进程 FILETIME 使用固定 16 位小写
十六进制，避免 JSON number 精度损失。resolve 逐输入位置返回 `resolved + ownerGeneration`
或 `stale`；broker 只在 producer sequence 与 Module applied sequence 完全相等且 owner live
时返回成功。

broker restart 必须产生新 epoch；同一 token 的 destroy/create 必须分配更大的 owner
generation。因此在对应 WinEvent 已交付并应用后，私有材料 `brokerEpoch + ownerGeneration` 能区分
历史实例。协议错误、旧 epoch、owner poison 和容量失败使用四个封闭错误码；不得返回 path、pipe、
HWND、PID、SID、RID、原始 Windows error 或事件 payload。

这不是原子 mutation 证明。out-of-context 事件异步排队，最后一次 resolve 与实际窗口 API 调用之间
仍可能发生 destroy/recreate；`IsWindow` 也明确警告检查后销毁与 handle 回收。协议没有 Windows
lifetime lease 或 conditional dispatch，不能把事件尚未交付解释为窗口未变化。

## SMC 与当前交付边界

- Window Generation Protocol Component 只解析严格 wire 字段和固定预算；
- Window Target Generation Module 独占 bootstrap/live/poison、live registry 与 generation；
- Windows Adapter 后续只拥有 hook、消息循环、EnumWindows 事实和 fixed-local IPC；
- ComputerControlSystem 后续只协调 broker readiness 与既有发现/执行路线。

Vikunja #2336 只交付协议、纯状态机和合成门禁，没有新增 broker binary、WinEvent hook、
fixed-local endpoint 或生产调用方。#2337 进一步确认异步事件无法关闭最终 dispatch TOCTOU；该候选
不接生产窗口身份或 mutation，`targetIdentityStrength.generationOwner` 保持 `none`。
