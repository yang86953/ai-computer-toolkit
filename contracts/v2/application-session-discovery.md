# `application.session.discover@2` Linux read-only aggregate

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`application.session.discover@2` 是 Linux 专属、provider-neutral 的只读聚合；它不修改冻结的
Windows `application.session.discover@1`。Linux 默认 `discover app` 继续返回
`application.discover@2`，只有显式传入
`--capability application.session.discover@2` 才进入本契约。`sessions app` 在 Linux 继续以
`CAPABILITY_UNAVAILABLE`、`executionRealm=none`、`fallback=none` 失败。

## 固定来源与零关系

本契约只聚合三个固定事实来源：当前 `s2:h` host、`application.discover@2` 同源的 XDG
Desktop Entry `s2:a` 清单，以及 `process.discover@1` 同源的 procfs `s2:p` 清单。窗口 provider
不存在，因此 `windows=[]`、`total.windows=null`、`truncated.windows=null`、
`complete.windows=false`；空数组不表示主机没有窗口。

应用与进程是彼此独立的清单。应用始终发布 `runningProcessSessionIds=[]`、
`relationshipEvidence=none`、`launchCapability=unavailable`；进程始终发布
`relatedApplicationIds=[]`、`relationshipStatus=unassociated`、`windowSessionIds=[]`。禁止按名称、
Desktop File ID、Exec、PID、AT-SPI 或窗口事实推断关系。本契约不读取 AT-SPI、Portal、Wayland
compositor 或媒体总线，也不启动应用。

## 完整性、上限与警告

`--max-applications` 与 `--max-processes` 各自限制在 1..4096，并在确定排序后独立截断。
`count` 必须等于对应数组长度；可证明总量时 `total` 为非负整数，否则为 null；来源不完整且
未发生可证明截断时 `truncated` 为 null。`coverage` 与 `complete` 必须逐来源一致，available
不等于 complete。

`warnings` 只包含 schema 枚举的稳定 `code/message/count`，禁止路径、PID、Desktop File ID、
Exec、环境内容或原生错误。任何 `--max-items`、`--max-windows`、target、input、confirmation 或
foreground 参数都会被拒绝，不能成为 accepted-but-no-effect 参数。

## 路由与评估

capability assessment 只接受当前 canonical `s2:h`，成功决策为 `executable-background`、
`executionRealm=host-headless`、`readOnly=true`、`noFallback=true`。Windows 对 @2 明确 unavailable；
Windows @1 的生产 JSON、schema 与默认 CLI 行为不变。
