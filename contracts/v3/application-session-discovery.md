# `application.session.discover@3` Linux UIX-aware read-only aggregate

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`application.session.discover@3` 在不修改冻结版本二的前提下，把 XDG Desktop Entry、procfs
与当前用户显式启用的 UIX Agent 窗口合并为一次只读快照。Linux 默认 `discover app` 仍返回
`application.discover@2`；调用方必须显式提供
`--capability application.session.discover@3`，并可分别设置
`--max-applications`、`--max-processes` 与 `--max-windows`。

## 固定来源与覆盖边界

- 应用来源固定为 XDG Desktop Entry；它仍是安装清单，不推断运行进程，也不提供启动能力。
- 进程来源固定为 procfs；公开结果只包含 opaque 进程代际、名称和中立状态。
- 窗口来源固定为同一用户主动发布的 `uix.agent.v1` endpoint。没有 endpoint 时，UIX
  窗口清单是可用且完整的空清单；这只表示当前没有 opt-in UIX 窗口，不表示桌面没有窗口。
- 不连接 Wayland compositor、AT-SPI、Portal 或 X11，不枚举任意第三方窗口，也不执行前台输入。

`coverage.windows.coverage=opt-in-uix-agent-applications` 是不可省略的范围声明。
`complete.windows=true` 只证明所有安全发现且通过认证的 UIX endpoint 已在有界期限和上限内
完成；它从不升级成全局桌面覆盖声明。

## 精确窗口到进程关系

UIX descriptor 的 PID 只在 Adapter 内用于认证 peer，并通过 `/proc/<pid>` 重新解析为当前
`s2:p` 进程代际。公共窗口仅发布 `ownerProcessSessionId`；PID、socket、token、UIX 原生
window ID 和 generation 均不得离开 Adapter。

只有重新解析成功的同一进程代际才能发布
`relationshipEvidence=authenticated-peer-process-lifetime`。聚合同时把窗口 opaque ID 写入已返回
owner 进程的 `windowSessionIds`，并据当前 UIX 可见有标题窗口更新 `hasVisibleWindow` 和
`windowVisibility`。进程不在本页或已经退出时，窗口仍可携带精确 opaque owner；解析失败则
`ownerProcessSessionId=null`、`relationshipEvidence=none`，不得按标题、进程名或 Desktop File
猜测关系。应用与进程关系继续是 none/unassociated。

## 完整性与失败

三个维度分别发布 `count`、`total`、`truncated` 和 `complete`。单个 UIX endpoint 的认证失败、
超时、协议错误或有界截断进入结构化 `warnings` 并使窗口覆盖不完整；聚合不得静默改走
AT-SPI、compositor 或前台输入。UIX 发现目录本身不可访问时，窗口 coverage 显式
`available=false`，而不是把空数组解释成成功枚举。

完整 JSON 形状由 `application-session-discovery.schema.json` 冻结。生产 assessment 只接受当前
`s2:h`，执行域固定为 `same-session-no-focus`，不需要确认或前景同意，并声明
`globalWindowDirectoryClaimed=false`、`noFallback=true`。
