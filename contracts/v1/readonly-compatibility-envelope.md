# Read-only compatibility envelope

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

迁移期 C++ 的 `status/sessions/inspect` 同时提供：

- 顶层旧调用入口需要的 `app/ok/readOnly` 以及对应 observation 字段；
- `data` 中相同的 provider-neutral C++ 安全对象，供已经接入新契约的调用者使用。

这不是对旧原生诊断字段的复制。顶层和 `data` 都不得出现 HWND、PID、className、
browser/notepad/encoder path、token、SID 或 provider identity；session ID 始终是
opaque versioned target。兼容层不得生成 `window:<HWND>` 或 `process:<PID>`。

在所有调用者切换到单一稳定契约后可移除过渡 `data` 重复，但在此之前两处共享同一
Module 结果，不允许独立实现或产生不同 capability/target facts。
