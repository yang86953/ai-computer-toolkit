# 非图像后台任务控制 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 范围与入口

本协议给可信任务启动端提供长存活的 CLI 控制通道，不增加 GUI、第二个 LLM 或自动规划器。
宿主使用项目登记的 UIX `0.0.2` 不可变提交 `323d4fa2d485288b545602fe75d15a0229ac6a4f` 的
`AppMode::CLI`、`Cli` 与 `App::cli`。不引用相邻框架工作树或可移动版本标签。
该固定版基础输入控件直接依赖 `feedback`，故消费配置为 `default-features=false, features=["feedback"]`；
不启用图形 backend 默认集合。框架 feature 裁剪缺陷需要在上游空闲后独立修复，不在本仓复制框架逻辑。

```text
ai-computer-toolkit serve --stdio --grant-file=/absolute/path/task-grant.json
```

新入口只接受以上两个选项，`--grant-file` 使用等号形式。参数错误也输出 JSON，不把 UIX 的人类帮助文本
混进机器通道。其他命令和隐藏 worker 路由继续使用现有 v1 单结果 CLI，确认和隔离语义不变。

## 授权来自启动端，而不是应用内容

启动端须从用户已经分配的任务建立授权，必要时先用只读发现取得精确目标，再在不重复询问的情况下物化范围。
授权文件不是凭据，也不是允许任意本地调用者提权的接口；能够选择或修改启动参数的主体本来就在可信边界内。
同一普通请求流、网页文本、应用返回值、模型自行添加的 `confirmed=true` 均不能建立或扩大授权。

授权见 [task-grant.schema.json](task-grant.schema.json)：

- 固定 `act/task-grant/v1`、启动端生成的 `t2:<32 lowercase hex>`、`authorizationSource=user-task`。
- 单个任务、固定单调期限 `1..1920000 ms`、最多 `1..64` 次执行尝试。
- 最多 64 个唯一的 capability × exact target 权限项；不接受通配符、PID、HWND 或执行文件选择。
- `requiredInput` 对最终输入施加递归对象包含条件；数组和标量必须完全相等。未限定部分仍必须通过 capability 自身
  的封闭输入契约。启动端负责为任务中的收件人、文档、资源和破坏性操作提供足够窄的约束。
- `allowDiscovery` 缺省 false。打开后不能用后续帧修改授权、换任务、重置期限或清空执行账本。
- Discovery capability 不进入精确目标 permissions；它通过 `allowDiscovery` 授权，因为目录发现尚无可绑定的目标。

文件硬上限 64 KiB，只读取有界普通文件。Linux 还要求同一有效 UID、组和其他用户不可写，并拒绝符号链接。
Windows 拒绝 reparse point；可信启动端负责将授权来源置于自身受保护的 ACL 边界内。本协议不声称隔离同 UID 的
任意恶意进程，也不把一个自报 `user-task` 的网络消息当成人类授权。

## 传输与操作

请求见 [task-control-request.schema.json](task-control-request.schema.json)。stdin/stdout 为 UTF-8 JSON Lines，
每帧最多 65536 字节（包含换行），每个 JSON 必须完整。stdout 只输出协议响应，输出后立即 flush。
响应见 [task-control-response.schema.json](task-control-response.schema.json)。无效关联 ID 不回显为有效收据。
宿主退出码 0 只表示通道正常结束；调用端必须逐条检查 `ok`、execution outcome 与验证事实，不能把退出码或
`task.close` 当作业务任务已成功完成。启动、语法、传输或宿主期限失败退出 2。

```json
{"contractVersion":"act/control/v2","requestId":"open-1","taskId":"t2:0123456789abcdef0123456789abcdef","operation":{"type":"task.open"}}
```

| operation.type | 语义 |
|---|---|
| `task.open` | 激活启动时已经冻结的授权，不从帧中接收授权 |
| `task.status` | 返回状态、取消请求、是否关闭/过期、剩余期限和预算计数 |
| `task.cancel` | 提前传播取消意图，停止后续派发；响应不是回滚证明 |
| `task.close` | 结束该宿主任务，不关闭未经授权的第三方应用 |
| `catalog` | 返回 App capability 与严格后台资格，不把 registry 登记等同可执行 |
| `discover` | 可选 `scope=applications|media`；默认 applications 保留固定 64 项分类上限，media 通过生产 facade/worker 有界列出最多 128 个媒体目标；两者都要求 allowDiscovery |
| `assess` | 对授权内 capability/target 检查资格；未知条件失败闭合 |
| `execute` | 最终输入先经过任务授权，再经既有 System/Policy/固定 worker 执行 |

