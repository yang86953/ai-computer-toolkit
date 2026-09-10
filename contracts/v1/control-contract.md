# 计算机控制公开契约 v1

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

## 产品语义

产品承诺“广泛通用、按能力降级”，不承诺任意软件、任意状态、任意操作都能成功。
每次写操作必须先完成能力探测、精确目标解析和策略评估，再返回以下决策之一：

| 决策 | 含义 |
| --- | --- |
| `executable-background` | 已有认证的后台实现；仍按 capability 要求确认 |
| `confirmation-required` | 路径可用，但状态变更尚未取得明确确认 |
| `foreground-consent-required` | 只有受控前台路径可用，必须先说明影响并取得同意 |
| `isolation-required` | 严格零打扰请求只能交给认证的隔离 worker |
| `permission-blocked` | 当前完整性级别、系统权限或应用权限不允许 |
| `unavailable` | 目标已知但当前状态、后台或依赖不可用 |
| `unsupported` | 精确目标未发布所请求的 capability |
| `capability-gap` | 公共 capability 目录没有该领域操作 |

## 稳定外部边界

- 进程间公开协议是版本化 JSON over stdio；stdout 只输出一个 JSON 结果。
- 失败使用非零退出码和结构化错误；诊断信息不得混入 stdout。
- 公共类型只包含拥有型值、字符串、整数、布尔、数组和对象。
- 所有 `--input <file|->` JSON 来源统一限制为 16 MiB；文件和 stdin 都在解析前执行
  `最大值+1` 有界读取，超限返回 `INVALID_ARGUMENT` 及 `maximumBytes/observedBytes`。
- 不公开 HWND、PID 作为写目标，不公开 COM/WinRT/UIA/第三方对象、ProgID、
  provider 路由键、任意脚本、菜单 ID、窗口消息号或可执行路径。
- `targetId` / `sessionId` 是 opaque、版本化、可重新解析并检测 stale 的精确目标。
- 公共 ABI 不依赖 C++、Vlang、Zig 或 Rust 的语言 ABI；未来嵌入式接口如有需要，
  另行定义版本化 C ABI，不复用内部类或容器 ABI。

兼容 CLI 在迁移期继续接受现有命令：

```text
catalog | methods | describe | doctor | status
sessions <surface>
inspect <surface>
run <surface> <operation>
sequence --input <file|->
```

Vikunja #1990 只把 CLI 参数、兼容 surface、逐操作确认和有界 JSON 来源映射产生的
`INVALID_ARGUMENT`、`CAPABILITY_GAP`、`CONFIRMATION_REQUIRED` 与
`INPUT_READ_FAILED` 收敛到私有封闭 `CliErrorCode`；`src/cli.rs` 与
`src/cli/json_input.rs` 的三十六个生产直接 `AppControlError` 构造表达式归零。既有
code、message、空或安全 details、退出码、单一 stdout、pretty、兼容命令、strict
isolation、confirmation-first 和下层 System/Policy/Module/Component 错误传播保持不变。

`sequence` 当前固定使用 `act/sequence-workflow/v1`，输入必须满足封闭的
`sequence-input.schema.json`：步骤数为 1–64，未知字段在任何步骤执行前拒绝，结果
负载默认使用 1 MiB、可配置为 256 字节至 16 MiB 的总预算。预算耗尽是不可放宽的
停止条件，并区分 provider 步骤事实与结果/错误负载省略。每步可声明最多 16 个
有界 JSON Pointer `exists`/`equals` 后置断言，还可对严格更早步骤的成功结果声明最多
16 个同边界前置条件，并可用最多 16 个有界绑定把更早成功结果替换进当前 step 的
`target/args`：旧 `destinationField` 保留已声明顶层字段替换，新 `destinationPointer`
允许在静态存在的对象父路径下创建或替换叶字段；根、数组、缺失父对象和重叠路径均拒绝。
每个绑定的来源恰好是直接 `sourceStep/sourcePointer` 对，或 1–32 个封闭 literal/source
段的结构化 `template`。模板只对静态 `status`、`sessions`、`inspect` 开放，至少含一个且
最多含 16 个严格字符串 source，静态文本和最终字符串分别受 4096 UTF-8 字节上限约束；
它不解释表达式或脚本，也不执行隐式类型转换。
绑定不能修改操作、确认、前台同意、隔离或读取上限；最终
请求仍经过统一 Policy。前置条件或绑定失败时当前步骤不启动；后置断言失败保持
provider 成功事实；这些工作流错误都硬停止且不受 `continueOnError` 放宽。公开输入另有
`totalTimeoutMs` 和每步 `timeoutMs`，生产 Workflow 通过固定 Rust worker 和双阶段
accepted/final 协议执行较早 deadline 与取消传播；Windows 使用 Job-bound sibling，Linux
使用 `/proc/self/exe` 固定隐藏自进程与独立 process group，不增加 companion；accepted 后缺失 final 时只
能报告 `OUTCOME_UNKNOWN`，不得自动重试。完整语义和预算优先级见
`sequence-workflow.md`。本版本不宣称事务、回滚、补偿或幂等；`run` 模板与动态值物化后的
交互式重新确认仍未开放。

新核心增加无副作用的 `assess` 语义。旧 `run` 在执行前必须经过同一 assessment，
不得绕过目标、确认、前台、隔离或权限判断。

