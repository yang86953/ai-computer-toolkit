# ai-computer-toolkit 文档中心

现行产品、接入与架构文档在本仓库维护；项目管理系统保留任务、验收和链接入口，不维护第二份现行正文。

## 当前产品与接入

- [定位与边界](产品/定位与边界.md)
- [根目录 README：构建、平台、CLI 与 MCP](../README.md)
- [MCP 接入与工具契约](computer-control-mcp.md)
- [Linux 桌面快速调用](Linux桌面快速调用.md)
- [输入后观察](desktop-interact-observe.md)
- [帧变化](desktop-frame-changes.md) · [帧订阅](desktop-frame-subscription.md)
- [模块映射](module-map.md) · [SMC 采用摘要](架构/SMC采用.md)

## 版本化协议与实验参考

[contracts](../contracts/) 中 JSON Schema 与 Markdown 按协议版本组织；Markdown 迁自原项目资料，
保存字段、拒绝和生命周期设计，也包含历史候选与兼容路线，不是默认能力清单。
CLI 以当前 capability/版本化 schema 为准，MCP 以当前 `tools/list` 为准。
同名旧文档的“当前”“已验收”不代替本次构建和真实目标的验证。

[spikes](../spikes/) 是实验资料，不是默认生产入口；Windows 专属构建和其他可选 feature
也不因文档存在而进入默认安装。内部任务正文、私有回执、缺失的测试制品不在公开仓库伪造。