`execute.postconditions` 复用 sequence 的 `exists`/`equals`：最多 16 条，RFC 6901 Pointer 最多 256 UTF-8 字节，
`equals.expected` 的紧凑 JSON 最多 4096 字节。不能提交脚本、CSS、XPath、raw CDP 或自选 worker。

输入待处理队列最多 16 帧；溢出会请求取消并终止通道，不无界排队。正常 EOF 表示输入结束，已入队帧依序处理，
随后宿主退出，不留下后台任务。异常输入和输出断开终止该通道。需要继续读取和控制的调用端应保持管道开放。
整个 CLI 进程只承载一个任务；stdin 读取线程和信号状态不在多个任务间复用。

## 严格后台与最小化资格

要求同时满足：无图像依赖、不注入宿主输入、不写宿主剪贴板、不激活或还原窗口、无前台 fallback，
并且目标最小化时仍可用，或该路线本来就不依赖用户窗口。不可避免的逐动作人类交互同样不合格。

每一项资格是 `supported`、`unknown` 或窗口无关等明确事实；不能从 `same-session-no-focus` 推导最小化支持。
既有 v1 的 strict 只接受 `host-headless/isolated-worker`，与本协议的最小化资格不是同一个概念。
任务授权只替代重复确认，不取消权限、身份、状态和 Provider 的其他拒绝条件。

Linux 新增 [MPRIS App v3](../v3/linux-mpris-media.md) 的窗口无关媒体发现、状态读取和带独立观察验证的 play/pause/stop。
`catalog.guarantees.routeEligible` 只证明固定路线，`eligible=false/assessmentRequired=true` 直到当前目标被重新评估；
媒体 assess 使用有界 sequence 读取授权目标的公开状态，结合 Can* 和 grant 的 operation 限制判断当前资格。
评估不是执行预留，也未验证尚未提交的完整 input；每次执行仍重新核验 input、身份与状态。
Windows 仍仅含原有固定 Broker 自有隔离浏览器的非图像导航、等待、查询、点击和输入候选；不代表用户现有浏览器
或 Windows 主机实测。其他 Linux 路线保持 unknown 并失败闭合。Provider 实测证据必须另见实施状态。

## 接受、终态与验证

执行复用现有 `AppControlService::sequence` 和固定 step worker：Linux 独立 process group，Windows sibling/Job。
单步 deadline 不超过 30 秒，且不晚于任务剩余期限。取消与总截止时间不等待执行线程返回才发布；worker 的
`not-dispatched/completed/failed/unknown`、accepted 与 final 事实原样保留在 `workflow` 证据中。

- `completed` 表示取得 Provider 可信完成事实，不等于业务已经持久化。
- 断言失败不能把已完成操作改成 `not-dispatched`。其错误为 `TASK_VERIFICATION_FAILED`，保留实际执行 outcome。
- `OUTCOME_UNKNOWN` 禁止自动重派；变更动作进入未知状态后，只允许授权内的观察/核查，不能继续写操作。
- 结果断言只证明 Provider 结果满足请求条件；`domainCommitVerified` 仍为 false，不把“点击成功”写成“订单已提交”。
- 取消被请求不等于已经取消，更不等于已回滚；若缺少 final，保持 unknown。

媒体 discovery/assessment 与 execute 均由既有 sequence worker 施加剩余任务预算。其他 discovery/assessment 仍走
既有只读 Module，其内部 I/O 没有被重写成本协议的新硬实时保证。

## 去重、预算和恢复

一个会话最多保存 256 条请求收据，含规范化请求和有界响应。同一 requestId/内容返回既有收据；同 ID 不同意图
返回 `REQUEST_ID_CONFLICT`。输入身份检查先于取消信号，冲突取消不能先产生副作用再被拒绝。
管理操作不负责清空账本；容量耗尽时仍保留已有收据并允许结束任务。

普通结果累计预算 1 MiB，超限保留紧凑的省略/执行事实和禁止重派标记；控制及预算错误元数据不冒充 Provider
负载。执行账本和输入身份表都有固定容量，不为腾空间忘记旧变更请求。结果过大不是再次执行的理由。

本阶段没有 durable resume 或跨进程 exactly-once。宿主断开且缺少 final 时，启动端必须先在新只读授权中核查实际
状态，不能凭新进程或新 requestId 自动重做变更。任务完成/未知/取消事实与审计持久化是独立交付项，不伪造已完成。