Rust `doctor` 与各 surface 的 `status` 只发布当前运行事实；doctor 顶层固定为
`ok/policy/results`，任何层级都不得出现 `cppPolicy`、`cppStatus`、
`cppExecutionEnabled`、`allCppExecutionAvailable`、实现语言、迁移版本或构建版本。
隐私门禁必须同时扫描成功信封和已声明的结构化不可用信封；不得为了取得零退出码而
恢复未认证 provider，也不得把 `BACKGROUND_OPERATION_UNAVAILABLE` 改写成成功。
直接 `status` 错误按主入口映射为退出码 `3`；`doctor` 保持 `ok/policy/results` 聚合形状，
当唯一结果不可用时顶层 `ok:false` 并使用聚合失败退出码 `2`，错误码保留在该结果内。
这些构建/迁移元数据分别归属 Rust 正式 `build-info`（Vikunja #569）与
`capabilities`（#568）；两者缺少 Rust runtime 时都失败闭合，不回退 C++。Rust 运行诊断 schema
为 `runtime-diagnostic-envelope.schema.json`；现有 `doctor-result.schema.json` 只保留为
C++ 直接兼容对照，不得套用到 Rust runtime。

#1963 仅把 `capabilities` Module 直接产生的 `INVALID_ARGUMENT`、`OPERATION_FAILED`
与 `SERIALIZATION_FAILED` 收口到私有封闭类型；公开错误码、消息、空 `details`、
surface/method/descriptor 信封、legacy companion 状态与运行诊断隔离契约均不变。

控制契约定义以下迁移期命令；`discover`、`assess`、`inspect` 与 `inspect-tree` 的
launcher 权威实现现为 Rust，历史 C++ 同名入口仅作为待删除资产：

```text
discover app [--max-applications n] [--max-processes n] [--max-windows n]
sessions app [--max-items <1..4096>]
assess app --capability <capability@version> --target sessionId=<opaque>
inspect app --target sessionId=<opaque-window> [--timeout-ms <1..30000>]
inspect-tree app --target sessionId=<opaque-window> --max-depth <0..20> --max-items <1..4096> --view <control|raw> [--timeout-ms <1..30000>]
preflight-capture app --target sessionId=<opaque-window>
probe-capture-frame app --target sessionId=<opaque-window> --confirm [--timeout-ms <1..30000>]
run app create --input <file|-> --confirm
run app apply --input <file|-> --confirm [--allow-foreground]
run notepad open-and-write-text --arg text=<utf8> --confirm
```

`discover app` 返回 host → installed application → running process → visible titled
window 关系图。无窗口进程必须保留；无法保守关联的进程标记为 `unassociated`，不得
猜测。已安装应用同时使用 Windows uninstall registry 传统桌面记录、公开 Shell
AppsFolder，以及当前用户与公共 Programs 两个固定 Start Menu Known Folder 中的普通
`.lnk`/`.appref-ms` 入口；后两者提供封闭的 Shell 启动来源，但不等同于完整包管理
清单。Start Menu opaque 身份绑定固定目录项及文件代际，路径和 Shell parsing identity
只保留在私有 Component。AUMID 仅可作为 Component 私有去重事实，不得公开或用作
调用目标。

统一 assessment 是同一 opaque 身份上的两步只读契约，而不是复制全部 provider 字段的
单一巨型响应：`discover app` 提供当前目录来源、公开版本/发布者、进程权限与相对完整性，
`assess app` 重新发现同一精确目标并输出 capability 的 decision、realm、确认与前台限制。
capability ID 必须先由当前运行时 registry 精确解析；未知 ID（包括仅仅形似
`name@version` 的字符串）返回 `INVALID_ARGUMENT`，不得降级为普通 `unavailable`。只有已登记
capability 在当前平台/目标没有 provider 时，才使用 realm `none`、fallback `none` 的结构化
unavailable。
目录中的 `publisher` 只是 Registry/Shell 声明元数据，不是 Authenticode 或可信来源验签；
缺少经认证 executable 来源时签名维度必须保持不可用，不得把 publisher、AUMID、路径或
SHA-256 冒充发行者签名。这样动态目录、私有身份、版本/发布者、权限及前后台限制均有
明确状态，同时不扩大公共平台类型或授权边界。

`assess` 必须先重新解析 opaque 精确目标。stale 目标返回结构化错误；有效目标上的
结果严格使用 assessment schema 的八种 decision。未迁移写能力只报告
`unavailable` 和兼容入口证据，不因此启用写路径。

`application.session.discover@1` 是阶段 3F 已迁回 Rust 的只读统一 app session 聚合。
`sessions app` 只复用当前已注册 provider 的 `sessions` 入口，按固定类别排序并在
`max-items 1..4096` 内截断；单 provider 失败进入结构化 `warnings`，不得静默改走第二套
provider 或前台输入。顶层与迁移期 `data` 外壳必须来自同一事实，且不得公开 PID、HWND、
AUMID、路径、provider 路由键或任何平台类型。完整形状与动态门禁见
`application-session-discovery-compatibility.md`。

Linux 不复用或扩写上述 @1。`application.session.discover@2` 只在显式 capability 选择时聚合
当前 `s2:h`、XDG `application.discover@2` 的 `s2:a` 与 procfs `process.discover@1` 的
`s2:p`；应用/进程关系固定 none/unassociated，该聚合未接入窗口来源且空数组不代表主机没有
窗口。完整版本二形状见 `../v2/application-session-discovery.md`。

`application.session.discover@3` 同样只在显式 capability 选择时执行，并保持上述 XDG/procfs
事实不变；它额外聚合当前用户主动发布且通过认证的 UIX Agent 窗口，把认证 peer 的 PID 仅在
Adapter 内重新解析为 opaque 进程代际，从而发布精确 window→process 关系。它不推断应用关系，
也不宣称 compositor 全局窗口覆盖；完整形状见 `../v3/application-session-discovery.md`。

