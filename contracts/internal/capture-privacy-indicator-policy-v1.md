# WGC 隐私指示器保留策略 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

契约名：`act/capture-privacy-indicator-policy/v1`。

## 所有权与边界

- `ComputerControlSystem` 只协调确认、精确目标、隔离 worker 和 Capture Module，不取得
  Windows Graphics Capture（WGC）平台类型。
- Capture Module 拥有“工具不得抑制系统捕获隐私指示器”的领域策略；Rust/C++ WGC
  Adapter 只按系统默认行为创建并启动 capture session。
- 该策略是静态不变量，不新增无状态 Component。公开接口只报告布尔证据，不公开
  WinRT、D3D11、HWND 或权限对象。

## 强制不变量

1. 工具不得把 capture session 的 border-required 属性设为 `false`。
2. 工具不得请求 borderless capture access，也不得在权限失败后尝试绕过系统边框。
3. Windows 是否以及以何种样式显示边框由当前系统版本和捕获上下文决定；工具只承诺
   不抑制系统行为，不能用自动化结果宣称肉眼已经观察到边框。
4. `window.screenshot@1`、`window.record@1`、`window.capture.frame.probe@1` 与兼容入口
   必须继续报告“系统隐私指示器可能出现”的公开证据。
5. 后台捕获失败不得降级为前台输入、窗口激活或权限绕过。

## 自动化与人工门禁

`tests/capture_privacy_indicator.rs` 扫描 Rust `src/` 与 C++ `cpp/src/` 生产源码，拒绝
`IsBorderRequired(false)`、`SetIsBorderRequired(false)` 和 borderless access 符号；
同时核对三个公开 schema 的 `const: true` 声明以及两条 WGC 主路径确实创建并启动
capture session。

自动化门禁只能证明仓库没有请求抑制边框。真实 Windows 捕获期间的可见指示器由
Vikunja #1650 验收，唯一执行人与关闭人为 `yang86`；任何 AI 不得代验、代签或关闭。

## 失败语义

- WGC 不可用或权限不足时返回既有结构化 capability/permission 错误。
- 不得为了消除系统提示而更换为未认证 provider 或前台抓取路径。
- 自动化门禁发现抑制符号或公开 schema 漂移时，构建失败并保持 capability 未认证。
