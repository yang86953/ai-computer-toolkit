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
| `computer_connect` | 显式授权后连接，返回 sessionId；可选 `authorizationMode=session` 与 `rememberAuthorization` |
| `computer_status` | 本客户端会话状态，不自动连接 |
| `computer_observe` | 返回 PNG、frameId、尺寸和截图坐标信息 |
| `computer_interact` | 截图坐标移动/点击、按键、ASCII 短批次；同一 broker 请求执行并截图 |
| `computer_keys` | 完整单键或组合键，动作后截图 |
| `computer_pointer` | 相对移动、点击、滚轮和完整拖拽；之后截图 |
| `computer_run` | 一次调用内由服务端循环「补帧 → 送一批 → 读回新帧」，连续跑多批；回读关键帧有界 |
| `computer_disconnect` | close → 验证空 sessions → shutdown |
| `computer_authorization` | 查看或撤销本工具记住的桌面授权（status/forget） |

调用顺序：`connect → observe → 看图确认目标 → interact/keys/pointer → 核验返回图 → disconnect`。`confirmed`、`foregroundConsent`、`strictIsolation` 只表达已有用户授权，不能用于自行授予权限。此路线要求显式 `strictIsolation=false`。

### 一次授权持续复用

`computer_connect` 始终要求完整显式确认三元组，这是唯一确认点。两个显式开启的复用模式：

- `authorizationMode=session`：connect 处的一次确认覆盖整条会话。之后 `observe`/`interact`/`keys`/`pointer`/`run` 可省略确认字段并继承已授予作用域；显式传入 `confirmed=false` 或 `strictIsolation=true` 仍被拒绝，继承不能覆盖明确拒绝。缺省（`operation`）保持逐操作显式确认。
- `rememberAuthorization=true`（仅 Linux Portal）：按系统原生 `persist_mode=2` 记住授权并保存单次 restore token（当前用户私有状态目录、原子替换、跨进程互斥），下次连接自动尝试恢复同一用户授权并轮换新 token。token 不出现在任何结果、日志或文件名中，响应只含 `restoreTokenRetained` 等脱敏事实。Portal 无法恢复时按官方语义回退正常选择弹窗，工具包不自动点击、失败不重复弹窗。Windows 无此机制，请求时在任何派发前返回 `DESKTOP_AUTHORIZATION_PERSISTENCE_UNSUPPORTED`。
- `computer_authorization`：`action=status` 查看是否已保存可恢复授权（脱敏）；`action=forget` 清除本地保存的凭据并停止本客户端全部 live 会话。本地忘记不撤销系统 Portal 侧授权记录（`revokesSystemPortalRecords=false`），后者需在桌面环境权限管理中单独处理。

详细协议语义见 [`contracts/v1/linux-desktop-session-broker-v1.md`](../contracts/v1/linux-desktop-session-broker-v1.md)。

### 长流程：`computer_run`

单批调用（`interact`）每次都要调用方先观察一次并回传 `frameId`，长流程因此被迫在「模型往返」和「输入批次」之间交替；`computer_run` 把这段循环搬进工具包进程内：

- 调用方给 `batches`（1..64 批，每批 1..648 步 + 自己的 `timeoutMs`），**不需要先 observe，也不需要给 frameId**。
- 服务端在每批前补一帧，因此每批仍绑定送出当时的最新帧，绝不复用旧帧；契约不变，只是不再把换帧的成本推给调用方。
- 出错默认停止后续批次并回读该批已发生的效果。预检拒绝（`acceptedMayHaveOccurred=false`）没有投递事件，属于可跳过的批次；`stopOnError=false` 时跳过并继续，但「输入可能已送出」永远停止，不自动重放。
- `captureEveryBatches` 采样回读关键帧，`maxFrames`（≤8）封顶；`totalTimeoutMs` 默认 45s、上限 600s，实际可用值取决于 MCP 客户端自己的工具超时（例如某客户端缺省 60s）。
- 内部补帧沿用调用方的 `maxDimension`：`observation-px` 由捕获图尺寸决定，内部帧与回读帧分辨率不一致会让调用方坐标落到另一套坐标系。
- 单客户端同一时刻只跑一个工具调用：一次 `computer_run` 期间其他工具调用返回 BUSY。

- 所有输入引用本客户端最新 `frameId`。输入尝试后旧帧失效，包括失败。
- `interact` 的 x/y 为返回图中的 `observation-px`。`pointer` 使用 `relative-logical-px`；Windows 按每段移动起点所在显示器的 UIX scale 换算。虚拟桌面可含负物理坐标，显示器之间的空隙不接受点击。
- 不提供跨请求按键或按钮持有。`text` 为 ASCII 键盘合成，仍受输入法影响。
- 截图身份不锁定应用焦点。输入已接受不代表应用效果已完成，需要核验返回图。
- 焦点不属于本工具包可控范围：Wayland 这条路线拿到的是整屏会话，没有第三方窗口目录，也没有前台窗口信号（`sessions window` 只覆盖 opt-in 的 UIX 应用）。因此 `computer_run` 的长批次一旦在发出期间被别的窗口抢走焦点，整批按键会落到那个窗口，工具包不会察觉、也无法预先阻止。不要把批次拉长到「用户可能中途切窗口」的时间尺度；确实需要长时间连续输入时，先与用户确认这段时间不要切换前台窗口。
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