`text.document.create@1` 是阶段 4A 已迁回 Rust 的确认型写能力。`sessions app` 发布不透明
`s2:a:*` 创建器，`assess` 返回 `confirmation-required/host-background`；
执行只创建并回读本次工具拥有的新临时 UTF-8 文件，再以 no-activate 请求打开固定
系统 Notepad。不得附着、读取或修改已有文档，不得使用键鼠、剪贴板、UIA 写、
注入、提权或任意 executable。launcher 的 app 与 legacy 路由均固定选择 Rust，
缺少 Rust runtime 时不得静默回退 C++。完整兼容与回滚语义见
`text-document-create-compatibility.md`。

Rust `sessions app` 的主机目标沿用 canonical `s2:h:*` 身份；历史 C++ inventory
`hostTargetId` 只解释兼容来源。host 只承担观察聚合，不再发布静态
`application.open@1` 白名单。
启动 capability 只绑定 `discover app` 当前返回、来源为 Shell AppsFolder 或固定 Start
Menu Programs、且带认证私有 Shell identity 的精确 `s2:a:*`；执行时必须再次完整枚举
并唯一解析，不接受公共 path、argv、shell verb、AUMID、PIDL 或 URI。完整身份与隐私约束见
`host-session-compatibility.md` 与 `application-launch-migration.md`。

结构化图像 provider 的 Rust 与 C++ inventory 共用 canonical `s2:a:*` 应用身份
和 `s2:d:*` 文档身份。两者都绑定进程创建 FILETIME；文档身份还绑定原生
文档 ID、当前名称和源路径。这些原生事实只参与指纹与重新解析，不得出现在
`sessions app` 或 `inspect app` 的公开 JSON 中。完整迁移契约见
`structured-image-output-migration.md`。

反向迁移 launcher 的 `discover app`、`assess app`、`sessions`、`inspect` 与
`inspect-tree` 全部只读/决策 surface 固定由 Rust 主入口处理，不依赖 C++ bundle。
历史 C++ 主入口不参与认证或运行。缺失 Rust runtime 时 launcher 必须返回
`COMPATIBILITY_RUNTIME_UNAVAILABLE`，不得将只读请求静默改投 C++。
生产 launcher 不得解析 C++ executable、保留语言选择状态或发布
`CPP_RUNTIME_UNAVAILABLE`；所有命令无条件进入 Rust 主入口。

所有 provider 资源 `s2` 目标都按使用时重新发现判定有效性，不使用墙钟 TTL。FNV-1a
指纹只是路由键：零命中为 stale，多命中为 `AMBIGUOUS_TARGET`，不得任取一个。
进程生命周期目标必须绑定创建 FILETIME 或等价代际事实。完整契约见
`opaque-target-validity.md`。

`s2:o:*` 专用于同一登录会话的长操作 broker 任务记录，不是 provider 资源或权限凭据。
其状态机、dispatch 前失败与 dispatch 后未知、幂等取消、1 MiB 结果预算、24 小时终态
保留和恢复顺序由 `long-operation.md` 冻结。Vikunja #2013 已交付契约和纯 Rust 状态机，
#2016 已交付原子 journal 与有界 registry，#2017/#2018 已交付固定 broker 与主 launcher
跨调用 status/cancel 客户端；#2015 已启用 broker 内部异步录制、逐任务取消、broker-owned
Rust worker 回收和公开 `operation start window.record@1`。同步 `window.record@1` 继续保留；
start 写入开始后的断连/timeout 不得自动重提，必须返回 acceptance-aware `OUTCOME_UNKNOWN`。

`inspect-tree` 只返回有界 UIA 结构属性：snapshot-scoped node ID、depth、name、
automation ID、class、framework、control type、enabled 和 offscreen。它不读取
Value/Text 内容，不公开 bounds、PID、HWND，不查询 Invoke/Value/Text pattern，
也不产生可复用的写目标。权限拒绝必须分类为 `PERMISSION_DENIED`，当前 provider
不可用映射为 `BACKGROUND_OPERATION_UNAVAILABLE`。所有 UIA/provider 调用必须在
专用 companion worker 中执行；主进程用 Windows Job 约束 worker 生命周期。超过
deadline 返回 `TIMEOUT`，Ctrl+C 或内部 cancellation token 返回 `CANCELLED`，
两者均须终止 worker 后才能返回，且不得留下后台 UIA 调用继续运行。worker 不接受
原生目标，只重新解析 opaque session；协议固定为
`act/observation-worker/v1` JSON over stdio。

## 执行域与严格隔离

公开请求通过 CLI `--strict-isolation` 或 JSON `isolationRequirement:"strict"`
要求严格零打扰；CLI 已要求严格时，JSON 不得把它降为 `standard`。运行时只使用
`host-headless`、`host-background`、`same-session-no-focus`、`isolated-worker`、
`host-foreground` 和 `none` 六种强类型执行域，主机影响策略只使用
`background-preferred` 与 `strict-no-interference`。完整结果证据 schema 固定在
`execution-policy.schema.json`。

System 必须在 target/provider 解析和任何动作之前冻结执行计划。严格模式只允许
已经认证的 `host-headless`，或认证路由和 companion 均可用的
`isolated-worker`：其他执行域返回 `ISOLATION_REQUIRED`；隔离路由未认证或 worker
缺失/不可达返回 `ISOLATED_WORKER_UNAVAILABLE`。`--allow-foreground` 与请求中的
`foregroundConsent` 永远不能降低严格要求，也不能把失败重试成同会话或前台输入。

