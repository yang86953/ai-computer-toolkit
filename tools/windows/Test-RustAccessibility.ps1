# 声明脚本参数。
[CmdletBinding()]
# 接收可跳过既有构建的开关。
param([switch] $SkipBuild)

# 任何 PowerShell 错误立即终止门禁。
$ErrorActionPreference = 'Stop'
# 定位仓库根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位 Rust 主程序。
$executable = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
# 定位 Rust companion worker。
$worker = Join-Path $projectRoot `
    'target\debug\ai-computer-toolkit-observation-worker.exe'
# 读取等价与安全策略。
$policy = Get-Content -LiteralPath (
    # 拼接策略路径。
    Join-Path $projectRoot 'tests\contracts\uia-tree-equivalence-policy.json'
# 解析策略 JSON。
) -Raw | ConvertFrom-Json

# 默认执行完整 Rust 构建与生命周期测试。
if (-not $SkipBuild) {
    # 构建主程序与 companion worker。
    & cargo build --manifest-path (Join-Path $projectRoot 'Cargo.toml')
    # 构建失败立即终止。
    if ($LASTEXITCODE -ne 0) {
        # 返回稳定失败原因。
        throw "Rust build failed with exit code $LASTEXITCODE."
    }
    # 运行真实 Job timeout/cancellation 生命周期测试。
    & cargo test --manifest-path (Join-Path $projectRoot 'Cargo.toml') `
        components::worker_process
    # 生命周期测试失败立即终止。
    if ($LASTEXITCODE -ne 0) {
        # 返回稳定失败原因。
        throw "Rust worker lifecycle tests failed with exit code $LASTEXITCODE."
    }
}

# 主程序必须存在。
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    # 禁止静默跳过 Rust 主程序。
    throw 'The Rust main executable is not built.'
}
# companion 必须与主程序同目录存在。
if (-not (Test-Path -LiteralPath $worker -PathType Leaf)) {
    # 禁止进程内降级。
    throw 'The Rust observation companion is not built.'
}

# 通过重定向执行一个纯 JSON 命令。
function Invoke-JsonCommand {
    # 声明函数参数。
    param(
        # 接收命令参数数组。
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        # 接收预期退出码。
        [int] $ExpectedExitCode = 0
    )
    # 拒绝当前门禁不需要的复杂引用参数。
    foreach ($argument in $CommandArguments) {
        # 空白或引号需要不同 argv 构造器，因此 fail closed。
        if ($argument -match '[\s"]') {
            # 返回具体参数以便维护测试。
            throw "Test argument requires unsupported quoting: $argument"
        }
    }
    # 创建无 shell 进程启动信息。
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    # 指定 Rust 主程序。
    $startInfo.FileName = $executable
    # 使用空格连接本门禁的简单参数。
    $startInfo.Arguments = $CommandArguments -join ' '
    # 禁止 shell execute。
    $startInfo.UseShellExecute = $false
    # 重定向 stdout。
    $startInfo.RedirectStandardOutput = $true
    # 重定向 stderr。
    $startInfo.RedirectStandardError = $true
    # 启动命令。
    $process = [System.Diagnostics.Process]::Start($startInfo)
    # 完整读取 stdout。
    $stdout = $process.StandardOutput.ReadToEnd()
    # 完整读取 stderr。
    $stderr = $process.StandardError.ReadToEnd()
    # 等待命令退出。
    $process.WaitForExit()
    # 核对退出码。
    if ($process.ExitCode -ne $ExpectedExitCode) {
        # 输出结构化命令文本帮助定位。
        throw "Unexpected exit $($process.ExitCode): $stdout $stderr"
    }
    # JSON CLI 不允许 stderr。
    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        # 返回 stderr 诊断。
        throw "Command wrote unexpected stderr: $stderr"
    }
    # 解析单个 JSON value。
    try {
        # 返回解析结果。
        return $stdout | ConvertFrom-Json
    } catch {
        # 拒绝非 JSON 输出。
        throw "Command did not return one JSON value: $stdout"
    }
}

# 枚举 canonical Rust 窗口清单。
$sessions = Invoke-JsonCommand -CommandArguments @(
    # 使用窗口 surface。
    'sessions', 'window', '--max-items', '4096'
)
# 要求至少一个真实可见窗口。
if (@($sessions.sessions).Count -eq 0) {
    # 当前真实门禁无法无窗口执行。
    throw 'No visible titled window exists for Rust accessibility verification.'
}
# 优先选择 Firefox 以获得非平凡树，缺失时选择首个窗口。
$selected = @(
    # 筛选 Firefox。
    $sessions.sessions | Where-Object {
        # 按 Windows 应用名不区分大小写比较。
        $_.applicationName -ieq 'firefox.exe'
    # 只取一个。
    } | Select-Object -First 1
)
# Firefox 缺失时使用任一可见窗口。
if ($selected.Count -eq 0) {
    # 选择当前快照首项。
    $selected = @($sessions.sessions | Select-Object -First 1)
}
# 保存 canonical 目标。
$sessionId = $selected[0].sessionId

