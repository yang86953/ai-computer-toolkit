# 常驻桌面帧订阅与增量捕获

Linux 桌面 broker 在既有 Portal 会话上增加 `observe-subscribe`、`observe-next`、`observe-unsubscribe`。先读取本次 `broker-ready`：`frameSubscriptions=true`、`subscriptionDelivery=latest-pull` 才能使用本协议。能力公布不代表 Portal 已授权、源已可用或 damage 已协商成功。

## 与普通 observe 的区别

| 方式 | 采集 | 图像输出 | 适合场景 |
| --- | --- | --- | --- |
| 普通 `observe` | 每请求取单帧 | 完整 PNG | 低频检查、输入定位 |
| `observe` + `changeDetection` | 每请求全帧读取并差分 | 完整 PNG + 差分事实 | 有目标区域、需要阈值过滤的离散检查 |
| 常驻订阅，有连续 Header + VideoDamage | 一条 PipeWire stream 持续消费，仅归一化 damage 包围区域 | 按需关键帧或裁剪 PNG 补丁，无变化不编码 | 连续画布、滚动、及时视觉反馈 |
| 常驻订阅，无可信 damage | 仍复用同一 stream，全帧读取后差分 | 同样可输出区域补丁 | 后端不支持 damage 时的明确降级 |

这里的订阅是 **producer 常驻，consumer 按需取最新状态**：没有 `observe-next` 时 worker 仍接收帧、更新缓存。不是每次 next 都重开 stream，也不是无限追加帧队列。JSONL/socket 仍为一请求一响应，不向原协议混入异步推送消息。

## 1. 建立订阅

通过现有 `session-call desktop --input <file|->` 发送；身份占位值必须替换为当前真实返回值。

```json
{
  "contractVersion": "act/linux-desktop-session-broker/v1",
  "brokerEpoch": "<本次 broker-ready 的 epoch>",
  "requestNonce": "<新的 32 位小写十六进制 nonce>",
  "operation": "observe-subscribe",
  "sessionId": "<已授权的 sessionId>",
  "confirmed": true,
  "strictIsolation": false,
  "input": {"durationMs": 60000}
}
```

返回 `data.subscriptionId`、`status=starting`、`delivery=latest-pull`。`starting` 只确认 worker 已创建，不确认已经收到首帧。后台启动错误从 next 返回，不隐藏为成功。

每会话最多一条订阅。`durationMs` 为 1000..300000，默认 60000；生命周期从 worker 的流初始化阶段开始计时，到期停止并清除像素缓存。延长观察需显式 unsubscribe 后以新 nonce 重新 subscribe，不自动重授权或无限续租。活动或已终止但尚未释放的订阅再次 start 返回 `SUBSCRIPTION_ALREADY_ACTIVE`。

## 2. 获取关键帧／补丁

```json
{
  "contractVersion": "act/linux-desktop-session-broker/v1",
  "brokerEpoch": "<epoch>",
  "requestNonce": "<新 nonce>",
  "operation": "observe-next",
  "sessionId": "<sessionId>",
  "confirmed": true,
  "strictIsolation": false,
  "input": {
    "subscriptionId": "<subscriptionId>",
    "afterSequence": 0,
    "waitMs": 1000,
    "path": "/absolute/path/update-1.png"
  }
}
```

- `afterSequence=0` 明确请求完整关键帧；后续填入**已成功应用到本地画面**的 sequence。
- `waitMs` 为 0..1000，默认 0。用条件变量等待变化，不固定睡满；无待交付变化时返回 `status=idle`，不写目标 PNG。
- broker 单线程执行请求，等待中的 next 会占用该执行位；同一订阅只保留一个在途 next，不并发排队大量长轮询。输入、unsubscribe、close 等后续请求须等当前请求返回。
- 路径沿用 PNG 输出保护。已有文件需明确 `overwrite=true`；即使最终 idle，也先验证输出位置。推荐唯一输出路径，避免重放响应指向被新帧覆盖的文件。

有更新时 `data` 包括：

```json
{
  "subscriptionId": "<subscriptionId>",
  "status": "update",
  "sequence": 12,
  "baseSequence": 8,
  "generation": 1,
  "kind": "patch",
  "coordinateSpace": "source-px",
  "width": 1920,
  "height": 1080,
  "region": {"x": 100, "y": 80, "width": 40, "height": 20},
  "path": "/absolute/path/update-2.png",
  "bytes": 180,
  "pixelDigest": "<补丁 RGBA 的 16 位 FNV-1a 摘要>",
  "captureMethod": "pipewire-damage",
  "framesReceived": 12,
  "pixelsRead": 2077000,
  "coalescedFrames": 3,
  "ageMs": 2,
  "atomicOutput": true,
  "replacedExisting": false,
  "effectConfirmed": false
}
```

上例数值仅示意。应用规则：

