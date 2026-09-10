# SMC 架构采用

[← 文档中心](../README.md)

本项目自有 Rust 代码采用 SMC v2.0，记录基线为
`64f55bc5634bf5a647a37cfc296a4b75a4799734`，范围是 `all-first-party-rust`；
机器可读事实见 [Cargo.toml](../../Cargo.toml) 的 `package.metadata.smc`。
外部规范不随本仓库复制或重新授权；本页只描述本项目的组织决定。

- MCP/CLI 提供明确的 Command/Query 接口，服务层协调能力、会话和操作结果。
- Module 持有会话、目标代际、授权、步骤语义与取消/终态，不把平台对象泄漏到公开 JSON。
- Component/Adapter 负责 IPC、进程、Portal/EIS/PipeWire、WGC 和平台输入等资源边界。
- broker/worker 的持有者负责期限、回收和未知结果；接受、完成、取消请求和结果未知分别表达。

状态、失败、容量、关闭和恢复行为需要与改动风险相称的验证；目录分层或类型存在不等于验收。
不为形式统一增加三层对象、EventBus 或第二套输入 owner。候选与 feature 专属路径不冒充默认生产能力。
职责与源码入口见[模块映射](../module-map.md)，本页不复制带时点的任务验收报告。
