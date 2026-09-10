# 精确协作式语义元素动作 v2

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.element.action@2` 通过统一 `app.apply` surface 控制显式启用 Agent 的协作式应用。
它不会改写基于 Windows UIA selector 的 `ui.element.action@1`，也不把 UIX 原生节点、
端点、进程、token 或一次性确认 ID 暴露到稳定 JSON。

调用方先用 `accessibility.tree.read@3` 获取 `snapshotId` 与 `elementId`，再提交版本二
输入。Module 会在已确认后重新读取当前快照；快照或元素身份不一致时返回
`STALE_ELEMENT`，不会把旧元素静默解析为新节点。动作只允许 `invoke`、`focus`、
`set-value`、`insert-text`、`select`、`toggle`、`increment`、`decrement` 和 `scroll`。

例如把以下对象保存为输入文件：

```json
{
  "snapshotId": "as3:0123456789abcdef",
  "elementId": "s2:e:0123456789abcdef",
  "action": { "type": "invoke" },
  "timeoutMs": 30000
}
```

再执行：

```text
ai-computer-toolkit run app apply --capability ui.element.action@2 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```

确认顺序固定为：工具包逐操作确认、有界 JSON 读取、参数与精确目标解析、认证端点连接、UIX perform。
若应用策略还要求用户确认，Adapter 在同一认证连接内使用私有 `confirm_id` 完成应用确认；
该身份不进入结果、日志或下一次公开请求。用户拒绝与确认失效分别返回
`CONFIRMATION_REJECTED` 和 `CONFIRMATION_EXPIRED`。

执行域是 `same-session-no-focus`：UI turn 内的控件焦点或内容可以改变，但工具包不请求
主机窗口激活，不发送桌面键盘或指针输入，也不回退 X11、XWayland 或其他输入机制。
严格隔离请求在 provider 前以 `ISOLATION_REQUIRED` 失败。

动作帧发出后若 transport、超时或响应协议无法给出可信终态，必须返回
`OUTCOME_UNKNOWN`，并声明 `automaticRetryProhibited=true`、`retrySafe=false`。
调用方必须重新读取可访问性树后再决策，不能自动重复 mutation。
