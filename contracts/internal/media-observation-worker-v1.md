# Media observation worker 协议 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

协议名：`act/media-observation-worker/v1`。这是 Windows GSMTC 的只读隔离
companion，不是媒体播放控制入口。

内部 `fixture-delay` operation 只接受 1..1000 ms，由生命周期测试证明 Job
timeout 会终止 companion；它不访问 GSMTC 或用户数据，也不属于公开 capability。

## Request

```json
{
  "contractVersion": "act/media-observation-worker/v1",
  "operation": "media-sessions-read",
  "maximumItems": 128,
  "sessionId": "s2:m:<opaque>"
}
```

`sessionId` 可省略；存在时 worker 必须在自己的实时 GSMTC 快照中重新解析精确
目标。不存在返回 `STALE_SESSION`；同一 provider identity 暴露多个会话时返回
`AMBIGUOUS_TARGET`，不得任取一个。

## 公开事实

- opaque `s2:m:*` session；
- `closed|opened|changing|stopped|playing|paused|unknown` 状态；
- title、artist、albumTitle；
- play、pause、toggle、next、previous 的可用性布尔值。

公共结果不返回 SourceAppUserModelId、PID、HWND、WinRT 对象或前景句柄。
provider identity 只在 worker 内用于生成 opaque ID；发现时遇到重复 identity 会
整组跳过并报告 `ambiguousSourcesSkipped`。

## 安全与生命周期

- 主进程用 `WorkerProcess` 把 companion 放入 `KILL_ON_JOB_CLOSE` Job；
- timeout/cancel 先终止 Job，再返回结构化错误；随后新请求必须可恢复；
- worker 只调用 `RequestAsync`、`GetSessions`、`GetPlaybackInfo`、
  `TryGetMediaPropertiesAsync` 与 controls 可用性 getter；
- 禁止 `TryPlayAsync`、`TryPauseAsync`、toggle、skip、前台 API 和输入；
- 调用前后复核前景窗口；变化返回 `HOST_INTERFERENCE_DETECTED`；
- `media.playback.control@1` 在等价验证前继续为
  `rust-compatibility-only`，只读迁移不得放松其逐操作确认。
