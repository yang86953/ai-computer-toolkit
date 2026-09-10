# Exact window lifecycle contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.lifecycle@1` 是 `restore`、`minimize`、`maximize`、`move` 和 `resize` 的
provider-neutral 同步 Command。不可逆内容关闭继续由独立的 `window.close@1` 拥有；
两者不得合并成一个按可选字段推断风险的命令。

## 输入和精确目标

调用只接受当前 `sessions app|window` 返回的 `s2:w:<opaque>`。System 必须在任何写入
前重新枚举并唯一解析同一当前窗口 token 与进程创建代际；零命中返回 `STALE_SESSION`，多命中
失败闭合。输入必须严格匹配 `window-lifecycle-input.schema.json`：每个请求恰好一个
动作，未知字段、HWND、PID、class、style、message 和 provider 字段均返回
`INVALID_ARGUMENT`。

全部五种动作都要求逐操作 confirmation 和预先 `allow-foreground`。这表示调用方授权
可见窗口状态或几何变化，不表示 Adapter 可以自行激活窗口、发送输入或改变焦点。
统一 Policy 必须先于 input、目标发现和平台调用评估这两个门禁。

## 坐标、DPI 与多显示器

`move` 和 `resize` 必须显式使用 `screen-physical-px`。它表示 Per-Monitor-V2 感知下的
虚拟桌面物理像素；虚拟桌面原点可以位于任意显示器，因此 `x`、`y` 是完整有符号
32 位值。`move.x/y` 是窗口外框左上角；`resize.width/height` 是窗口外框尺寸，不是
客户区、DIP 或缩放前逻辑单位。

Windows 私有 Adapter 在每次观察、校验和写入时建立 Per-Monitor-V2 上下文，读取目标
当前 DPI、虚拟桌面边界和该 DPI 下的系统 minimum tracking size。移动后的外框必须与
当前虚拟桌面至少相交一个物理像素；缩放必须不小于目标当前最小 tracking size，且不
大于虚拟桌面跨度。协议自身另以 1–65535 物理像素限制尺寸，防止在环境发现前接受无界
工作。DPI、最小尺寸、最终外框与用于校验的虚拟桌面边界只以 provider-neutral 数值进入
结果；公共 JSON 不出现任何 Windows 类型或标志。

不支持最小化、最大化或调整尺寸的窗口返回 `CAPABILITY_UNSUPPORTED`；坐标上下文无法
取得时返回 `COORDINATE_CONTEXT_UNAVAILABLE`；环境范围不满足时返回
`INVALID_ARGUMENT`。不得把不支持静默降级成前景键鼠或软件专用命令。

`act/window-target-identity/v1` 是该路线的身份强度权威。当前 token 变化或进程代际变化时可证明 stale，
但同一进程内完全相同窗口 token 被回收时，公开 Windows 事实不能提供窗口创建代际；因此当前短命
launcher 不得被描述为绝对 no-rebind。每个窗口 discovery/assessment 公开该缺口；异步 WinEvent
owner 也不能把最后 resolve 与窗口 API 调用原子绑定，因此一般 `window-recreated` 保持 gap；
confirmation、前景同意和最终读回均不能替代 lifetime-bound dispatch。

## 接受、最终状态和恢复责任

状态动作只发送固定系统命令，几何动作只使用不激活、不改变 Z 序的窗口定位调用。
平台已接收调用是 `accepted=true` 的事实建立点，但不等于目标已达到最终状态。Module
必须有界轮询经当前 token 与进程代际核对的目标：成功结果同时声明 `accepted=true`、
`finalStateReached=true`，并返回最终状态、外框、DPI 和最小尺寸。
成功结果必须携带 `targetIdentityStrength`，不得再用 `sameWindowGenerationVerified=true`
掩盖完全相同 token 回收的未解决缺口。

在事实建立点前取消或超时可以返回确定的 `CANCELLED` 或 `TIMEOUT`。事实建立点后出现
取消、超时、目标消失、读回失败或未授权前景变化时，结果只能是 `OUTCOME_UNKNOWN`，
并声明 `accepted=true`、`retrySafe=false`、`automaticRetryProhibited=true`。实现不得
自动重试、回滚窗口状态、重新激活目标或发送补偿输入。

若动作前目标不是前景窗口，执行期间前景只能保持原值或转移到同一个精确目标；转移到
任何第三方窗口都返回 host interference 的 OutcomeUnknown。若动作前目标就是前景窗口，
只有最小化允许 shell 选择后续前景；其他动作发生第三方前景变化仍视为干扰。成功结果
始终如实报告 `foregroundChangedDuringDispatch` 与 `meta.foreground.unchanged`。
