# Window observation v1 兼容策略

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

C++ `window.discover@1` 只发布可见、有标题的顶层应用窗口，并使用 opaque
`s2:w:*` 身份。`window.metadata.read@1` 对同一实时快照中的精确目标返回应用名、
标题、可见性与公开 capability。

阶段 3B 后 Rust `window` adapter 直接返回 provider-neutral 窗口观察，并拒绝 HWND、
PID、className、bounds、路径和原生 provider identity 选择器。旧
`window:<hwnd>` 目标不再跨越该公开边界；调用方必须使用 opaque session 和
capability，不得通过兼容 mapper 重新泄漏原生标识。

Rust 统一 `app` facade 已迁移到与 C++ 相同的窗口身份：对进程 ID、完整当前窗口 token 与进程创建
FILETIME 的私有字节串生成 `s2:w:*`。发现结果的事实与身份来自
同一快照；inspect 和 execute 不保存原生目标，而是重新枚举并要求该 opaque 身份
唯一命中。provider 内部把目标消失判为 `STALE_SESSION`；统一 `app` inspect 的跨 provider 未命中仍为
`TARGET_NOT_FOUND`，窗口专属 execute capability 由领域 Module 保留 `STALE_SESSION`。重复命中返回
`AMBIGUOUS_TARGET`。进程创建时间、原生窗口值、PID 和 className 都不得跨越统一 JSON 边界。
`sessions window` 已在阶段 3B 切换为
`window.discover@1` schema，`inspect window` 已切换为 `window.metadata.read@1`。

#2329 冻结 `act/window-target-identity/v1`：当前三字段材料能够区分进程代际和不同完整窗口 token，
但 Windows 官方文档明确允许窗口 token 回收，公开 API 不提供窗口创建时间。因此同一进程内完全相同
token 被新窗口回收时，短命 launcher 无法证明窗口代际；每个公开窗口 session 与窗口 assessment 都
必须携带 `targetIdentityStrength`，并报告 `sameProcessRecycledWindowToken=not-guaranteed`。#2337
确认持久异步事件 owner 只能分类已交付历史，不能原子绑定 resolve 与随后平台调用；绝对 no-rebind
需要 Windows lifetime-bound conditional dispatch 或 provider 合作，不能用标题、class、style、UIA
identity、写窗口 property 或未公开 API 伪造。

Rust 门禁只比较稳定且安全的事实：可见且有标题的 `processName + title` 集合，
并独立验证 count/total/truncated、精确 inspect、stale、前台不变以及
HWND/PID/className 泄漏 0。统一 `app` 窗口 session 必须使用相同 canonical
`s2:w:*` 身份；历史 C++ Jaccard 和动态双跑不再参与验收。

本兼容只覆盖 status/sessions/inspect 读取。截图、关闭、移动、激活和输入均不是
window observation 的隐式能力。
