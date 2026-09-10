# Provider-neutral semantic element action v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`ui.element.action@1` 是统一 `app.apply` surface 上的确认式 mutation。调用方只能提供
原始 `s2:w:*` 精确窗口目标、GC-LOC-001 的 provider-neutral selector 和五种封闭动作：
`invoke`、`value`、`toggle`、`select`、`scroll`。`s2:e:*` 仅是定位快照，既不是授权
token，也不能作为本 capability 的目标。

## 输入与动作

输入由 `ui-element-action-input.schema.json` 冻结。selector 的 `name`、
`automationId`、`className`、`frameworkId`、`controlType` 使用精确 AND 语义；
`maximumDepth`、`maximumItems`、`view` 和 `timeoutMs` 保持硬边界。动作形状如下：

- `{"type":"invoke"}`：调用唯一默认动作；
- `{"type":"value","value":"..."}`：设置 Value，空串可用于清空；
- `{"type":"toggle"}`：切换状态；
- `{"type":"select"}`：选择当前项；
- `{"type":"scroll","horizontal":"...","vertical":"..."}`：至少一个轴必须不是
  `no-amount`，其余相对量为 `large-decrement`、`small-decrement`、
  `large-increment`、`small-increment`。

任何 HWND、PID、COM/UIA pattern、RuntimeId、坐标、点击点、XPath、CSS selector、
应用私有命令或任意 provider 参数都必须在 dispatch 前失败。元素或 provider 不支持
请求动作时返回 `ACTION_UNSUPPORTED`，绝不静默改走指针输入；Workflow 只有在收到该
显式结果后，才可另建一项经自身 capability、确认和前台同意门禁的指针步骤。

## 执行门禁与 SMC 边界

`ComputerControlSystem` 只执行 Policy、freeze、provider 协调和结果 attestation。
Semantic Action Module 在启动 worker 前依次要求：

1. capability 与 `app.apply` verb 匹配，执行域为 `same-session-no-focus`；
2. 逐操作 `confirmed=true`，且确认先于 input、target 和 provider 解析；
3. 只读 capability assessment 重新确认权限关系、当前发布集合和无 fallback 约束；
4. 原 `s2:w:*` 在主进程中重新解析为唯一当前窗口，并记录前景不变量；
5. 独立 Job-bounded worker 再次解析窗口，以原 selector 完整证明唯一元素，检查
   enabled 与动作 pattern 后执行恰好一次调用。

UIA 公共 surface 与 `act/observation-worker/v1` 仍是只读的。UIA、COM、HWND、pattern
接口和 HRESULT 只存在于 Semantic Action Worker 私有 Component 内，不进入稳定 JSON。
该 Command 是 D3/R0：改变当前交互会话状态，不自动重试，也不发布 EventBus 事件。

完整搜索为零时返回 `ELEMENT_NOT_FOUND`，唯一元素未启用时返回
`ELEMENT_NOT_ENABLED`；多匹配、搜索不完整、窗口失效和权限拒绝继续分别使用
`AMBIGUOUS_TARGET`、`SEARCH_INCOMPLETE`、`STALE_SESSION` 与 `PERMISSION_DENIED`。
这些失败都发生在动作 dispatch 前；不会把“未找到”当作成功 no-op。

## 结果与不确定性

成功结果由 `ui-element-action.schema.json` 冻结，只回显动作类别、完整搜索数量、窗口与
元素均已重解析、确认/权限/前景门禁在 dispatch 前完成、前景未变化、未使用指针回退，
以及 `outcome=completed`、`retrySafe=false`、`automaticRetryProhibited=true`。
不会返回 Value 内容、Toggle 状态、选中状态、滚动百分比或动作后的 UIA 快照。

动作方法一旦开始，provider 异常必须返回 `OUTCOME_UNKNOWN`。parent 不能证明 worker
未启动的启动错误、timeout、cancel、等待失败、输出超限、stderr/framing/JSON/退出码
冲突也必须收敛为相同语义。其 details 固定包含：

- `outcome=unknown`；
- `dispatchState=accepted-may-have-occurred`；
- `acceptedMayHaveOccurred=true`；
- `automaticRetryProhibited=true`；
- `retrySafe=false`；
- `pointerFallbackUsed=false`；
- 当前可得的 `foregroundUnchanged` 布尔证据。

调用方必须先重新观察，再由人或显式 Workflow 决定后续操作；不得根据 HRESULT、超时、
取消或错误文本自动重复 mutation。严格零打扰请求会在 Policy 中以
`ISOLATION_REQUIRED` 拒绝 `same-session-no-focus`，不会因实现使用 Job worker 就伪装成
无主机会话影响的 `isolated-worker`。