1. `kind=keyframe`：`baseSequence=0`，region 覆盖完整 width×height；替换本地画面与 generation。
2. `kind=patch`：要求本地 sequence 与 baseSequence 相等，generation 与完整图尺寸相符；将 PNG 按 region.x/y **直接覆盖**到原图，不做 alpha 混合。PNG 尺寸为 region.width×region.height，不是完整图尺寸。
3. 应用成功后将本地 sequence 改为返回 sequence。无法对齐、文件丢失或摘要不符时，用新 nonce 和 afterSequence=0 重新取关键帧，不勉强拼接。

序号可以跳跃，表示多个源帧已合并；显式重建也可重新返回同一 sequence 的关键帧。`coalescedFrames` 是此次返回与上次交付之间没有逐帧交付的源帧数，不等于未经处理而丢失的 damage 数。`ageMs` 是复制更新时距离最后一次消费源帧的时间，不是应用处理延迟或端到端时延。

## 增量、失效与资源边界

- 通过 SPA ParamMeta 请求 Header 和最多 256 个 VideoDamage 矩形。仅在源序号连续、格式尺寸未变、metadata 完整合法时按 damage 包围区域读取；多矩形合并为一个包围框，避免重叠区域造成无界重复处理。
- 缺少 Header/damage、初帧、源序号跳跃、DISCONT 或格式重协商时读取完整源缓冲。尺寸/格式/连续性失效递增 generation，交付完整关键帧；错误客户端基准也触发关键帧，而不是丢掉补丁后继续错误拼接。
- `captureMethod` 表示最新一次源帧读取方式：`pipewire-damage` 或 `full-frame-diff`。即便输出 keyframe，最新采集步骤也可能是 native damage；即便没有 native damage，输出也可以是小补丁。**采集增量与传输增量是两个不同事实。**
- worker 保留一个最新 RGBA 缓存和一个累计变化包围框，不缓存所有历史帧。源帧会逐次消费并合并，consumer 慢时不会无限积压；中间短暂提示可能被覆盖，此接口不是录像或无丢帧事件日志。
- 单订阅源图最多 8388608 像素（32 MiB 缓存，8 会话最多 256 MiB）；不包含在途源映射、补丁副本、编码输出，以及另行启用的普通 observe 差分基准。订阅当前不支持 maxDimension，超限明确终止为 `OPERATION_RESULT_TOO_LARGE`，可退回可缩放的普通 observe。
- `framesReceived` 和 `pixelsRead` 是本订阅累计消费帧数和实际归一化像素数。会话 inspect/sessions 的 framesCaptured/pixelsConsumed 也计入后台消费，停止订阅后保留计数；其中 pixelsConsumed 延续原单帧契约，按消费源图的完整面积计，不应拿它代替 pixelsRead 衡量增量收益。
- 生产路径只接收已有 Portal remote FD，不提供任意 node/FD/路径输入。独立 logind 监视器需与原 lease 同代际；约每 50ms 检查锁定、失活和监视器故障，检测到即停止采集并清除缓存，next 前后也复核 Portal/宿主有效性。Portal/PipeWire 断开、损坏或无法重建的缓冲会终止订阅；不盲目重连。
- 部分失败若发生在取得更新之后，错误附 `requiresKeyframe=true`；下次以新 nonce 和 afterSequence=0 恢复。相同 nonce 只重放原响应，不推进缓存交付游标或再次写文件；重放不保证文件仍存在。请求协议仍受原账本预算限制，不能无限轮询。

## 3. 停止与清理

```json
{
  "contractVersion": "act/linux-desktop-session-broker/v1",
  "brokerEpoch": "<epoch>",
  "requestNonce": "<新 nonce>",
  "operation": "observe-unsubscribe",
  "sessionId": "<sessionId>",
  "input": {"subscriptionId": "<subscriptionId>"}
}
```

返回 `status=closed`、`workerJoined=true`。停止不要求新增采集确认，普通账本满后有独立 8 次 unsubscribe 收尾配额，不挤占 close/shutdown 配额。关闭会话、broker 正常退出或 lease 被错误路径释放时同样停止并 join worker。计时器约 50ms 唤醒检查停止，但 join 仍须等待当前像素处理完成，不承诺严格 50ms 总停止期限。

输入定位仍用带 frameId 的普通 observe；订阅的 source-px 和 sequence 不能直接冒充旧接口的 frameId。视觉更新始终 `effectConfirmed=false`，不证明业务完成。

## 验证

```bash
cargo test --locked --lib desktop_frame_stream
cargo test --locked --lib subscription
cargo test --locked --lib native_pipewire_subscription_fixture -- --ignored --nocapture
```

最后一项需要 pipewire、pw-cli、pw-dump、timeout；启动私有目录中的隔离 PipeWire daemon 和自有 RGBA 测试源，验证原生协商、常驻消费、无消费者调用时仍更新及自动到期，不读取真实桌面。普通测试覆盖 damage 像素算法、补丁拼接、重放、配额和释放；隔离源不等于真实 compositor 已提供 VideoDamage，也不是 Portal GUI 或生产性能验收。
