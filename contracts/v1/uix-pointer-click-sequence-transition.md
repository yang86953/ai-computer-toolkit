# `ui.input.pointer.click.sequence.transition@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

这是一次应用内普通左键点击序列与提交后语义条件的绑定。它只面向已认证的
UIX 应用窗口，执行域为 `same-session-no-focus`，不控制宿主桌面或任意第三方窗口。

## 输入

正式输入由 `uix-pointer-click-sequence-transition-input.schema.json` 冻结，根对象严格
封闭，只接受 `coordinateSpace`、`clicks`、可选 `intervalMs`、可选 `timeoutMs` 和必填
`postcondition`：

- `coordinateSpace` 必须是 `client-logical-px`；每个点仅有 finite 的 `x`、`y`，范围为
  `0..65535`；
- `clicks` 为 `1..64` 个点，provider-neutral；每个点映射为普通左键 `click_at`，不公开
  独立 down/up、按钮所有权、双击或 click count；
- `intervalMs` 为 `0..500` 毫秒，默认 `0`；计划时长为
  `(clicks.length - 1) * intervalMs`，且不超过 `5000` 毫秒；
- `timeoutMs` 为 `100..30000` 毫秒，默认 `30000`，且至少为计划时长加 `100` 毫秒；
- `postcondition.selector` 复用 exact-AND selector，至少包含一个有效字段；condition 只能是
  `unique` 或 `missing`。

未知字段、任意显式 `null`、浮点时间、越界坐标、空序列、越界时序、空 selector、其他
坐标空间或其他 postcondition 都必须在 provider I/O 前失败。Component 只暴露已验证的
点击点、计数、时序和 postcondition，不回显非法原始 JSON。

## 确认、dispatch 与观察

Module 必须 confirmation-first：确认先于输入解析、目标解析和 provider I/O。确认后只
resolve 一次，建立一个认证 Agent 连接，并在固定 window generation 上按序发送完整点击
序列；同一连接维护连续 revision。普通左键 click 是应用内部配对的 PointerDown + PointerUp，
但契约不提供独立按钮操作。

所有点击必须在首个 dispatch 前完成 `perform`、`snapshot`、`wait` 与实际所需
`click_at` 的协议/动作预检。动作完成后，以序列动作 revision 为基线取得 snapshot；必要时
使用同连接的 semantic revision-after wait，再取得 snapshot，检查 exact-AND 的 `unique` 或
`missing`。`sequenceSettled=false` 仍可成功，只表示该动作尚未被声明为全局最终 settled。

成功只证明完整点击序列已被 Agent 接受，并且 dispatch 后观察到当前语义条件匹配；不证明
点击与条件之间存在因果关系，不证明应用消费、最终 UI 状态或桌面指针状态。结果固定
`allClicksBalanced=true`、`transactionSemantics=false`、`rollbackSemantics=false`，也不
声明事务或回滚能力。

首个 dispatch 前的协议、权限、目标或输入错误可以精确失败。任一 dispatch 开始后出现
连接关闭、timeout、stale、协议异常、部分接受、后置条件歧义或无法证明终态，必须保留
`acceptedMayHaveOccurred`/`OUTCOME_UNKNOWN` 或 `AMBIGUOUS_TARGET` 语义并禁止自动重试。
业务失败不得冒充成功，也不得冒充部分点击已安全完成。

## 安全与版本边界

本能力只使用 `uix-agent-v1` 公开的应用内 `click_at`，不请求前景，不注入桌面输入，不
移动桌面指针，不使用独立按钮、双击、click count、drag、scroll、原生/传输身份、X11/
XWayland、compositor 私有协议或 fallback。结果中的 element 仅为脱敏的 snapshot-revision
语义投影。

契约以 `uix-app v0.0.2` 的公开同连接 action revision、semantic snapshot 和 revision-after
wait 语义为基础，本批不修改 uix-app。用户真实交互验收暂缓；静态契约通过不等同于真实
应用消费或最终 UI 验收。
