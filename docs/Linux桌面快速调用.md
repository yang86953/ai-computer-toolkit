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

## 一次授权持续复用与撤销

`open` 可选 `authorizationScope=session`：该次确认覆盖整条会话，后续 `observe`/`input-key`/
`input-pointer`/`interact`/`observe-subscribe`/`observe-next` 可省略确认字段并继承已授予作用域
（显式拒绝仍被拒绝）。可选 `rememberAuthorization=true`（仅 Linux Portal）按 `persist_mode=2`
记住授权：下一次连接自动尝试恢复；token 保存在当前用户私有状态目录并在每次成功 Start 后轮换，
绝不进入结果、日志或文件名。查询与撤销：

```bash
# 查看是否已保存可恢复授权（脱敏，不含任何凭据内容）：
echo '{"contractVersion":"act/linux-desktop-session-broker/v1","brokerEpoch":"<epoch>","requestNonce":"<32位小写hex>","operation":"authorization-status"}' > status.json
target/debug/ai-computer-toolkit session-call desktop --input status.json

# 撤销：清除本工具保存的凭据并停止该客户端全部 live 会话：
# 把 operation 换成 forget-authorization 即可。
```

本地忘记不撤销系统 Portal 侧的授权记录，后者需在桌面环境权限管理中单独处理；
语义与边界见[版本化参考](../contracts/v1/linux-desktop-session-broker-v1.md)。