Vikunja #1964 只把 System 自己产生的 `INVALID_ARGUMENT`、`OPERATION_FAILED` 与
`SERIALIZATION_FAILED` 选择收敛到私有封闭类型；既有公开 code、message、details，
确认优先级、冻结计划证明及下层 Policy/Module/Adapter 错误传播语义均保持不变。

成功结果必须由 System 附加 `executionRealm`、`requiredExecutionRealm`、
`executionRealmCertified`、`isolationRequirement` 和 `hostImpactPolicy`。正式契约要求
隔离、但尚在兼容进程内运行的标准请求必须如实报告实际域与未认证状态；相同路线在
严格模式下不得进入 provider。认证 Rust 隔离路线当前包括只读 UIA inspection、
`accessibility.tree.read@1`、`ui.element.locate@1`、`ui.element.wait@1`、browser screenshot、精确窗口截图与
精确窗口录制；media 路线在迁回 Rust 并通过隔离认证前继续失败闭合。
Vikunja #1965 只把 Accessibility Module 的十种稳定错误类别、七种 worker 失败白名单
输入与六种内部错误公开转换收敛到私有封闭类型；未知 worker 码在 worker 边界仍先收敛为
`WORKER_PROTOCOL_FAILED` 且不回显 provider 消息，再按既有公开转换成为
`OPERATION_FAILED`。Job/stdio/deadline/取消/整树回收、root/tree 隐私字段和
`Accessibility Wait` 采样语义均保持不变。

`ui.element.locate@1` 是独立的 provider-neutral 只读 Query，不改变
`accessibility.tree.read@1` 的无 bounds 契约。输入只接受
`name|automationId|className|frameworkId|controlType` 精确 AND selector 与有界搜索参数；
主 Module 和隔离 worker 每次执行都重新解析精确窗口。零匹配成功返回 `missing`，两个
匹配返回 `AMBIGUOUS_TARGET`，截断或属性不完整返回 `SEARCH_INCOMPLETE`，窗口失效返回
`STALE_SESSION`。唯一结果的 `s2:e` 仅是观察快照；bounds 和 provider ClickablePoint 使用
允许负值的虚拟桌面物理屏幕像素并携带当前窗口身份材料、DPI 上下文和重新定位要求。该路线不读
Value/Text、不查询或调用写 pattern、不设置焦点，也不把遮挡未知解释为无遮挡。输入、
结果和详细语义分别由 `ui-element-locate-input.schema.json`、
`ui-element-location.schema.json` 与 `ui-element-location.md` 冻结。

`ui.element.action@1` 是统一 `app.apply` 上独立的确认式 mutation，不改变 `uia` surface
和 `accessibility.tree.read@1` 的只读契约。输入只能使用原 canonical `s2:w:*`、与
GC-LOC-001 相同的精确 AND selector 及 invoke/value/toggle/select/scroll 五种封闭动作；
snapshot `s2:e:*`、坐标、provider identity 和平台 pattern 都不是授权目标。System 在
dispatch 前冻结 capability、确认、权限、stale、前景和 `same-session-no-focus` 执行域，
严格隔离请求在 provider 解析前返回 `ISOLATION_REQUIRED`。私有 Job worker 再次唯一解析
窗口和元素，只发起一次对应 UIA pattern 调用；不支持或 Value 只读时返回
`ACTION_UNSUPPORTED`，不得静默回退指针。方法可能已经被调用后的 provider 异常，以及
无法证明未 dispatch 的 timeout、取消、进程或协议失败，统一返回 `OUTCOME_UNKNOWN`、
`acceptedMayHaveOccurred=true` 和禁止重试证据。输入、结果、公开语义与私有协议分别由
`ui-element-action-input.schema.json`、`ui-element-action.schema.json`、
`ui-element-action.md` 和 `semantic-action-worker-v1.md` 冻结。

`ui.input.pointer@1` 是统一 `app.apply` 上的确认式 `host-foreground` Command。正式输入
只接受原 canonical `s2:w:*`、屏幕或窗口客户区物理像素，以及 move/button/click/
scroll/drag 五种有界步骤；左右中键的显式按下必须在同一请求中配对释放，短命 CLI 不
拥有跨请求按钮状态。确认和前景同意先于 input、target、权限和 Adapter；每个点都按
Per-Monitor-V2 当前窗口事实重新转换，首次接触必须实际命中目标或其子窗口。恢复、激活
或 dispatch 开始后的 timeout、取消、前景变化、stale 和平台拒绝统一返回
`OUTCOME_UNKNOWN`、已完成步骤、不可重试及 best-effort 安全释放证据。统一 capability
不再静默委托旧 `desktop.click`，也不接受 HWND、PID、原生输入标志或特定软件命令。
输入、成功结果与完整语义分别由 `pointer-input.schema.json`、
`pointer-input-result.schema.json` 和 `pointer-input.md` 冻结。

`ui.input.key@1` 是同一统一入口上的确认式 `host-foreground` 同步 Command。正式输入只
接受原 canonical `s2:w:*`、完整 provider-neutral 命名键、key/chord/text 三类有界步骤和
同请求配平的 down/up；快捷键按调用方顺序按下并逆序释放，Unicode 文本按 scalar 边界
调度。确认和前景同意先于 input、target、权限和 Adapter；每次新按下或文本 scalar 前都
重新解析窗口并核对前景，释放不会被取消、deadline 或前景变化阻止。恢复、激活或 dispatch
开始后的失败统一返回 `OUTCOME_UNKNOWN`、不可重试及 best-effort 逆序安全释放证据。
统一 capability 不再委托旧 `desktop.press-key`，也不接受 virtual key、scan code、原生
输入标志或特定软件命令。输入、成功结果与完整语义分别由 `key-input.schema.json`、
`key-input-result.schema.json` 和 `key-input.md` 冻结。

