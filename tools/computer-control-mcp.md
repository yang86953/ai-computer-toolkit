# 电脑控制 MCP（Linux / Windows）

`ai-computer-toolkit` 无参数启动标准 MCP stdio JSON-RPC 服务。它与 CLI 共用 `session-host desktop`、桌面会话模块、输入契约和取消机制。每个 MCP 客户端有自己的 broker 和会话。包内只提供通用 MCP，不包含特定客户端扩展。

## 接入

先构建 `cargo build --locked --bin ai-computer-toolkit`，然后把绝对二进制路径交给 MCP 客户端。Windows 使用 `.exe`；没有参数。

```json
{
  "mcpServers": {
    "computer-control": {
      "command": "/absolute/path/to/ai-computer-toolkit",
      "args": []
    }
  }
}
```

`--list-tools` 返回同一份工具清单，初始化和枚举不连接桌面。MCP 直接运行 Rust 二进制，无需启动包装脚本。

支持的协议版本：2024-11-05、2025-03-26、2025-06-18。支持 `initialize`、`notifications/initialized`、`ping`、`tools/list`、`tools/call`、`notifications/cancelled`。

## 平台与实现归属

| 能力 | Linux | Windows |
|---|---|---|
| 会话、授权、最新截图身份、取消、关闭 | 同一 `DesktopSessionModule` 和 JSONL broker | 同左 |
| 显示器/光标 | Portal/EIS 的授权映射 | UIX 平台层的显示器查询和已有光标实现 |
| 捕获 | 现有 PipeWire 捕获 | 现有 WGC/D3D11 捕获核心，扩展到显示器 |
| 键鼠 | 现有 EIS 适配器 | 现有键盘/指针执行器，共用请求内释放和 deadline |
| PNG 编码与缩放、原子输出 | 公共帧组件 | 同左 |

Windows 使用当前活动交互桌面；不绕过锁屏、安全桌面或权限边界。Linux 需要 Portal RemoteDesktop/ScreenCast、EIS 和 PipeWire，并遵循授权对话框。Windows 会话返回 `backend=windows-wgc`，不伪造 EIS/PipeWire 事实。协议标识 `act/linux-desktop-session-broker/v1` 为兼容既有调用方保留，名称不再限定后端。帧订阅暂限 Linux，公共 broker 明确返回 `frameSubscriptions` 能力值。

## 操作

| 工具 | 用途 |
|---|---|
| `computer_connect` | 显式授权后连接，返回 sessionId |
| `computer_status` | 本客户端会话状态，不自动连接 |
| `computer_observe` | 返回 PNG、frameId、尺寸和截图坐标信息 |
| `computer_interact` | 截图坐标移动/点击、按键、ASCII 短批次；同一 broker 请求执行并截图 |
| `computer_keys` | 完整单键或组合键，动作后截图 |
| `computer_pointer` | 相对移动、点击、滚轮和完整拖拽；之后截图 |
| `computer_disconnect` | close → 验证空 sessions → shutdown |

调用顺序：`connect → observe → 看图确认目标 → interact/keys/pointer → 核验返回图 → disconnect`。`confirmed`、`foregroundConsent`、`strictIsolation` 只表达已有用户授权，不能用于自行授予权限。此路线要求显式 `strictIsolation=false`。

- 所有输入引用本客户端最新 `frameId`。输入尝试后旧帧失效，包括失败。
- `interact` 的 x/y 为返回图中的 `observation-px`。`pointer` 使用 `relative-logical-px`；Windows 按每段移动起点所在显示器的 UIX scale 换算。虚拟桌面可含负物理坐标，显示器之间的空隙不接受点击。
- 不提供跨请求按键或按钮持有。`text` 为 ASCII 键盘合成，仍受输入法影响。
- 截图身份不锁定应用焦点。输入已接受不代表应用效果已完成，需要核验返回图。
- 单客户端只执行一个工具调用，并发请求立即返回 BUSY。临时截图在私有目录内生成，读取为 MCP 图像后删除，退出清理目录。

## 取消与失败

取消绑定当前 MCP requestId，并关联到 broker 原请求 nonce；公共输入检查点负责停止及释放。取消确认与原操作结果都会被读取，避免污染下一次调用。已发生的动作不回滚。打开会话和单帧捕获按已有有界调用收尾；若连接期间取消，随后关闭新会话。

超时、断线或不相关的回包使传输失效，返回 `OUTCOME_UNKNOWN`，不重放、不隐式重连。输入完成后截图失败返回部分失败，不能重放原输入。EOF 或信号请求取消后由 owner 清理；强制终止的结果不能当作正常关闭。

## 验证

```bash
cargo test --locked --lib
cargo test --locked --test mcp_stdio
cargo check --locked --target x86_64-pc-windows-msvc --lib --bin ai-computer-toolkit
```

协议与交叉编译检查不等于 Windows 实机验收。WGC、多屏缩放、鼠标键盘和关闭流程还需在真实交互 Windows 桌面验证。
