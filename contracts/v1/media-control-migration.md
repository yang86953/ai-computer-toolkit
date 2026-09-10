# 阶段六媒体控制安全联锁契约

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`media-session.toggle-play-pause`、`play`、`pause`、`skip-next` 与
`skip-previous` 是阶段六计划提供的 Rust capability。当前没有已认证生产实现：历史
C++ worker 已退出生产授权，旧 Rust 主进程 adapter 已删除且不得恢复到组合根。

## 当前公开边界

- 每项控制仍在 target discovery 或 provider access 前要求逐操作确认；确认只授权一次
  尝试，不赋予未认证实现执行权。
- 公开目标只接受 canonical `s2:m:*`；旧 `media:*` 不再具有兼容执行语义。
- 未确认请求返回 `CONFIRMATION_REQUIRED`；确认后在 provider 选择点返回
  `BACKGROUND_OPERATION_UNAVAILABLE`，不得枚举或控制 GSMTC。
- 生产 launcher 无条件把媒体命令交给 Rust Policy；canonical opaque 目标不得重新触发
  已退役的 C++ runtime。
- `status`、`sessions` 与 `inspect` 同样返回 `BACKGROUND_OPERATION_UNAVAILABLE`；在
  阶段六只读 worker 完成前不得从主进程读取 SourceAppUserModelId 或媒体属性。
- method 与 descriptor 目录必须报告 `candidate-not-certified`，所有 C++/Rust 生产执行
  开关为 `false`。

## 阶段六目标边界

- Media Module 拥有 session 发现、opaque 身份、状态读取、控制、错误映射和结果投影。
- 只读与控制使用物理分离的固定 Rust sibling worker，并受 `KILL_ON_JOB_CLOSE` Job
  约束；WinRT 类型只存在于 worker 私有实现。
- worker 实时重新枚举 canonical `s2:m:*`，零/一/多命中分别为 stale、唯一和
  `AMBIGUOUS_TARGET`，不得任取首项。
- 控制 worker 在 dispatch 后的 timeout、取消、provider 异常或前景干扰必须返回
  `outcome=unknown`、`retrySafe=false`、`acceptedMayHaveOccurred=true`；调用方不得自动
  重试。
- 工具自有静音媒体 fixture 只用于自动化证据；用户专属视觉与交互验收仍由 Vikunja
  #1743 管理，AI 不得代验或关闭。