`window.lifecycle@1` 是统一 `app.apply` 上独立的确认式 `host-foreground` 同步 Command，
只拥有 restore/minimize/maximize/move/resize；不可逆关闭继续由 `window.close@1` 独立
拥有。确认和前景影响同意先于 input、target、权限与平台解析；正式目标只接受当前完整
窗口 inventory 中唯一重解析的 canonical `s2:w:*`，PID、HWND、style 与 native flag
不得进入公共协议。状态动作使用固定系统命令，几何动作使用 Per-Monitor-V2 虚拟桌面
物理像素、带符号多显示器坐标与目标当前 DPI 下的系统最小 tracking size；最小化、
最大化或不支持缩放的当前状态必须结构化拒绝，不能隐式恢复或改走键鼠。

平台调用成功返回是 `accepted=true` 的事实建立点，不等于最终状态已完成。Module 必须在
同一当前窗口 token 与进程代际下精确读回状态或外框后才返回 `finalStateReached=true`；平台接受后的取消、
deadline、stale、权限变化或读回失败返回不可自动重试的 `OUTCOME_UNKNOWN`。后台目标只
允许前景保持原值或转移到同一个精确目标，调用前已经是前景的目标只有最小化可由 shell
选择后续前景；其余第三方竞争保持 `HOST_INTERFERENCE_DETECTED`。成功结果携带
`targetIdentityStrength` 并明确完全相同 token 回收未获保证，详情同样明确
`accepted=true`、结果未知且禁止重试。成功结果如实报告
`foregroundChangedDuringDispatch`。输入、成功结果和
完整语义分别由 `window-lifecycle-input.schema.json`、
`window-lifecycle-result.schema.json` 与 `window-lifecycle.md` 冻结。

`process.terminate.graceful@1` 与 `process.terminate.force@1` 是统一 `app.close` 上两个独立
的确认式同步 Command，只接受可重新解析且绑定创建代际的 canonical `s2:p:*`。优雅路径
只向精确进程当前顶层窗口投递固定关闭请求；强制路径才使用内核进程终止，两者不能由 input、
timeout 或 provider 静默切换。公开 input 只允许 1–30000 ms 的可选 `timeoutMs`，不接受
PID、path、argv、shell、signal、exitCode、native handle 或自动升级字段。

Process Lifecycle Module 在 dispatch 前保护当前工具、PID 0/4、Windows critical process、
高完整性与不可认证目标，并在平台接受后只以同一私有进程句柄退出作为完成事实。接受后的
取消、deadline、等待失败、未完成提示或前景变化统一返回不可自动重试的
`OUTCOME_UNKNOWN`；优雅路径绝不补发强制终止。成功结果提供原 opaque target 与
`process.discover@1` 重新观察提示，不公开 PID、句柄、路径、退出码或原生错误。输入、成功
结果与完整语义分别由 `process-termination-input.schema.json`、
`process-termination-result.schema.json` 与 `process-termination.md` 冻结。

