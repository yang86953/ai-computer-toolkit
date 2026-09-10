# WGC C++ 工具链证据

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本目录验证 Windows Graphics Capture 的 C++ 构建与运行边界，不属于已发布的
`window.screenshot@1` 实现。

## 证据

- LLVM-MinGW 自带的 WIDL C++ 头在本机因 `boolean`/`BYTE` UUID 模板重复而不能
  严格编译；生产 preflight 因此只使用私有最小 ABI vtable，不让 WinRT 类型越过
  Windows backend。
- Windows SDK 10.0.26100.0 的 C++/WinRT 投影可由当前 clang++ 编译；SDK 生成头在
  Windows 大小写不敏感文件系统上产生 `nonportable-include-path` 告警，探针只对
  这一个 SDK 告警定向关闭，其他警告仍按 `-Werror`。
- `support_probe.cpp` 验证 `GraphicsCaptureSession::IsSupported()`。
- 经过验证的 frame 实现已从 spike 提升为生产
  `cpp/src/platform/windows/wgc_capture.cpp` 与
  `cpp/src/worker/capture_worker_main.cpp`；本目录不保留重复实现。
- 测试通过通用 `WorkerProcess` 将 capture worker 放入 Windows Job；1 ms deadline
  会终止 Job，随后正常请求仍可取得内存帧。
- 内存帧探针不读取/编码/持久化像素，不创建输出文件，不激活窗口或发送输入；
  测试前后前台必须相同。

该 C++ 内存探针门禁已在阶段 5E 退役；本目录只保留历史选型证据，不再提供构建或运行入口。

WGC 可能显示系统捕获隐私指示器；探针不会规避或隐藏该提示。

## 尚未发布

- GPU surface readback、BGRA/RGBA 转换和像素边界；
- PNG 编码、路径/覆盖确认和原子写入；
- Rust/C++ 帧级等价门禁。

真实目标的“帧元数据、无 surface 读取”窄纵切已经通过
`window.capture.frame.probe@1` 发布，必须逐操作确认。它不等于截图结果能力。
