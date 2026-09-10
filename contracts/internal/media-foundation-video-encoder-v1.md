# Media Foundation 视频编码 Component v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 所有权

`MediaFoundationEncoder` 是 Recording Module 私有的窄 Component。它只拥有一个
H.264/MP4 编码会话的原生适配、帧布局转换和配对生命周期，不拥有公开 capability、
精确窗口解析、WGC 捕获、确认、覆盖许可、多产物事务、worker 协议或分析产物。

`ComputerControlSystem` 与公开 JSON 边界不得出现 COM、Media Foundation、HRESULT、
GUID、Sink Writer、Source Reader 或任何其他 Windows 原生类型。

## 输入契约

- 输出必须是 Module 已预留、只供当前会话写入的私有 `.mp4` staging 路径；
- `width` 和 `height` 必须是非零偶数；
- `framesPerSecond` 必须大于零；
- `quality` 是 1–100 的 provider-neutral 质量等级，不是 x264 CRF，也不得静默按 CRF
  解释；
- 每帧必须是冻结尺寸、顶向下行序、每像素四字节的 RGBA；
- 调用方不能提供 codec、profile、filter、argv、shell、原生属性、encoder path 或外部
  executable。

Component 把 RGBA 转换为 Media Foundation `RGB32` 所需的底向上 BGRX；alpha 不参与
编码。平均码率由像素率和 `quality` 确定性派生，并裁剪到 250 kbps–20 Mbps。该派生是
内部策略，不成为公开比特级稳定格式。

## 生命周期

1. `is_h264_available` 只启动配对 COM/Media Foundation 生命周期并枚举同步或异步软件
   H.264 输出 MFT，不创建文件；它不要求编码器直接接收 RGB32，因为 Sink Writer 可以
   组合系统颜色转换器。worker 固定禁用硬件转换器，避免驱动异步生命周期越过进程收尾。
2. `start` 重验配置与可达性，启动独立运行时，协商 H.264 输出和 RGB32 输入并调用
   `BeginWriting`。
3. `write_rgba_frame` 验证固定帧长，写入单调百纳秒时间戳样本。失败不会改变公开目标，
   staging 的回滚仍由 Module 拥有。
4. `finish` 消耗会话并调用 `Finalize`；writer 必须先于 Media Foundation 和 COM 释放。
5. 进程取消或异常退出时，worker Job 仍是外层所有者；Component 不创建子进程，也不
   产生独立后台任务。

## 失败语义

Component 只返回稳定、无原生细节的配置、运行时、编码器、启动、帧、写入和收尾错误。
阶段 5D2 由 Recording Module 把这些错误映射到版本化公开错误码；不得把 HRESULT 或原生
对象泄漏到 Reply。

## 独立验证

Rust 动态测试必须使用合成 RGBA 帧生成真实非空 MP4，验证 `ftyp`，再通过 Media
Foundation Source Reader 读取原生 H.264 媒体类型和至少一个样本。另一个测试必须覆盖
非法奇数尺寸、错误帧长、失败后的有效帧写入、正常收尾和临时文件清理。

本阶段不把 Component 接入 WGC、recording worker 或 launcher，也不改变公开录制输入；
这些集成与 FFmpeg 清除属于阶段 5D2。
