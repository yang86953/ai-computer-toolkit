# C++ install, upgrade, and rollback contract v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

安装包固定包含 C++ 主 CLI、三个受限 worker、兼容 descriptor manifest 及
`bundle-manifest.json`。安装和回滚必须分别显式传入 `-ConfirmInstall` 与
`-ConfirmRollback`。

安装器拒绝源/目标重叠、缺失或额外产物、重复产物、不匹配的 role/size/SHA-256。
升级先在目标同级目录完成 staging 验证，再把当前版本保留为
`<destination>.rollback`，最后切换新版本。已有 rollback 时拒绝继续覆盖，因此不会
静默删除已保留版本。回滚前再次验证 active 与 retained bundle，并通过不删除目录的
交换完成；再次回滚可切回原版本。

SHA-256 只证明 bundle 复制后与 manifest 一致，不是发行者签名，也不证明下载来源。
生产分发在缺少签名/可信来源策略时必须继续 fail closed。

当前契约测试只在项目 `build` 夹具目录中执行，不授权写系统安装目录、切换
`computer-control` 技能入口或删除 Rust 兼容实现。实际切换仍受逐 capability 等价
门禁约束。
