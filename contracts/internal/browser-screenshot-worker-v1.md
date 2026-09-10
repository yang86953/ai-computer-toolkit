# Browser screenshot worker v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

契约名：`act/browser-screenshot-worker/v1`。

父 Browser Screenshot Module 在任何 runtime 探测、profile 或文件操作前执行逐操作确认，
随后验证唯一 `target.url` 与 `path/width/height/timeoutMs/overwrite` 封闭参数集。Module
独占工具临时根下的空 profile 与最终目标同目录的 `CREATE_NEW` staging，并只把 URL、
有界视口、deadline、staging/profile 私有路径和重复确认通过 JSON stdin 交给固定 sibling
`ai-computer-toolkit-browser-worker.exe`。最终路径和 overwrite 不跨 worker 边界。

worker 拒绝未知字段、未知协议、未确认请求、非 http/https/file URL、越界 viewport/
timeout、非普通 staging，以及不位于固定工具临时根直接子目录的 profile。Chromium 路径
只从主机配置或固定安装候选私下发现；公开请求不能传 executable、argv、flag、profile
或 shell。worker 只构造固定 `--headless=new`、无首次运行、无默认浏览器提示、独占
user-data-dir、screenshot、window-size 与 URL argv 模板。

主进程在 worker 首条指令前把它绑定到 `KILL_ON_JOB_CLOSE` Job，Chromium 子树继承同一
Job。取消或外层 deadline 会终止并回收整树；worker 的内部 deadline 返回
`BROWSER_TIMEOUT`。成功前必须验证 PNG signature、IHDR 尺寸、24 字节下限、64 MiB
上限、前景不变并返回固定单行 JSON。父 Module 再核对 staging 实际长度与 worker 事实，
前景不变后才执行 write-through 原子提交。所有失败路径由 RAII 清理自有 staging/profile，
不会触碰用户浏览器 profile、标签页、Cookie、扩展或凭据。

Vikunja #1985 将 worker 协议入口自身直接产生的失败收敛到普通私有
`BrowserWorkerErrorCode`；失败 envelope 仍只包含稳定 `code` 与安全 `message`：

- `CONFIRMATION_REQUIRED`：缺少逐操作显式确认；
- `CAPABILITY_GAP`：协议版本不在固定认证集合；
- `BROWSER_UNAVAILABLE`：没有可用的认证 Chromium runtime；
- `BROWSER_START_FAILED`：认证 Chromium runtime 无法启动；
- `BROWSER_FAILED`：Chromium 执行或直接子进程监控失败；
- `BROWSER_TIMEOUT`：Chromium 超过 worker 内部 deadline；
- `INVALID_ARGUMENT`：请求 JSON、URL、viewport、deadline 或 stdin 边界无效；
- `INVALID_OUTPUT_PATH`：父 Module 预留的 staging 不再满足普通文件所有权；
- `TEMP_PROFILE_FAILED`：profile 不在工具自有临时根直接子目录；
- `SCREENSHOT_MISSING`：PNG signature、IHDR、尺寸或字节上限证明失败；
- `FOREGROUND_CHANGED`：隔离 Chromium 意外改变宿主前景。

上述十一项只归 worker 协议边界所有。BrowserRuntime、ByteDigest、前景门禁及其他下层错误
继续原样传播，再由父 Browser Screenshot Module 的既有白名单与公开转换失败闭合。十二个
直接 `AppControlError` 构造点归零，但 confirmation-first、固定 argv、退出码、envelope、
profile/staging、直接子进程与 Job 回收、PNG 证明和 RAII 清理均不改变。