# 读取 ControlView 有界树。
$control = Invoke-JsonCommand -CommandArguments @(
    # 使用 uia 兼容别名。
    'inspect-tree', 'uia', '--target', "sessionId=$sessionId",
    # 使用策略边界。
    '--max-depth', [string] $policy.maximumDepth,
    # 使用策略数量。
    '--max-items', [string] $policy.maximumItems,
    # 使用 control view。
    '--view', 'control',
    # 使用策略 deadline。
    '--timeout-ms', [string] $policy.defaultTimeoutMs
)
# 核对 ControlView 顶层契约。
if (
    # capability 必须稳定。
    $control.capability -ne 'accessibility.tree.read@1' -or
    # scope 必须有界。
    $control.scope -ne 'bounded-tree' -or
    # view 必须保留。
    $control.view -ne 'control' -or
    # 必须只读。
    -not $control.readOnly -or
    # 前景必须不变。
    -not $control.foregroundUnchanged -or
    # 必须使用 Job 隔离。
    $control.safety.providerTimeoutIsolation -ne 'job-bounded-worker' -or
    # 必须可取消。
    -not $control.safety.workerCancellable -or
    # 禁止读取 Value 内容。
    $control.safety.valueContentRead -or
    # 禁止读取 Text 内容。
    $control.safety.textContentRead -or
    # 禁止查询写 pattern。
    $control.safety.writePatternsQueried -or
    # 禁止公开 bounds。
    $control.safety.boundsExposed
) {
    # 任何不变量失败即终止。
    throw 'Rust ControlView tree violated its safety contract.'
}
# 核对节点数量与 visited 一致。
if (
    # 节点不得超过策略上限。
    @($control.nodes).Count -gt $policy.maximumItems -or
    # visited 必须与节点数一致。
    $control.visited -ne @($control.nodes).Count -or
    # 节点不得超过深度。
    @($control.nodes | Where-Object {
        # 筛选越界节点。
        $_.depth -gt $policy.maximumDepth
    }).Count -ne 0
) {
    # 有界性失败即终止。
    throw 'Rust ControlView tree exceeded its requested bounds.'
}
# 核对每个 node ID 与 freshness。
$invalidNodes = @(
    # 扫描全部节点。
    $control.nodes | Where-Object {
        # node ID 必须是 canonical s2:e。
        $_.nodeId -notmatch '^s2:e:[0-9a-f]{16}$' -or
        # 生命周期必须限定到当前检查快照。
        $_.identityFreshness -ne 'inspection-snapshot'
    }
)
# 任一无效节点终止门禁。
if ($invalidNodes.Count -ne 0) {
    # 返回稳定失败原因。
    throw 'Rust tree returned a noncanonical or non-snapshot node identity.'
}

# 读取 RawView 有界树。
$raw = Invoke-JsonCommand -CommandArguments @(
    # 使用 accessibility 语义别名。
    'inspect-tree', 'accessibility', '--target', "sessionId=$sessionId",
    # 使用深度 1。
    '--max-depth', '1',
    # 使用数量 30。
    '--max-items', '30',
    # 使用 raw view。
    '--view', 'raw',
    # 使用策略 deadline。
    '--timeout-ms', [string] $policy.defaultTimeoutMs
)
# 核对 RawView 有界且前景不变。
if (
    # view 必须保留。
    $raw.view -ne 'raw' -or
    # 节点不得超过数量边界。
    @($raw.nodes).Count -gt 30 -or
    # 节点不得超过深度边界。
    @($raw.nodes | Where-Object { $_.depth -gt 1 }).Count -ne 0 -or
    # 前景必须不变。
    -not $raw.foregroundUnchanged
) {
    # RawView 不变量失败。
    throw 'Rust RawView tree violated its bounds or foreground invariant.'
}

# 验证 depth 0 只返回 root。
$rootTree = Invoke-JsonCommand -CommandArguments @(
    # 使用最小有界树。
    'inspect-tree', 'uia', '--target', "sessionId=$sessionId",
    # depth 0。
    '--max-depth', '0',
    # 只允许一个节点。
    '--max-items', '1',
    # 使用 control view。
    '--view', 'control'
)
# 核对 depth 0 结果。
if (
    # 必须恰好一个 root。
    @($rootTree.nodes).Count -ne 1 -or
    # root 深度必须为 0。
    $rootTree.nodes[0].depth -ne 0
) {
    # depth 0 门禁失败。
    throw 'Rust depth-zero tree did not return exactly one root.'
}

