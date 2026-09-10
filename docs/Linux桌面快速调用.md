# Linux 桌面快速调用

[← 文档中心](README.md)

本项目控制第三方前台桌面软件，不提供 UIX 应用后台控制或焦点隔离。
先按[构建要求](../README.md#build)取得当前二进制和 PipeWire 依赖。

## 推荐 MCP 入口

将 `ai-computer-toolkit` 的绝对路径配置为 MCP stdio command，不加参数即可启动服务。
初始化和 `tools/list` 不触碰桌面；显式 `computer_connect` 才按授权开启会话。
使用 `connect → observe → interact/keys/pointer → 核对返回图 → disconnect`，
参数以当前 `tools/list` 为准，详见[MCP 文档](computer-control-mcp.md)。

## 直接 CLI / broker

需要版本化 JSON 接入时，可以让同一二进制持有桌面 broker：

```bash
cargo build --locked --bin ai-computer-toolkit
target/debug/ai-computer-toolkit session-host desktop --socket
# 另一个终端向已就绪的宿主提交一个完整请求：
target/debug/ai-computer-toolkit session-call desktop --input request.json
```

由调用方持久运行时托管宿主；等待本次 `broker-ready` 并核对 transport、epoch、契约版本，
不能把进程启动当作会话已创建。请求绑定本次 epoch、新 nonce 和原会话身份。
具体字段见[broker schema](../contracts/v1/linux-desktop-session-broker-v1.schema.json)，
历史协议说明见[版本化参考](../contracts/v1/linux-desktop-session-broker-v1.md)。

`open` 仍需用户明确授权和系统 Portal 选择；不要重建已由 MCP 持有的第二个 broker。
操作结果不明时先观察，不能自动重新连接并重放输入。结束时显式关闭会话、核对空会话后关闭宿主。
本页不搬用旧主机性能数字，也不把某种终端的存活记录泛化为所有宿主保证。
