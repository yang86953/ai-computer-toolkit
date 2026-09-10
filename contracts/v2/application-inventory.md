# `application.discover@2` Linux application inventory

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`application.discover@2` 是 provider-neutral 的 Linux 应用/进程独立清单；它不修改冻结的
Windows `application.discover@1`，也不承诺 launch、窗口关系、前景或坐标控制。

## XDG 来源与安全边界

- 按绝对 `XDG_DATA_HOME`、有序绝对 `XDG_DATA_DIRS` 的 `applications/` 扫描；相对配置失败闭合。
- Desktop File ID 在过滤前 claim。高优先级 `Hidden=true` 遮蔽低优先级；同根 flat/nested
  同 ID 冲突时两者均不发布且清单不完整。
- NoDisplay 应用仍是 installed application；OnlyShowIn/NotShowIn 按当前 desktop 顺序决定，
  同一 desktop 同时出现在两表为非法。
- 要求未本地化 Name；遵守 locale 回退、规范 list escape、Type/Exec/DBusActivatable 与 TryExec
  过滤。Exec/TryExec 永不执行也不进入公开结果。
- walker 从可信 root fd 相对打开，禁止目录/文件 symlink、magic link 与非普通文件，文件打开
  nonblocking；打开前后及读取后均核对 regular、大小上限和文件代际。任何竞态只产生无路径
  warning 和 `complete=false`，不发布部分文件内容。

## 公开完整性与隐私

`applicationsReturned` 等于 `applications` 长度；`applicationsTruncated` 表示 max-items 截断。
只有扫描完整时 `totalEligibleApplications` 才是可证明总量，否则为 null。`warnings` 只能使用
schema 的稳定枚举，禁止携带路径、Desktop File ID、Name 或原始错误。所有 XDG 根均缺失时
coverage 为 unavailable 且 applications 不完整；可读根中的安全记录仍可在其他根不可读时返回。

每个 application 的 `runningProcessSessionIds=[]`、`relationshipEvidence=none`、
`launchCapability=unavailable`；windows 固定为空。公开目标不含 Desktop File ID、原生路径、
Exec、TryExec、内容摘要或任何 app↔process↔window 猜测。

## 验收

合成 fixture 必须覆盖根内/根外 symlink、FIFO/socket、lstat→open 替换、并发普通文件↔
symlink/FIFO、同 ID 冲突、Hidden/NoDisplay、Only/Not 顺序与重叠、locale、TryExec 和 2→1 截断。
真实主机验收允许零应用，只验证 v2 schema、统计一致性和隐私；不得打印真实清单。Portal、
Wayland compositor 与用户桌面不参与该 capability。
