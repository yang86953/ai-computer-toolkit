# `window.close.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.close.transition@1` 是 UIX Agent 的精确窗口关闭 transition。它只接受
`s2:w:<opaque>` 窗口目标和可选的 `timeoutMs`，范围为 100–30000 毫秒，默认 30000
毫秒；调用必须逐操作确认，但不请求焦点，也不要求前景同意。

```text
ai-computer-toolkit run app close \
  --capability window.close.transition@1 \
  --target sessionId=<s2:w:opaque> \
  --input <file|-> --confirm
```

成功的唯一证明是：同一认证连接先收到 `perform close_window` 的动作响应，再收到
针对同一窗口 generation、以动作 revision 为基线的 `wait` 显式 `closed` 终态。动作响应的
`actionSettled:false` 仍可成功；成功只表示动作响应和精确关闭终态可信，不表示应用全局
关闭或动作因果已被证明。

连接关闭、传输断开、`app_closed`、非显式 `closed` reply 和丢失终态都不能作为成功证据。
dispatch 已开始后若终态不可信，结果为 `OUTCOME_UNKNOWN`，`retrySafe:false`，并禁止
自动重试。`connectionCloseAcceptedAsProof:false` 与
`applicationClosedConfirmed:false` 固定表达这一边界。

公开结果使用 `same-session-no-focus`，不激活前景、不注入桌面输入、不使用 X11，也不
暴露 provider 原生身份。确认必须先于 input 文件读取、target 解析和 provider I/O。

v0.0.2 已有终态 reply drain，因此该 transition 不声明额外框架缺口。当前 `uix-app`
正在由其他任务修改，本批不写入该项目。
