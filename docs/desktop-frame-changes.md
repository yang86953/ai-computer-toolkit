# 按场景使用桌面单帧／区域差分

本文描述按请求执行的 `observe` 差分；持续采集与增量 PNG 交付见 [常驻桌面帧订阅与增量捕获](desktop-frame-subscription.md)。

Linux 常驻桌面 broker 在原 `observe` 请求上增加可选 `changeDetection`。复用已有 Portal 授权、lease、超时、原子 PNG 输出和 nonce 重放边界；不增加输入权限，不建立第二条捕获通道。

## 能力与选择

读取本次宿主 `broker-ready`：`observationModes` 为 `["snapshot", "frame-diff", "region-diff"]`，`maxTrackedPixels` 为 8388608。旧宿主没有这两个字段时不要发送新选项；仍可按预算执行普通 `observe`。

- 应用能提供语义状态或完成事件：优先使用对应应用接口，此接口不替代语义终态。
- 静态、低频查看：不带 `changeDetection`（或设为 null），保持普通单帧输出，也释放该会话的差分基准。
- 需要知道两次观察之间哪里变化：使用 `changeDetection`，不带 `region` 比较全帧。
- 等待提示、监控局部画布：带 `region`，只统计该区域；阈值可过滤小幅像素噪声。动画若位于目标区域仍会触发变化，需上层理解。

这是**全帧采集后的 RGBA 差分**，不是 PipeWire damage 元数据、常驻帧订阅或原生增量传输。每次请求仍采集并编码、写出完整 PNG；仅可减少上层无变化时的图像分析，尚不节省采集与 PNG 编码成本。不会自动逐帧调用模型或自行发送键鼠。

## 请求

通过现有 `session-call desktop --input <file|->` 调用；示例中的身份和路径须替换为当前真实值。

第一次建立基准：

```json
{
  "contractVersion": "act/linux-desktop-session-broker/v1",
  "brokerEpoch": "<broker-ready 的 epoch>",
  "requestNonce": "<新的 32 位小写十六进制 nonce>",
  "operation": "observe",
  "sessionId": "<open 返回的 sessionId>",
  "confirmed": true,
  "strictIsolation": false,
  "input": {"path": "/absolute/path/baseline.png", "maxDimension": 1400, "timeoutMs": 5000},
  "changeDetection": {}
}
```

下一次请求使用新的 nonce 和输出路径，在同会话内引用上次成功返回的 `data.frameId`：

```json
"changeDetection": {
  "baselineFrameId": "<上次跟踪观察返回的 frameId>",
  "region": {"x": 100, "y": 80, "width": 400, "height": 200},
  "pixelThreshold": 3,
  "minChangedPixels": 10
}
```

`changeDetection` 是 **observe 顶层字段**，不是 `input` 字段；不用于 `capture-frame` 或 `interact` 内的截图参数。是否存在输入后截图组合扩展与此能力无关；普通输入后可独立调用本接口。

坐标是本次输出的 `observation-px`，不是源桌面坐标。区域需完全在图内，不自动裁剪。`pixelThreshold` 为 0..255（默认 0）：RGBA 任意一个通道差值**严格大于**阈值才记为变化像素。`minChangedPixels` 为 1..8388608（默认 1），仅控制 `changed` 布尔值，不隐藏实际变化计数及边界；值大于比较区域面积时永远不会触发。region/baselineFrameId 为 null 时等同省略。

## 返回与生命周期

原观察结果不变，启用时增加 `data.changes`：

```json
{
  "status": "compared",
  "changed": true,
  "changedPixels": 24,
  "comparedPixels": 80000,
  "changedBounds": {"x": 120, "y": 95, "width": 16, "height": 3},
  "region": {"x": 100, "y": 80, "width": 400, "height": 200},
  "pixelThreshold": 3,
  "minChangedPixels": 10,
  "baselineFrameId": "<被比较的旧 frameId>",
  "coordinateSpace": "observation-px",
  "method": "full-frame-rgba-diff",
  "effectConfirmed": false
}
```

`changedBounds` 是所有超过像素阈值的变化像素的最小包围矩形，坐标相对完整观察图；无变化时为 null。不是每个脏矩形的列表，也不包含裁剪图。

- 省略基准身份：`status=baseline-reset`、`reason=baseline-created`、`changed=null`。此次帧成为新基准，不把初始化伪装成“无变化”。
- 源帧尺寸、输出尺寸或已知输入映射代际变化：`baseline-reset`，reason 为 `dimensions-changed` 或 `mapping-changed`；不比较不可对齐的像素。重新核对目标与区域。映射不可用时只能校验图像尺寸，不承诺检测所有布局、焦点或窗口切换。
- 每次成功的跟踪观察替换上次基准；比较的是相邻请求帧，不能引用任意历史帧。若需要相对固定旧帧的累积变化，当前接口不支持。普通观察、关闭会话或 broker 退出释放缓存。
- 基准过期、跨会话或对应帧未缓存：`STALE_OBSERVATION`，在采集之前拒绝。收到响应丢失或基准失效后可省略 baselineFrameId 明确重建，不重放键鼠。
- 每会话最多缓存 8388608 像素，即 32 MiB RGBA（8 会话最多 256 MiB **基准缓存**，不包含在途采集、PNG 和当前帧）。超限仍返回普通截图，并给出 `status=unavailable`、`reason=tracking-limit`、`changed=null`，不缓存该帧。下一次降低 `maxDimension` 并省略基准身份重新建立。
- 参数、基准校验失败不消费帧；开始新观察后，捕获、输出或后置映射检查失败会使旧观察失效。若新图尺寸变化导致区域越界，可能已写出 PNG；错误附 `capturedPath` 与 `pixelsMayHaveBeenConsumed=true`，不返回可继续引用的观察身份。
- 同 nonce 同请求返回原响应，不重采集、不推进基准；同 nonce 改选项被拒绝。重放不会让旧 frameId 重新变为最新，文件也可能已被外部删除或覆盖。

## 上层有界等待

`建立基准 → 等待任务适合的间隔 → 用新 nonce 比较 → 检查 status → changed=true 时核验预期内容`。没有变化时可跳过模型图像分析，但必须按返回的最新 frameId 更新基准。每次均是完整观察，仍受原请求账本配额约束。

由调用方控制整体截止时间、采样间隔、最大请求数和取消；普通 `observe` **不提供服务端 wait-change 或订阅生命周期**；常驻订阅使用独立的 `observe-subscribe/observe-next/observe-unsubscribe`，不要混用两种基准身份。单次采集沿用 1..30 秒超时，差分在取得帧后同步执行，采集超时不等于整个观察端到端期限。两次采样之间的短暂提示可能漏掉；不承诺实时拖拽闭环或无丢帧。

视觉变化只触发验证；`effectConfirmed` 始终 false。画面无变化、稳定或成功输出 PNG 均不证明应用任务完成。完成、超时或用户取消后停止后续输入，按现有会话协议收尾。

## 验证入口

```bash
cargo test --locked --lib desktop_frame_changes
cargo test --locked --lib linux_desktop_session_broker::change_tests
cargo test --locked --test linux_desktop_session_contract
```

前两项覆盖像素算法以及通过 broker 契约执行的模拟 lease；第三项包含真实 launcher 协议检查。它们不代表真实 Portal 桌面采集、交互速度或 GUI 操作成功的验收。
