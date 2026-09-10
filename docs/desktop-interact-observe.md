# 桌面动作后观察：一次请求返回键鼠结果与截图

在常驻 `session-host desktop --socket` 上复用已授权的 Portal 会话，通过原 `session-call desktop --input -` 调用。无需新增进程协议或重连授权；原 `interact` 请求不带 `observation` 时行为不变。

```json
{
  "contractVersion": "act/linux-desktop-session-broker/v1",
  "brokerEpoch": "<本次 broker-ready 返回的 epoch>",
  "requestNonce": "<本次请求新生成的 32 位小写十六进制 nonce>",
  "operation": "interact",
  "sessionId": "<open 返回的 sessionId>",
  "confirmed": true,
  "foregroundConsent": true,
  "strictIsolation": false,
  "input": {
    "timeoutMs": 3000,
    "steps": [
      { "type": "key", "keys": ["numpad-1"] },
      { "type": "wait", "ms": 300 }
    ]
  },
  "observation": {
    "path": "/absolute/path/front.png",
    "maxDimension": 1400,
    "timeoutMs": 5000
  }
}
```

上述占位身份必须替换为真实返回值，确认标志仅表达已有授权。点击仍需在 `input.frameId` 中引用同会话最新观察，使用该观察图的坐标，不是原始桌面分辨率坐标。

## 返回与失败语义

- 成功：保留 `data.completedSteps`、`inputEventsSent` 等原字段，新增 `data.observation`（`path`、`frameId`、`coordinateSpace: observation-px`、`width`、`height`、`pointAvailable` 等）。读取 PNG 后核验界面，再决定下一批动作。
- `effectConfirmed` 始终为 false。截图是输入之后采集的帧，不保证应用已处理完输入或画面已经刷新。示例中的等待只用于界面刷新，不构成应用完成确认。
- 截图参数、输出 guard 和 staging 可写性在输入前预检；实际截图再次检查、原子提交。预检失败：`error.details.failedStage=observation-preflight`，`interactionAttempted=false`，无键鼠发送。
- 输入失败：`failedStage=interaction`、`observationAttempted=false`，保留原输入错误中的完成步数、事件数、未知结果与取消信息。不继续截图或重放。
- 输入成功但截图失败：`failedStage=observation`、`interactionCompleted=true`，`error.details.interaction` 保存完成的输入报告。整体 `completed=false`、`acceptedMayHaveOccurred=true`、`automaticRetryProhibited=true`。需要时单独 `observe`，不得重新发送整批动作。
- 同 nonce 同请求只重放原响应，不再次输入或截图；同 nonce 改参数被拒绝。重放中的文件路径不保证文件仍存在。
- `input-cancel` 继续绑定整个请求 nonce；输入阶段按原检查点取消，输入后开始截图前再次检查。截图开始后没有新加可中断截图机制，仍按原 capture 超时收尾。不得将“发送了取消”当作“已取消”。
- 输入与截图各自最长 30 秒，组合请求客户端响应上限为 65 秒，避免沿用原 35 秒导致后半程断线。仍不自动重试超时请求。

## 帧寿命与等待预算

- `frameId` 一次性：输入送出即失效，必须先重新观察。**预检拒绝不消耗帧**——等待超预算、字段非法这类在投递前被拒的请求，`acceptedMayHaveOccurred` 为 false，没有发出任何事件，观察结论仍然成立，修正参数后可用原帧重发。结果未知或已投递的失败保持失效，不自动重放。
- `steps` 内所有 `wait` 的毫秒之和必须严格小于本次请求的 `timeoutMs`（MCP 客户端默认 3000ms），单步 `wait` 上限 1000ms；按键、文本、点击与移动都不计入这条预算。超限整批拒收、不部分执行，错误信息给出实际合计与 `timeoutMs`，需要更长停顿就抬 `timeoutMs`，或者把批次拆小。
- 单批步数上限 648（`contracts/v1/desktop-interaction.schema.json` 与 `desktop_interaction` 的同一常量，MCP schema 直接引用它，避免两处再漂移）。wait 合计只受本批 `timeoutMs` 约束——`timeoutMs` 已封顶 30000ms，不再叠一层独立的等待上限，否则大批次装得下步骤却装不下停顿。
- 另一条独立预算是展开后的平台工作单元：点击 3、移动 1、按键每键 2、文本每字符 4，单批上限 16384。所以「648 步」对文本密集批次不成立，超限会报出实际单元数并要求拆批。
- **坐标步有实测天花板，且超出是破坏性的**：坐标步逐个派发，每次派发都要 start-emulating → 发帧 → stop-emulating。在本机 KWin/libei 上实测单个请求连续派发约 90 次绝对指针后 EIS 连接断开，返回 `OUTCOME_UNKNOWN`、`stage=stop-emulating`、`releasesConfirmed=false`，并作废该会话；60 与 65 次稳定通过。这不是步数上限造成的（旧的 128 上限同样会踩），但意味着「648 步」对纯坐标批次不可用：坐标密集的活儿按 ≤64 步拆批，用 `computer_run` 串多批，而不是把坐标步堆进一批。
- 键盘与快捷键只送达持有焦点的窗口。批量发键前先用点击确认焦点在目标窗口，并留意同机其他窗口（包括用户正在操作的窗口）随时可能抢走焦点。

## 推荐连续执行链

`观察 → 确认目标/焦点 → 小批次 interact + observation → 读取返回 PNG → 核验`

会话和宿主在同一建模任务内复用，出现弹窗、焦点变化或结果不明时停止后续写请求并观察。结束按 `close → sessions（空）→ shutdown` 收尾。截图只获取当前任务需要的分辨率，不额外绕过 Portal 授权。
