# Linux release archive v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

本契约属于独立 Release System，不改变 Computer Control System 的公共控制 JSON。首发
`tar.zst` 只有一个主 CLI、零 companion；根目录只允许 schema 列出的六个文件（其中
`manifest.json` 自描述，其余五项由 `files` 逐字声明）。Windows worker、测试 fixture、
Browser、Portal、AT-SPI、PipeWire/libei worker 与 service 均不得进入制品。

安装器只读取绝对 `XDG_DATA_HOME` 与 `HOME`：受管版本位于
`$XDG_DATA_HOME/ai-computer-toolkit/releases/`，`current` 和 `rollback` 是根内相对链接，
唯一用户入口为 `$HOME/.local/bin/ai-computer-toolkit` 的受管固定链接。相对路径、链接或
非目录根、入口碰撞、非普通 archive entry、额外文件、target/mode/hash 不一致均失败闭合。
archive validator 必须同时证明外置 SHA 正确、输入只有一个 zstd frame、tar 在精确两个结束
块处物理耗尽；重算 SHA 的尾随字节、拼接 frame、结束块之后数据和未声明成员均失败闭合。

所有 mutation 使用 `XDG_DATA_HOME` 下固定的同用户互斥锁。install/upgrade/rollback/uninstall
先验证 marker、current、rollback、launcher、全部 release、mode/owner/link/hash 和 pending journal，
再写入 `prepared -> committing -> committed` journal。current/rollback 的两次 rename 由 journal
恢复到完整 old 或完整 new，不称为单一原子切换；write/chmod 后文件 fsync，staging/bin/releases、
链接与 launcher 的父目录在 rename 后 fsync。卸载先认证整个根，再经固定 tombstone 前滚，禁止
在预检完成前删除 release 或 launcher，也禁止跨越未认证根递归删除。

manifest 的 `elfPolicy` 由结构化 ELF parser 生成，并在 archive 与 installed-path 对实际 binary
重新核验 arch/interpreter、PIE、NX stack、RELRO、BIND_NOW、DT_NEEDED 精确 allowlist 与最大
GLIBC。SBOM 由 Cargo metadata 与 Cargo.lock 的 Linux normal closure 生成；SPDX 表达式必须由
版本锁定 parser 接受，未知值使用 `NOASSERTION` 加固定 comment/notice，完整文档通过仓库内固定
SPDX 2.3 严格子集 schema、checksum、ID 与 relationship closure 门禁。

`releaseEnvironmentVerified=false` 的 archive 只可作本机、临时前缀验收，不可发布。
正式发布还必须在 digest 固定的 Ubuntu 22.04 自有构建镜像中离线执行，且
`maximumGlibc <= 2.35`、ELF 与 archive 门禁均通过。当前本机最大需求为 GLIBC_2.39，故
`publishable=false`、`releaseEnvironmentVerified=false`。Gitea 无 runner，也没有已验证的
自有镜像 digest，因此仓库不声称远端 CI 或正式发布构建已经完成。