Vikunja #1982 只把 `act/observation-worker/v1` 协议入口自身直接产生的六项错误定义
收敛到私有封闭类型；十四个 `AppControlError` 直接构造点归零，HRESULT 到权限拒绝、
目标过期或 provider 不可用的分类也由同一类型驱动。父 Accessibility Module 的 worker
白名单与公开转换、下层 Windows inventory 错误传播、v1 单行 envelope、退出码、COM/UIA
cache、Job/stdio/deadline/取消和整树回收生命周期保持不变。
Vikunja #1966 只把 Accessibility Wait Module 的输入、搜索完整性、内部成功证据、歧义、
取消与超时六种稳定错误收敛到私有封闭类型；既有 code、message、details、selector
隐私、零/一/多匹配、连续稳定状态、取消与总 deadline 语义均保持不变，Accessibility
Module 与 worker Component 错误继续按既有边界传播。
Vikunja #1967 只把 Window Closed Wait Module 的输入、取消、超时、目标歧义与内部状态机
五种稳定错误收敛到私有封闭类型；六个直接构造点的既有 code、message、details、采样
时序与隐私语义保持不变，窗口观察、前景检查、稳定等待与取消 Component 错误继续按既有
边界传播。
Vikunja #1969 只把 Window Close Module 的清单边界、目标解析、输入、确认、平台失败映射、
前景拒绝与宿主干扰九种稳定错误收敛到私有封闭类型；十五个直接构造点的既有 code、
message、details、failure 映射与执行顺序保持不变，Windows Adapter 与下层发现错误继续
按既有边界传播。
Vikunja #1970 只把 Window Record Module 的确认、输入、捕获/编码可达性、worker envelope、
watchdog timeout 与多产物事务错误收敛到私有封闭类型；十五个直接构造点、二十七项
worker 失败白名单和外层 `TIMEOUT` 分类由同一类型驱动，既有 code、message、空 details、
未知码失败闭合、隐私与执行顺序保持不变。
Vikunja #1972 只把 Browser Screenshot Module 自己产生和筛选的十三项错误定义收敛到
私有封闭类型；十二个直接构造点、九项 worker 失败白名单和外层 `TIMEOUT` 分类由同一
类型驱动，既有 code、message、空 details、未知码失败闭合与执行顺序保持不变。
Vikunja #1985 只把 `act/browser-screenshot-worker/v1` 协议入口自身直接产生的十一项
错误定义收敛到私有封闭类型；十二个 `AppControlError` 直接构造点归零。父 Browser
Screenshot Module 的九项 worker 白名单和公开转换、下层 runtime/摘要/前景错误传播、
confirmation-first、固定 Chromium argv、v1 单行 envelope、退出码、profile/staging、
直接子进程与 Job 回收、PNG 证明和 RAII 清理生命周期保持不变。
Vikunja #1973 只把 Text Document Module 自己产生的六项错误定义收敛到私有封闭类型；
十五个直接构造点（其中两个带安全 details）由同一类型驱动，既有 code、message、
details、confirmation-first、写前拒绝、artifact 事务和下层 Adapter 错误传播保持不变。
Vikunja #1974 只把 Standard Edit Module 自己产生和映射的九项错误定义收敛到私有封闭
类型；二十三个直接构造点（其中五个带安全 details）由同一类型驱动，既有 code、
message、details、opaque 重解析、权限门禁、mutation 顺序和平台失败映射保持不变。
Vikunja #1975 只把 Video Recording Adapter 自己产生和映射的十一项错误定义收敛到
私有封闭类型；二十九个直接构造点由同一类型驱动，既有 code、message、空 details、
Component 错误翻译、分析产物和原子提交生命周期保持不变。
Vikunja #1976 只把共享 Window Capture Adapter 自己产生和映射的十六项错误定义收敛到
私有封闭类型；`window_capture.rs` 的三十六个直接 `AppControlError` 构造表达式归零，
二十五个平台错误 mapper 调用改为封闭类型入参。既有 code、message、安全 details、
WGC/D3D11/WinRT 资源所有权、像素/元数据路径与系统隐私指示器语义保持不变。
Vikunja #1979 只把 `act/capture-worker/v1` 协议入口直接产生的五项错误定义收敛到私有
封闭类型；十八个 `AppControlError` 直接构造点归零。下层窗口、捕获和前景错误继续原样
传播，既有 code、message、空 details、单行 envelope、退出码、staging 与 Job 生命周期
保持不变。
Vikunja #1981 只把 `act/recording-worker/v2` 协议入口直接产生的六项错误定义收敛到私有
封闭类型；十个 `AppControlError` 直接构造点归零，极端序列化 fallback 的
`WORKER_PROTOCOL_ERROR` 也由同一类型驱动。下层 RecordingConfig、Window/Capture/Video
Recording Adapter 错误继续原样传播，既有 code、message、空 details、单行 envelope、
退出码、Job、staging 与多产物事务生命周期保持不变。
Vikunja #1988 只把 RecordingConfig 自身参数/路径验证和 Output Guard 翻译产生的
`INVALID_ARGUMENT`、`OVERWRITE_CONFIRMATION_REQUIRED` 与 `OPERATION_FAILED` 收敛到
私有封闭类型；十七个直接 `AppControlError` 构造点归零。既有 code、message、空 details、
失败优先级、字段白名单、覆盖许可和下层 Component 错误分类保持不变；Window Record
Module、Recording Worker、Video Recording Adapter 与 Policy 的传播和生命周期语义不变。
`window.closed.wait@1` 不需要 provider worker，在已认证的 `host-headless` 域逐次重新
枚举并唯一匹配精确窗口；成功仅表示目标连续无法从可见有标题窗口 inventory 解析，
不得推导原生 HWND 已销毁，也不得激活窗口、发送消息、输入或写文件。

版本化策略清单位于 `tests/contracts/strict-isolation-policy-v1.json`；真实 launcher
门禁只经 `tools/Invoke-ComputerControl.ps1` 运行
`tools/Test-RustIsolationPolicy.ps1`，覆盖主机无头成功、真实 observation worker
成功、同会话拒绝、未认证 worker 拒绝、前台不变、worker 回收与原生身份不泄漏。

## 确认优先

`ComputerControlSystem` 的统一 Policy 是公开基础确认策略的唯一权威：目录中每个
`mutates:true` 操作必须同时声明 `requires_confirmation:true`，每个
`mutates:false` 读取操作不得要求变更确认。未确认的变更请求必须在解析目标、参数或
provider 之前返回 `CONFIRMATION_REQUIRED`，避免无效请求触达任何领域行为或外部边界。

前台许可、严格隔离和覆盖确认是相互独立的门禁。它们沿固定策略顺序返回各自错误，
不得为制造基础确认错误而改变既有优先级；基础确认通过后，保存或导出覆盖仍须在创建
输出前取得单独的覆盖确认。领域 Module 可以重复检查确认以纵深防御，但不得成为与
System 公开策略冲突的第二权威。

全目录不变量由
`tests/policy.rs::all_catalog_mutations_require_confirmation_before_request_fields` 锁定；
测试遍历所有公开操作，并为每个变更操作验证空目标、空参数请求首先得到
`CONFIRMATION_REQUIRED`。

Rust 文件输出的独立覆盖许可由 `output_guard` Component 统一只读检查。缺失目标允许
首次输出；既有真实普通文件在 `overwrite=true` 缺失时返回
`OVERWRITE_CONFIRMATION_REQUIRED`；目录、符号链接与特殊目标不得因覆盖许可被当作
普通文件。desktop/browser screenshot、recording 和 structured-image 的 PSD/PNG
路径共用该门禁，具体 writer 仍须在最终提交点复核竞态。详细内部契约见
`contracts/internal/output-overwrite-guard-v1.md`。

