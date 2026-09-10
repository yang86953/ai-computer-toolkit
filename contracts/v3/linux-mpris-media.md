# Linux MPRIS App facade v3

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 范围与兼容

本版本将第三方播放器的公开 MPRIS 接口接到统一 App facade 和 UIX CLI 任务宿主。
`@2` 仍属于 Media candidate surface，其私有 fixture 协议和 `execution:none` 不变；
`@3` 属于 App surface，控制成功额外要求重新观察到目标播放状态，不能把两者互换。
Windows v1、Linux 旧 `media-session` direct surface 和 UIX 应用控制路线均不改变。
不发布任意 D-Bus 地址、服务名称、对象路径、方法名、原生 PID 或脚本执行字段。

- `media.session.discover@3`：App `discover`，空 target；input 接受 `maximumItems`（1..128，默认 128）和 `timeoutMs`（1..30000，默认 5000）。
- `media.playback.state.read@3`：App `read`，精确 `s2:m`；input 只接受 `timeoutMs`。
- `media.playback.control@3`：App `apply`，精确 `s2:m`；input 接受 `operation=play|pause|stop` 和 `timeoutMs`。
- 状态与控制重新解析时固定最多 128 项；目录截断、换代和歧义不被解释成成功。
- 所有 input 都是封闭对象，未知字段、错误类型和越界预算在任何 provider I/O 前拒绝。
- 旧 CLI 的写入仍要求 `--confirm`；任务宿主只从可信启动 grant 推导内部确认，不增加逐操作人工提示。

## 路由与生命周期

生产只解析当前有效 UID 的规范 `/run/user/<uid>/bus`；不从调用者环境或公开请求选择总线。
沿用现有 GUID、owner generation 绑定、禁用自动激活、读/写分离的固定主映像 self-worker，
且只调用 MPRIS Player 白名单属性及 Play/Pause/Stop；不调用 Raise、OpenUri、Quit、Seek、Metadata、
截图、OCR、宿主输入、剪贴板或窗口激活，也没有前台 fallback。

Module 拥有一次操作的单一总 deadline。控制 method 最多提交一次；回复后重新解析同一 opaque
目标并有界读取状态，直到观察到 playing / paused / stopped 中对应的条件。
跨读取不能重置总 deadline。worker 的 accepted、final、取消、异常与不确定结果语义继续保持。

- 拒绝在 dispatch 前发生时，不伪报执行成功。
- method 回复不等于业务效果；返回控制成功必须含匹配的只读状态观察。
- 观察只证明请求后曾满足状态条件，不证明因果、音频物理效果或持久提交。
- method 已接受后，观察失败、目标换代或期限耗尽不得自动重发写入；保留已有派发事实与失败原因。
- 任务执行仍由现有 sequence worker 包住，采用剩余任务预算、取消与结果上限，不创建第二套执行器。

## 后台资格

资格分为固定路线保证和当前目标/能力/状态评估。目录只声明路线可用，不能将任意 `s2:m`
自动认定为可执行。执行前仍重新校验当前 owner 与 Can*。MPRIS 路线不需要窗口句柄、焦点、
图像或窗口可见状态，因此是 window-independent，不是宣称已经观察到某个最小化窗口。
不请求激活不等于约束第三方应用自身的通知或媒体业务效果。

任务 `discover` 新增可选 `scope=media`，默认仍为 `applications`，两者均受启动 grant 的
`allowDiscovery` 限制。媒体发现使用已授权的只读 App discovery 和同一有界 sequence 执行器。
媒体评估不读取 Metadata 或用户媒体名称；只读取同一已授权目标的播放状态与控制可用性。

## 验证状态

当前源码已有默认生产 App v3 路由，仍须针对实际目标 assessment，不把候选 v2 升级为通用认证。公开 CLI、协议/故障回归与真实独立 Haruna 的本轮证据由 TASK-061（历史验证资料未随仓库迁入） 持有；最终制品的长时连续性由 TASK-071（历史验证资料未随仓库迁入） 单独验收。

这些记录仅覆盖指定版本和负载：合成服务不替代真实第三方验证，私有 Xvfb 实例不冒充真实最小化窗口、Portal/EIS、物理音频或人工基线。应用首次加载时已公开 playing 也不等于其就绪或本次操作已生效；状态确认失败仍禁止重派。使用 UIX App 开发的应用不属于本项目控制对象，原有相关实现只保留历史兼容，不新增控制适配。
