# MCP 直接控制

适用于 computer-control-toolkit 的桌面 MCP（Linux/Wayland 与 Windows 共用标准 MCP 入口，实机验收分别记录）；不假定其他主机已安装或授权桌面。工程通常在 `~/data/code/computer-control-toolkit`（GitHub `yang86953/ai-computer-toolkit`），以实况为准。

## 入口与接入

- MCP 服务：**就是 `ai-computer-toolkit` 二进制本身** —— 无参数启动即标准 MCP stdio 服务，不依赖 Python、不需要额外适配脚本。带参数启动则走常规 CLI。
- 客户端按标准方式配置即可，无需 `--binary` 之类参数：

```json
{
  "mcpServers": {
    "computer-control": {
      "command": "/absolute/path/to/ai-computer-toolkit"
    }
  }
}
```

- 连接前服务会检查 broker 的 `postInputObservation` 能力，旧二进制报 `BROKER_FEATURE_UNAVAILABLE`，不试发输入；源码存在、文件时间较新或增量构建命中缓存都不等于运行产物含该接口。
- 初始化与 `tools/list` 不连接桌面；只有显式 `computer_connect` 才建立 Portal 会话。协议版本支持 2024-11-05、2025-03-26、2025-06-18。
- 框架依赖、配置已登记、文件已部署和当前会话已加载是不同事实。以当前 `tools/list` 为准；需要时由用户在安全时重载或新开会话，不擅自打断活跃任务。

## 最小操作链

1. `computer_connect`：依据当前用户授权传 `confirmed`、`foregroundConsent`、`strictIsolation`，使用真实返回的 `sessionId`。此路线是前台桌面，不能承诺后台隔离；拒绝不换通道绕过。
2. `computer_observe`：直接返回 PNG 图片、`frameId`、尺寸；看图确认目标、窗口遮挡和焦点。默认最长边 1280，细节不足再增大。
3. `computer_interact`：直接移动/点击，或确定的小批次键鼠；`computer_keys` 发单键/快捷键；`computer_pointer` 做相对移动、滚轮和完整拖拽。输入后均返回截图，核验再决定下一步。
4. `computer_disconnect`：关闭本客户端桌面会话，验证空会话并释放 broker；不关闭被操作应用。

位置和按键以当前 `tools/list` schema 为准：

- `interact` 点击/移动坐标对应返回图的 `observation-px`，可直接使用该预览图坐标；不自行乘桌面缩放。
- `pointer` 相对位移是 `relative-logical-px`，与预览坐标不同。需要拖拽时先用截图坐标移动到起点，再按实际相对位移分段操作。
- 输入引用同会话最新 `frameId`；输入尝试后旧帧失效。失败后重新观察，不能盲目重复整个动作。
- 快捷键示例是 `keys: ["left-shift", "f5"]`，数字小键盘如 `numpad-1`。组合完整释放，不跨请求持有按钮。
- `interact` 的 `text` 仅合成 ASCII 键盘事件，不是 Unicode 粘贴，会经过输入法。确认英文输入模式再输入路径或名称；不依赖它执行应用代码来替代键鼠任务。

## 结果与异常

- `frameId` 绑定截图映射，不锁住应用焦点。其他窗口抢焦点、用户同时操作、弹窗或输入法变化时停下重新观察。
- 输入事件已发送及 `effectConfirmed=false` 都不是应用完成证据；动作后的单帧也可能早于应用刷新，按任务需要再次观察。
- 同客户端并发操作返回 BUSY，不排队继续未知流程；逐次检查结果，不把互相依赖的动作并行调用。
- MCP 取消绑定原请求，按键取消不等于回滚；Portal 打开与单帧捕获可能要等已有超时边界收尾。
- `OUTCOME_UNKNOWN`、断线、部分执行或输入完成但截图失败：不自动重放。仍有有效会话则只读观察；传输失效时说明需要安全重载/重新连接，不能把新实例当旧会话。
- 任务收尾用 disconnect。协议测试、实机截图与实际目标操作分开报告；客户端未加载当前工具或未做键鼠实测，不能声称已验收。