## 前景不变

统一 `app` facade 必须在 `status`、`sessions`、`inspect` 以及所有非
`host-foreground` capability 的 provider 调用前后比较进程私有前景身份。只有身份一致
的结果才能进入公开成功响应；不一致时 Rust 主契约返回
`HOST_INTERFERENCE_DETECTED`，旧兼容 surface 可以映射为 `FOREGROUND_CHANGED`。
任何响应和错误都不得公开前后 HWND、PID 或 provider identity。

已成功返回的 mutation 若随后检测到前景变化，不能撤销或断言目标未变；错误必须保留
`outcome:"completed"`、`retrySafe:false` 与 `targetMayHaveMutated:true`，调用方不得
自动重试。只读结果被拒绝时不产生 mutation outcome，可在重新发现目标后重试。
`host-foreground` capability 因公开执行计划允许可观察前景变化而显式豁免此不变量，
但仍受确认、前台许可、精确目标、权限与结果证明约束。

领域 Module 和 worker 的更窄前景复核继续作为纵深防御，并拥有各自更精确的 timeout、
回滚和 outcome 语义；facade 门禁不得覆盖更早返回的领域错误，也不得把后台失败降级为
前台输入。

## 静态权限评估

同会话后台 mutation 的权限 assessment 必须是无主动写探针的纯决策。Standard Edit
Module 只把进程元数据访问状态与相对完整性事实映射给窄 Component：元数据可用且目标
较低或同级时返回 `requires-confirmation`；目标较高时返回 `permission-blocked`；关系
未知时返回 `indeterminate`。元数据权限拒绝优先返回 `permission-blocked`，元数据不可用
优先返回 `indeterminate`，不得使用完整性输入猜测或尝试提权。

全部决策固定 `safeToExecuteNow:false`、`requiresConfirmation:true`、
`foregroundRequired:false` 和 `activeWriteProbePerformed:false`。assessment 只说明固定
后台路径在确认后是否可以尝试，不构成写入授权；实际 mutation 仍须经过确认、精确 opaque
目标重新解析、前景门禁、deadline 与结果验证。公共结果不得包含 token、SID、完整性 RID、
PID、HWND 或路径。

Component 测试必须穷举三种元数据状态与四种完整性关系的全部十二个组合；Rust 公开
`sessions/status win32-control` 集成门禁还须验证逐目标字段、三类计数完整性和主动写探针
为零。

## 统一错误语义

目录缺口返回 `CAPABILITY_GAP`；能力已发布但没有认证后台实现时返回
`BACKGROUND_OPERATION_UNAVAILABLE`；opaque 目标在使用时消失或身份改变返回
`STALE_SESSION`。兼容 Win32 写在消息可能已经送达、但 deadline 内无法确认结果时返回
`TARGET_HUNG_OR_UNAVAILABLE`，并在 details 保留正式 `TIMEOUT`、
`outcome:"unknown"`、`retrySafe:false` 与目标可能已经变化的事实。调用方不得自动重试
unknown outcome。版本化清单位于 `tests/contracts/error-semantics-policy-v1.json`，所有
运行时错误码必须进入封闭 `error-envelope.schema.json`。该 schema 的 `code.enum` 是公开
错误码集合的协议权威；集成门禁扫描 `src` 中非测试生产 Rust 源码以及唯一生产 launcher
中的全大写下划线字符串，只对环境变量、构建变量、Serde 命名规则和 Win32 消息常量保留
逐值白名单，防止新增生产错误码绕过 schema。门禁不要求 schema 与当前源码完全相等，
以保留已发布兼容错误码。

`process.metadata.read@1` 只公开进程名、运行状态、关系、窗口可见性、
`metadataAccess` 和相对本工具进程的 `integrityRelation`。不得公开 PID、token、
SID、完整性 RID 或可执行路径。访问被 Windows 拒绝时 assessment 返回
`permission-blocked`/realm `none`，不得尝试提权或绕过。

`window.capture.preflight@1` 只读取精确窗口的 visible、minimized、DWM cloaked、
非零显示范围、桌面合成和 capture-affinity 分类。公共结果不得公开坐标、尺寸、
HWND/PID 或路径，不得读取像素、创建文件、激活窗口或发送输入。它可以调用 WGC
`IsSupported` 和 `CreateForWindow` 验证 runtime/item interop，但不得创建 frame
pool、启动 capture session 或压制系统隐私指示器。preflight 可执行
不自行表示 `window.screenshot@1` 可执行；该能力现已另行通过实际 WGC 像素等价、
确认、路径与原子输出门禁，assessment 返回 `confirmation-required`/
`isolated-worker`。旧原生目标仍只经迁移 launcher 使用 Rust。

`window.capture.frame.probe@1` 是 `window.screenshot@1` 之前的窄纵切。它要求逐操作
`--confirm`，未确认时必须在解析 target 前返回 `CONFIRMATION_REQUIRED`。确认后，
主 facade 先做只读 preflight，再把 opaque session 交给 Job 约束的
`act/capture-worker/v1` companion；worker 自行重新解析目标，只取得一个 WGC frame
的宽高和设备类型。该能力不调用 frame `Surface()`，不复制、返回或持久化像素，不
写文件，不激活窗口、不发送输入，也不压制系统隐私指示器。它证明受控帧会话路径，
它本身不返回图片；正式 `window.screenshot@1` 由独立 Screenshot Module 在确认后
执行。

