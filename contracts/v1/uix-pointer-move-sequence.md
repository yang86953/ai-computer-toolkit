# `ui.input.pointer.move.sequence@1`

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

该 capability 通过统一 `app.apply` surface，在已认证 UIX 窗口的同一连接、固定 generation 与连续
revision 链中顺序提交多个应用内部 pointer move。它用于级联菜单、下拉、图表等需要跨多个客户区
logical 点连续更新悬停目标的流程，不把单次 `ui.input.pointer@2` 或含点击的混合序列重复包装。

## 输入与真实增量

输入由 `uix-pointer-move-sequence-input.schema.json` 冻结，只接受 `coordinateSpace`、`moves`、
`intervalMs`、`timeoutMs`。坐标空间固定 `client-logical-px`；`moves` 包含 2..64 个 finite 且
0..65535 的点；相邻间隔 0..500 ms、计划时长最多 5000 ms，总 deadline 必须保留 100 ms。

UIX v0.0.2 的 `pointer_move` 会同步更新应用内部 enter/leave/hover target，即使组件不继续消费 move
也不把已更新的悬停事实判为失败。该能力只按调用方给出的点顺序派发，不插值、不平滑、不生成额外
采样，也不包含 click、pointer down/up、拖拽或滚轮。

## 执行与失败边界

用户确认必须先于 input、target 与 provider。Adapter 在首个 dispatch 前预检 `perform` 和
`pointer_move`，随后只在同一认证连接、固定 generation 与响应 revision 链上顺序提交。任一
dispatch 后出现连接、修订、deadline 或最终响应不可信，返回 `OUTCOME_UNKNOWN`，公开已接受移动数
并禁止自动重试。

成功结果由 `uix-pointer-move-sequence-result.schema.json` 冻结，只证明 Agent 接受各移动并按其协议
更新应用内部悬停事实，不证明最终组件状态或 UI；`effectConfirmed=false`、`finalStateReached=false`。
能力明确无插值、点击、按钮所有权、拖拽、滚轮、桌面输入或桌面指针，不请求前景，不泄露原生/传输
身份，不使用 Portal、X11/XWayland、compositor 私有协议或 fallback。用户真实交互验收暂缓。

```text
ai-computer-toolkit run app apply --capability ui.input.pointer.move.sequence@1 --target sessionId=<s2:w:opaque> --input <file|-> --confirm
```
