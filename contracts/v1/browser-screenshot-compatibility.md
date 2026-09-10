# Isolated browser screenshot compatibility contract

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

`browser.screenshot@1` 是旧 `run browser screenshot` 的 Rust 正式兼容实现，只接受
有界 `http://`、`https://` 或 `file://` URL，以及已有父目录中的 `.png` 输出。
它不附着用户浏览器、不读取用户 profile、标签页、扩展、Cookie 或保存的凭据。

执行顺序固定为：

1. confirmation-first；
2. URL、viewport 1–10000、timeout 1000–300000 和 overwrite 校验；
3. 私下发现已安装 Chromium；路径不进入公开结果；
4. 建立本次调用专属临时 profile 和 staging PNG；
5. 在 `KILL_ON_JOB_CLOSE` worker Job 中，以固定 Chromium 参数执行；
6. 超时或取消时终止 worker 与全部浏览器子进程；
7. 校验 PNG signature、64 MiB 上限与 IHDR 尺寸；
8. 前台不变后才原子提交公开输出，并清理临时 profile。

调用者不能传 executable、profile、Chromium flag 或 shell command。输出已存在且
`overwrite=true` 缺失时返回 `OVERWRITE_CONFIRMATION_REQUIRED`。超时返回
`BROWSER_TIMEOUT`，公开输出不存在，整个子进程 Job 已终止。浏览器意外改变前台时
返回 `FOREGROUND_CHANGED`；提交前检测到时不产生公开文件。

成功结果不返回 browser path、profile path、PID、HWND 或前景 native identity。
兼容 launcher 将全部 `browser.screenshot` 调用路由 Rust。网页日常交互仍应使用
Browser/Chrome 控制面；该入口仅保留旧的一次性隔离截图契约。

Rust 动态门禁用仓库自有 HTML 与浏览器 fixture 生成 640×360 PNG，并通过 launcher
重复正式路由；实机 Chromium 对同一 HTML fixture 也生成 640×360 PNG。工具自有挂起
fixture 证明超时会终止整个 Job，不留公开输出或临时 profile。历史 C++ browser
实现与像素等价门禁已废弃，不再是构建、测试或发布前置条件。