所有 WGC 生产路径共同遵守 `../internal/capture-privacy-indicator-policy-v1.md`：
不得请求 borderless access 或把 border-required 属性设为 `false`。自动化门禁只
认证源码和公开 schema 没有压制系统行为；真实可见边框仍由独立人工视觉任务验收。

## 能力目录

能力 ID 使用 `<domain>.<noun>.<verb>@<major>`，例如：

- `application.discover@1`
- `process.discover@1`
- `process.metadata.read@1`
- `window.discover@1`
- `window.discover@2`
- `window.metadata.read@1`
- `window.metadata.read@2`
- `window.capture.preflight@1`
- `window.capture.frame.probe@1`
- `browser.screenshot@1`
- `accessibility.tree.read@1`
- `accessibility.tree.read@2`
- `ui.element.locate@1`
- `ui.element.action@1`
- `ui.element.wait@1`
- `window.screenshot@1`
- `window.record@1`
- `ui.text.input@1`
- `ui.input.key@1`
- `ui.input.pointer@1`
- `window.lifecycle@1`
- `file.document.create@1`
- `media.session.discover@1`
- `media.playback.state.read@1`
- `media.playback.control@1`
- `window.close@1`

每个 descriptor 至少声明：

- capability ID、版本、输入 schema 和结果 schema；
- 支持的目标种类与目标身份有效期；
- `read` / `mutation` / `destructive` 风险；
- `host-headless` / `same-session-no-focus` / `isolated-worker` /
  `host-foreground` 执行域；
- 是否要求确认、前台同意、隔离、权限或外部依赖；
- 原子性、幂等、超时、取消、部分成功、补偿和覆盖语义；
- 实际可用性与不可用原因。

`methods [method-id]` 是旧调用者的只读方法目录，保持原顶层 JSON、method ID、
`summary`、`availability` 与 `executionScope`。C++ 迁移状态通过
`capabilities method [method-id]` 独立报告 `cppStatus` 与 `safetyBoundary`，
不得把旧 Rust `native` 字段误读成 C++ 已认证执行。

`describe <app> [operation]` 从版本化 companion manifest 读取迁移基线。目录查询
保持旧顶层 JSON，不授权执行。`capabilities descriptor <app> [operation]` 对所有
尚未过等价门禁的 operation 报告 `cppExecutionEnabled:false`。manifest 缺失或
不合法时两条路径都 fail closed，不回退 Cargo。

## 受保护上下文与权限边界

所有精确目标读取和 mutation 在 provider 解析前都必须经过
`security-boundary.md` 定义的 Permission Assessment。只有活动的非零交互会话，且工具
进程桌面就是当前输入桌面时，System 才能继续解析目标。Session 0、非活动会话以及 UAC、
锁屏或其他受保护/非输入桌面返回 `PERMISSION_DENIED`；会话或桌面状态无法可靠探测时返回
`CAPABILITY_ASSESSMENT_UNAVAILABLE`。两种失败都必须证明尚未读取或写入目标，也没有尝试
提权、注入或备用路线。

主机姿态许可后，目标相对完整性仍由静态权限 Component 和领域 Module 独立失败闭合：
同级或较低目标只能在 capability 自身确认规则满足后继续；较高完整性或 metadata 权限
拒绝返回 `PERMISSION_DENIED`；未知关系和 metadata 不可用返回
`CAPABILITY_ASSESSMENT_UNAVAILABLE`。主机姿态许可不能覆盖目标门禁，确认、前台同意或
严格隔离也不能覆盖任一权限结论。

## 安全不变量

1. 目录没有能力时返回 `CAPABILITY_GAP`，不得换控制面。
2. 精确目标未发布能力时返回 `CAPABILITY_UNSUPPORTED`。
3. 能力已发布但无认证实现时返回 `BACKGROUND_OPERATION_UNAVAILABLE`。
4. 后台路径不得调用焦点、激活、键鼠、光标或剪贴板注入。
5. UIA 仅用于发现和读取；写操作统一经过受控 facade。没有应用 API 时，
   UIA 读取可辅助定位，但实际输入仍需前台同意和精确目标验证。
6. 后台失败不得静默降级为前台输入。
7. 严格隔离不得降级为同会话无焦点或主机前台。
8. 保存、覆盖、删除、凭据、提权和目标范围变化要求新的明确确认。
9. 任何权限不足都必须报告，不得尝试绕过 UIPI、UAC、沙箱或应用权限。
10. 操作前后记录可观察的前景、目标身份和结果证据；检测到违反声明的主机影响
    时拒绝结果。
11. Session 0、非活动会话、锁屏、UAC 安全桌面或不可确定安全姿态必须在目标访问前
    失败闭合；不得泄漏桌面名称、SID、token、handle、路径或安全描述符。

## 支持层级

| 层级 | 能力 | 对产品的承诺 |
| --- | --- | --- |
| L0 发现 | 进程、窗口、已安装/已运行应用 | 可列出并分配 opaque 精确目标 |
| L1 观察 | 元数据、只读 UIA 树、精确窗口截图/录像 | 不修改目标；报告可见性和捕获限制 |
| L2 认证后台写 | 应用 API、CLI、IPC、文件协议、媒体会话、认证标准消息 | 只对已发布的具体 capability 承诺 |
| L3 受控前台写 | 精确窗口键鼠输入 | 明确确认、前台所有权校验、可观察影响 |
| L4 隔离执行 | 认证 VM/独立交互会话 worker | 满足严格零打扰；缺失时结构化失败 |

支持等级按“目标种类 × capability × 执行域”统计，不按“支持多少个 App 名称”
统计。动态发现应用不等于自动授权启动或写入。