# 验证 root-only inspect 也使用同一 worker。
$root = Invoke-JsonCommand -CommandArguments @(
    # 使用 uia 兼容入口。
    'inspect', 'uia', '--target', "sessionId=$sessionId",
    # 使用策略 deadline。
    '--timeout-ms', [string] $policy.defaultTimeoutMs
)
# 核对 root-only 结果。
if (
    # scope 必须为 root-only。
    $root.accessibility.scope -ne 'root-only' -or
    # 必须只读。
    -not $root.readOnly -or
    # 前景必须不变。
    -not $root.foregroundUnchanged -or
    # session 必须保持相同 canonical ID。
    $root.session.sessionId -ne $sessionId
) {
    # root 兼容入口失败。
    throw 'Rust root-only accessibility inspect failed.'
}

# 验证 stale 目标 fail closed。
$stale = Invoke-JsonCommand -CommandArguments @(
    # 使用不存在的 canonical 目标。
    'inspect-tree', 'uia', '--target',
    # 指定固定 stale ID。
    'sessionId=s2:w:0000000000000000'
# 预期通用失败退出码。
) -ExpectedExitCode 2
# 核对 stale 错误分类。
if ($stale.error.code -ne 'STALE_SESSION') {
    # 禁止接受 stale 目标。
    throw 'Rust accessibility accepted a stale target.'
}

# 验证 1ms deadline 强制走 timeout 回收路径。
$timeout = Invoke-JsonCommand -CommandArguments @(
    # 请求最大 RawView 树。
    'inspect-tree', 'uia', '--target', "sessionId=$sessionId",
    # 使用最大深度。
    '--max-depth', '20',
    # 使用最大数量。
    '--max-items', '4096',
    # 使用 raw view。
    '--view', 'raw',
    # 使用最小 deadline。
    '--timeout-ms', '1'
# 预期 timeout 失败退出码。
) -ExpectedExitCode 2
# 核对 timeout 分类。
if ($timeout.error.code -ne $policy.timeoutError) {
    # 禁止模糊 provider 失败。
    throw 'Rust accessibility did not preserve timeout semantics.'
}

# 把 ControlView 序列化后扫描禁止字段。
$serialized = $control | ConvertTo-Json -Depth 40 -Compress
# 逐项扫描公开隐私禁区。
foreach ($field in $policy.forbiddenPublicFields) {
    # 精确匹配 JSON property 名。
    if ($serialized -match ('"' + [Regex]::Escape($field) + '":')) {
        # 返回泄漏字段。
        throw "Rust accessibility leaked forbidden public field $field."
    }
}

# 读取 Rust worker 源码。
$workerSource = Get-Content -LiteralPath (
    # 拼接 worker 源码路径。
    Join-Path $projectRoot 'src\observation_worker.rs'
# 读取完整文本。
) -Raw
# 扫描禁止 UIA/write API。
foreach ($api in $policy.forbiddenRustWorkerApis) {
    # 任何禁止符号都使静态门禁失败。
    if ($workerSource.Contains($api)) {
        # 返回命中的 API。
        throw "Rust observation worker contains forbidden API $api."
    }
}

# 验证新增 schema 是有效 JSON。
$null = Get-Content -LiteralPath (
    # 拼接 schema 路径。
    Join-Path $projectRoot 'contracts\v1\accessibility-tree.schema.json'
# 解析 schema。
) -Raw | ConvertFrom-Json

# 给内核退出通知短暂传播时间。
Start-Sleep -Milliseconds 100
# 查询残留观察 worker。
$remainingWorkers = @(
    # 按固定 companion 进程名查询。
    Get-Process -Name 'ai-computer-toolkit-observation-worker' `
        -ErrorAction SilentlyContinue
)
# 任何 worker 残留都失败。
if ($remainingWorkers.Count -ne 0) {
    # 返回零残留门禁失败。
    throw 'A Rust observation worker remained after the test.'
}

# 输出紧凑可存档证据。
[PSCustomObject]@{
    # 标记门禁成功。
    ok = $true
    # 输出被测应用名。
    application = $selected[0].applicationName
    # 输出 ControlView 节点数。
    controlNodes = @($control.nodes).Count
    # 输出 RawView 节点数。
    rawNodes = @($raw.nodes).Count
    # 标记 depth 0 通过。
    depthZero = $true
    # 标记 root-only 通过。
    rootOnly = $true
    # 标记隐私泄漏为零。
    forbiddenFieldLeaks = 0
    # 标记前景不变。
    foregroundUnchanged = $true
    # 输出 stale 错误。
    staleError = $stale.error.code
    # 输出 timeout 错误。
    timeoutError = $timeout.error.code
    # 标记取消生命周期由 Rust 单元测试覆盖。
    cancellationLifecycle = 'cargo-test-job-terminated'
    # 输出残留 worker 数量。
    remainingWorkers = $remainingWorkers.Count
# 转为深度足够的 JSON。
} | ConvertTo-Json -Depth 10 -Compress
