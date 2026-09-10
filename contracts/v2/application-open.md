# `application.open@2` Linux authenticated fixture launch

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该版本不复用 Windows `application.open@1` 的 Shell identity 语义。首批只接受
`application.discover@3` use-time 重解析后仍唯一命中、且指向 toolkit 自身映像的 Desktop Entry 夹具；
普通用户应用、`application.discover@2` 目标和 D-Bus activation 当前全部保持 unavailable。

公开请求只接受 canonical `s2:a`、空 input object、逐操作确认和预先前景影响同意。不接受或
公开 Desktop File ID、路径、Exec、TryExec、argv、环境、工作目录、URI、field code、PID、fd
或 shell verb，也不回退 `gio`、`gtk-launch`、`xdg-open`、PATH、Portal、X11/XWayland。

Adapter 只认证绝对 Exec 路径仍与当前 toolkit 映像 device/inode 相同，并要求唯一固定参数
`__application-launch-fixture-v1`。dispatch 不执行 Desktop Entry 路径，而是通过
`/proc/self/exe`、固定 argv、空环境、固定 `/` 工作目录和 null stdio 启动同一映像；因此
Desktop Entry 文件或路径在检查后被替换也不能改变被执行代码。当前严格 Exec 子集拒绝 field
code、反斜杠、shell 元字符、未知参数和 `DBusActivatable=true`。

成功只证明固定子进程已启动并以成功状态退出，不声明窗口获得焦点，也不读取跨客户端前景。
spawn 成功后发生的异常禁止自动重试；deadline 会回收本工具创建的夹具子进程并返回
`OUTCOME_UNKNOWN`。本版本不是通用用户应用启动认证，扩展范围必须先补齐标准 D-Bus activation
或 executable FD 绑定、完整 Exec tokenizer、权限/所有权策略与真实应用逐项验收。
