# Exact process termination contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`process.terminate.graceful@1` 与 `process.terminate.force@1` 是统一 `app.close` 上两个
独立的 provider-neutral 同步 Command。风险只能由 capability ID 选择，不能由 input、
provider 或 timeout 自动升级；优雅终止失败、取消或超时绝不回退强制终止。

## 输入、目标与确认

正式目标只接受当前 `sessions app|process` 返回且 `identityFreshness=process-lifetime` 的
canonical `s2:p:<opaque>`。System 在任何平台调用前重新枚举完整进程 inventory 并要求
唯一命中；PID、进程创建时间和名称只用于私有代际核对，公共请求与结果不得出现这些值。
零命中返回 `STALE_SESSION`，多命中返回 `AMBIGUOUS_TARGET`；打开进程后发现退出或创建代际
变化必须以写前 `STALE_SESSION` 失败闭合。

两个 capability 都要求各自逐操作 confirmation。input 必须严格匹配
`process-termination-input.schema.json`，只允许 1–30000 ms 的可选 `timeoutMs`；不得接受
mode、signal、exitCode、PID、path、argv、shell、native handle 或自动升级字段。确认必须
先于 input、目标发现、权限检查和平台调用。

## 两级风险与平台边界

优雅 capability 的风险等级是 `high`，只向精确进程当前拥有的通用顶层窗口投递固定关闭
请求。没有可认证顶层窗口时返回 `CAPABILITY_UNSUPPORTED`；至少一个关闭请求被平台接受
后才建立 `accepted=true`。提示保存、拒绝关闭或无响应都由目标应用自行决定，工具不得
发送按键、点击按钮、调用软件专用协议或升级为强制终止。

强制 capability 的风险等级是 `critical`，只在重新核对同一进程代际、静态权限和保护
门禁后使用固定内核进程终止。退出码由内部固定且不进入公共协议；调用方不能指定信号或
退出码。平台接受后不得撤销、补偿或自动重试。

当前工具进程、Windows critical process、PID 0/4、其他 Windows 登录会话、元数据不可读、
高完整性或完整性未知目标均在 dispatch 前结构化拒绝。实现不得尝试提权、调试权限、服务
控制、驱动、远程注入或安全边界绕过。名称不用于授权，防止同名进程影响精确身份判断。

## 接受、最终状态与重新观察

平台接受只表示关闭请求已排队或内核终止已提交，不等于进程已经退出。Module 持有经代际
认证的私有进程句柄并有界等待其退出；只有确认同一句柄已 signaled 才返回
`finalStateReached=true`、`state=exited` 与 `sameProcessGenerationVerified=true`。成功
结果提供 `reobserveWith=process.discover@1` 和 `expectedObservation=target-absent`，调用方
仍可用原 opaque ID 重新执行只读发现或检查 stale。

事实建立点前取消或 deadline 返回确定的 `CANCELLED` 或 `TIMEOUT`。事实建立点后取消、
deadline、等待失败、权限变化、目标提示未完成或前景干扰都返回 `OUTCOME_UNKNOWN`，并
声明 `accepted=true`、`retrySafe=false`、`automaticRetryProhibited=true`。公共错误必须
保留原 opaque `targetId` 供重新观察，但不得泄漏 PID、句柄、路径、退出码或平台错误文本。

两种操作都不需要前景同意，也不主动激活窗口。成功路径必须保持宿主前景不变；优雅请求
导致目标或第三方改变前景时，结果只能是不可重试的 host-interference OutcomeUnknown。
