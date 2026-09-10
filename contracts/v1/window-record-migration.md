# Exact window recording Rust contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`window.record@1`、`desktop.record` 与 `app.record` 的正式 launcher 路由固定进入 Rust。
录制不再读取请求内容选择 C++，也不存在 C++ runtime 缺失时的静默降级。
正式目标只接受当前目录中的 canonical `s2:w:*`；native、`s1:*` 与其他 target kind
全部失败闭合。

公开 input 固定为 `window-record-input.schema.json`。默认 30 秒、2 fps、最大宽度
960px、provider-neutral quality 75、最多 8 个关键帧；硬上限为 5 分钟、10 fps、
1920px、quality 100、20 个关键帧，因此单次最多 3000 个编码帧。
`changeThreshold` 保持 JSON number；旧 `crf` 字段必须按未知字段失败，不得静默换算。

正式 Rust Module 的顺序必须是：

1. confirmation-first；
2. 严格解析 config、最终 MP4/analysis 路径与独立 overwrite 许可；
3. 唯一重新发现精确 `s2:w:*` 并完成 WGC 零帧 preflight；
4. 私下探测 Windows Media Foundation H.264 编码能力，调用者不能提供 runtime；
5. 父 Module 独占同卷 MP4 staging 与 analysis staging；
6. 固定 sibling `ai-computer-toolkit-recording-worker.exe` 使用
   `act/recording-worker/v2`，在 kill-on-close Job 中再次校验 confirmation、target、
   config 与 staging；worker 的 preflight、WGC 和编码生命周期全部归属同一个 MTA 线程，
   编码器启动在该线程内再次验证 Media Foundation availability；
7. worker 内保持一个 WGC session，并调用项目自有 Rust `MediaFoundationEncoder`；
   自由线程 WGC 生命周期必须按 frame pool、session、item/D3D、WinRT apartment 的顺序
   关闭并释放，显式 `Close` 与 Rust 字段析构顺序必须一致；编码器不创建子进程、shell、
   argv 或独立后台任务，timeout/cancel 只需回收 worker Job；
8. 不捕获音频和光标，不激活、恢复、显示窗口或发送输入；WGC 系统隐私指示器可以出现；
9. 尺寸变化以 `CAPTURE_SIZE_CHANGED` 停止，不拉伸伪造；
10. worker 验证 MP4 `ftyp`、真实长度、PNG 签名、manifest、关键帧数量和前景不变；
11. 父 Module 严格解析无路径、无 PID、无 native 字段的单行 JSON envelope；
12. 分析文件先以可回滚方式安装，MP4 最后原子提交；MP4 提交失败必须恢复旧分析文件。

已有 analysis directory 在 `overwrite=false` 时拒绝。`overwrite=true` 只允许替换工具拥有的
`frame-*ms.png`、`storyboard.png` 和 `manifest.json`，调用方未拥有的文件保持原位。
候选、备份与 Job 在成功、错误、timeout 和 cancellation 后都必须清理；回滚失败必须返回
`VIDEO_ARTIFACT_RESULT_UNKNOWN`，不得声称最终状态已知。

调用者不能传 encoder path、codec、filter、argv、shell、WGC native target 或任意分析脚本。
公共接口与 worker envelope 都不能公开 Win32、WGC、D3D、进程或临时路径类型。

本契约的自动化完成条件包括 Rust 单元/集成测试、launcher 固定路由、失败残留检查，以及
在具备系统内建 H.264 MFT 的环境中对自有动态窗口完成真实 H.264/MP4 录制和 Source
Reader 回读。自动化结果不替代用户的真实窗口视觉与交互验收。

截至 2026-08-10，Rust worker v2、Job、固定路由、多产物事务和项目自有 Media Foundation
编码链路已经落地。仓库自有窗口 fixture 已生成真实 H.264/MP4、manifest、关键帧与
storyboard；挂起 sibling fixture 验证 timeout 回收和 staging 零残留。2026-08-11 的
Vikunja #1959 将自由线程 frame pool 的关闭与字段析构提前到 capture session，修复两次
同偏移 `GraphicsCapture.dll_unloaded` 访问冲突；修复后首轮全目标、20 次精确录制与
5 轮完整录制测试组均未新增 WER 崩溃。剩余门禁是用户对真实窗口候选的视觉与交互验收，
不能由自动化关闭。

Vikunja #1975 用 recording worker 私有 Video Recording Adapter 的封闭
`VideoRecordingErrorCode` 统一捕获读回/目标、覆盖许可、分析/分析写入、编码器收尾/
启动/可达性/帧写入、视频缺失与原子提交十一项错误定义。二十九个构造点不再分别维护
公开字符串；上述单 MTA WGC、同线程编码、Component 错误翻译、analysis 所有权、原子
提交及用户 #1726 视觉验收边界保持不变。

Vikunja #1976 进一步用共享 Window Capture Adapter 私有封闭的
`WindowCaptureErrorCode` 统一 WGC/D3D11/WinRT 捕获链十六项错误定义；
`window_capture.rs` 的三十六个直接 `AppControlError` 构造表达式归零，二十五个平台
错误 mapper 调用只能接收封闭类型。上述 frame pool、session、item/D3D 与 apartment
关闭/析构顺序、尺寸变化语义、同线程编码以及用户 #1726 视觉验收边界保持不变。

Vikunja #1981 用 `RecordingWorkerErrorCode` 私有封闭类型统一
`act/recording-worker/v2` 协议入口自身六项错误定义；十个直接 `AppControlError` 构造点
归零，固定序列化 fallback 也从该类型取得 `WORKER_PROTOCOL_ERROR`。RecordingConfig、
Window/Capture/Video Recording Adapter 错误仍原样传播；confirmation-first、canonical
目标、单 MTA WGC、同线程 Media Foundation、Job、staging、多产物事务、前景不变和用户
#1726 视觉验收边界保持不变。

Vikunja #1988 用 `RecordingConfigErrorCode` 私有封闭类型统一 RecordingConfig 自身的
`INVALID_ARGUMENT`、`OVERWRITE_CONFIRMATION_REQUIRED` 与 `OPERATION_FAILED` 三项错误
定义；十七个直接 `AppControlError` 构造点归零，参数/路径验证和 Output Guard 错误翻译
由同一类型驱动。RecordingConfig 仍是普通共享领域配置；Window Record Module、Recording
Worker、Video Recording Adapter 与 Policy 继续原样传播，字段白名单、覆盖许可、staging、
Job、WGC、Media Foundation、多产物事务和用户 #1726 视觉验收边界保持不变。
