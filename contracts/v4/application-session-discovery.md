# `application.session.discover@4` Linux launch-aware UIX aggregate

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

版本四保持 `application.session.discover@3` 的 XDG、procfs、UIX 窗口来源和窗口到进程的
认证关系，只把 `application.discover@3` 的启动状态纳入同一次 XDG 快照。只有指向当前
toolkit 自身映像、固定私有参数且非 D-Bus activation 的 Desktop Entry 夹具返回
`available-confirmed`；普通应用继续返回 `unavailable`。

该版本不建立 Desktop Entry 应用与进程或窗口之间的关系：应用的
`runningProcessSessionIds` 仍为空，进程的 `relatedApplicationIds` 仍为空。UIX 只覆盖显式
opt-in 应用窗口，不冒充全局窗口目录。版本二和版本三 schema、默认 CLI 路由及公开结果保持
不变，调用方必须显式选择 `application.session.discover@4`。

聚合是只读的，不启动应用、不请求前景、Portal 或桌面输入，也不公开 Desktop File ID、Exec、
路径、PID、UIX endpoint、token 或原生句柄。启动前仍必须用 `application.open@2` 对精确
`s2:a` 重新解析，并经过逐操作确认与预先前景影响同意。
