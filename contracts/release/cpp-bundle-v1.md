# C++ release bundle v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

当前可逆迁移 bundle 由主 CLI、三个受限 worker 和 descriptor compatibility
manifest 组成。bundle 不包含 Cargo manifest、Rust source 或 target。

`cpp-install-rollback-v1` 进一步规定固定产物、SHA-256 完整性检查、显式确认、
staging 安装、保留上一版和可逆目录交换；对应测试只在项目 `build` 夹具中运行。

这只证明 C++ 已迁移能力能独立构建、启动和进行可逆目录部署，不表示所有 capability
已完成，也不授权删除仓库中的 Rust 兼容实现。最终切换前仍需真实分发来源/签名、
技能入口、配置和全部 mutation capability 门禁。
