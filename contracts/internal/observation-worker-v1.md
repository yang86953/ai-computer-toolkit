# 隔离观察 worker 协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

协议名：`act/observation-worker/v1`。

这是主 CLI 与同目录 companion worker 之间的内部稳定边界，不是绕过公开
capability 的第二控制面。

## 传输与生命周期

- stdin 接受一个单行 JSON request，stdout 返回一个单行 JSON result；
- stdout 不得混入诊断；stderr 非空视为协议失败；
- worker 必须处于 `KILL_ON_JOB_CLOSE` Windows Job；
- 主进程对 deadline 和 cancellation 轮询，触发后终止 Job、等待退出、回收管道；
- 主 worker 自行退出后，主进程必须先终止仍留在 Job 内的子孙进程，再等待
  stdout/stderr EOF，禁止继承管道把协议回收拖到 descendant 自行结束；
- 任一路径只有在主 worker 与 Job 子孙均已终止后才能向上层返回；
- worker 只接受 opaque `s2:w:*` session，内部重新枚举并检测 stale；
- request/response 不包含 HWND、PID、路径、COM/UIA 对象或 provider identity。

## Request

共同字段：

```json
{
  "contractVersion": "act/observation-worker/v1",
  "operation": "accessibility-root",
  "sessionId": "s2:w:<opaque>"
}
```

`accessibility-tree` 另带 `maximumDepth`、`maximumItems` 和
`view: control|raw`。`accessibility-element-locate` 另带相同有界字段和只含
`name|automationId|className|frameworkId|controlType` 的 provider-neutral selector；
多字段使用精确 AND 语义。未知 operation 必须关闭为 `CAPABILITY_GAP`。

## Result

成功结果包含 `ok: true`、完全相同的 `contractVersion` 和 `data`；失败包含
`ok: false`、`contractVersion` 与结构化 `error.code/message`。worker 自身不产生
`TIMEOUT`/`CANCELLED`；它们由主进程在确认 worker 已终止后生成。

Vikunja #1982 将 worker 协议入口自身直接产生的失败收敛到普通私有
`ObservationWorkerErrorCode`；失败 envelope 仍只包含稳定 `code` 与安全 `message`：

- `INVALID_ARGUMENT`：请求 JSON、协议版本、canonical 目标、树边界、防御字段或 stdin
  边界无效；
- `CAPABILITY_GAP`：operation 不在固定只读集合；
- `STALE_SESSION`：opaque 窗口不再唯一解析，或 UIA provider 报告 element 已过期；
- `AMBIGUOUS_TARGET`：opaque 窗口重新解析为多个候选；
- `PERMISSION_DENIED`：UIA provider 明确返回权限拒绝；
- `ACCESSIBILITY_UNAVAILABLE`：其他 COM 或 UIA provider 调用当前不可用。

上述六项只归 worker 协议边界所有。Windows inventory、opaque ID Component 与其他下层
错误继续原样传播，再由父 Accessibility Module 的既有白名单与公开转换失败闭合。十四个
直接 `AppControlError` 构造点归零，但 HRESULT 分类、退出码与 envelope 形状均不改变。

worker 仅可调用 UIA 元数据和 TreeWalker 读取接口。焦点、激活、键鼠、剪贴板、
Invoke/Value/Text pattern 和权限提升均禁止。定位 operation 使用独立八属性 cache，
仅比树读取增加 BoundingRectangle，并且只对唯一候选调用 GetClickablePoint；它在调用
期间使用 Per-Monitor-V2 线程上下文和只读最小化状态查询。返回坐标必须是虚拟桌面物理
屏幕像素，UIA element 与 HWND 仍不得越过 worker 边界。
