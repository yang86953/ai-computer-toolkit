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
# 定位统一 launcher。
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 定位 capability assessment schema。
$schemaPath = Join-Path $projectRoot `
    'contracts\v1\capability-assessment.schema.json'

# 默认构建最新 Rust 主程序。
if (-not $SkipBuild) {
    # 编译全部 Rust 二进制。
    & cargo build --manifest-path (Join-Path $projectRoot 'Cargo.toml')
    # 构建失败立即终止。
    if ($LASTEXITCODE -ne 0) {
        # 返回稳定构建失败原因。
        throw "Rust build failed with exit code $LASTEXITCODE."
    }
}

# 主程序必须存在。
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    # 禁止静默跳过 Rust 门禁。
    throw 'The Rust main executable is not built.'
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
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
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
    $process = [Diagnostics.Process]::Start($startInfo)
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

# 执行一个成功的 capability assessment。
function Invoke-Assessment {
    # 声明函数参数。
    param(
        # 接收 capability ID。
        [Parameter(Mandatory)]
        [string] $Capability,
        # 接收 opaque session。
        [Parameter(Mandatory)]
        [string] $SessionId
    )
    # 调用 Rust assess 入口。
    $result = Invoke-JsonCommand -CommandArguments @(
        # 指定命令与 generic surface。
        'assess', 'app',
        # 指定版本化 capability。
        '--capability', $Capability,
        # 指定精确 opaque 目标。
        '--target', "sessionId=$SessionId"
    )
    # 核对公共 envelope 与禁止副作用证据。
    if (-not $result.ok -or
        # 核对控制契约版本。
        $result.contractVersion -ne 'act/control/v1' -or
        # 核对 capability 回显。
        $result.capability -ne $Capability -or
        # 核对 opaque 目标回显。
        $result.targetId -ne $SessionId -or
        # assessment 永不授权前景激活。
        $result.evidence.foregroundActivationAllowed -or
        # assessment 永不授权输入。
        $result.evidence.inputAllowed -or
        # 所有决策禁止未认证回退。
        -not $result.constraints.noFallback) {
        # 拒绝任何不完整或放宽的结果。
        throw 'Rust assessment envelope is incomplete or unsafe.'
    }
    # 序列化进行原生字段泄漏扫描。
    $serialized = $result | ConvertTo-Json -Depth 20 -Compress
    # 禁止原生身份与路径进入公共结果。
    foreach ($forbidden in @('hwnd', 'processId', 'native', 'providerId')) {
        # 任一字段泄漏都使门禁失败。
        if ($serialized -cmatch ('"' + [regex]::Escape($forbidden) + '"')) {
            # 返回可定位字段名。
            throw "Assessment leaked forbidden field: $forbidden"
        }
    }
    # 返回已验证结果。
    return $result
}

# 枚举当前 app sessions 取得 host 与文本创建器。
$appSessions = Invoke-JsonCommand -CommandArguments @(
    # 使用有界 app session 发现。
    'sessions', 'app', '--max-items', '256'
)
# 选择唯一当前 host session。
$hostTarget = @(
    # 过滤 host 类别。
    $appSessions.sessions | Where-Object { $_.kind -eq 'host' }
)[0].sessionId
# 选择发布文本创建 capability 的应用 session。
$textTarget = @(
    # 过滤精确 capability descriptor。
    $appSessions.sessions | Where-Object {
        # 要求应用类别且发布文本创建。
        $_.kind -eq 'application' -and
            $_.capabilities.id -contains 'text.document.create@1'
    }
)[0].sessionId

# 从动态应用目录选择单独认证的精确启动目标。
$applicationInventory = Invoke-JsonCommand -CommandArguments @(
    # 使用完整应用边界避免测试截断。
    'discover', 'app', '--max-applications', '4096', '--max-processes', '4096', '--max-windows', '4096'
)
# 选择首个带确认型 Shell 启动能力的 s2:a。
$applicationTarget = @(
    # 只读取 Rust 当前公开的认证状态。
    $applicationInventory.data.applications | Where-Object {
        # 要求单独认证状态。
        $_.launchCapability -eq 'available-confirmed'
    }
)[0].sessionId

# 枚举当前窗口取得真实 s2:w。
$windowSessions = Invoke-JsonCommand -CommandArguments @(
    # 使用窗口只读 surface。
    'sessions', 'window', '--max-items', '4096'
)
# 环境必须至少有一个真实可见窗口。
if (@($windowSessions.sessions).Count -eq 0) {
    # 无窗口时无法完成 Stage 3E 真实门禁。
    throw 'No visible titled window exists for Rust assessment verification.'
}
# 选择首个当前窗口。
$windowTarget = @($windowSessions.sessions)[0].sessionId

# 枚举当前进程取得可读 s2:p。
$processSessions = Invoke-JsonCommand -CommandArguments @(
    # 使用完整硬上限，避免测试截断。
    'sessions', 'process', '--max-items', '4096'
)
# 选择元数据可读的当前进程。
$processTarget = @(
    # 过滤可用进程元数据。
    $processSessions.sessions | Where-Object {
        # 只接受 available。
        $_.metadataAccess -eq 'available'
    }
)[0].sessionId

# 验证 host 只读发现可后台执行。
$hostRead = Invoke-Assessment `
    -Capability 'application.discover@1' `
    -SessionId $hostTarget
# 验证精确动态应用启动仍需逐操作确认。
$applicationLaunch = Invoke-Assessment `
    -Capability 'application.open@1' `
    -SessionId $applicationTarget
# 验证进程元数据只读路径。
$processRead = Invoke-Assessment `
    -Capability 'process.metadata.read@1' `
    -SessionId $processTarget
# 验证窗口元数据只读路径。
$windowRead = Invoke-Assessment `
    -Capability 'window.metadata.read@1' `
    -SessionId $windowTarget
# 验证隔离树读取的认证 realm。
$treeRead = Invoke-Assessment `
    -Capability 'accessibility.tree.read@1' `
    -SessionId $windowTarget
# 验证前台键盘能力只报告 consent 门禁。
$foreground = Invoke-Assessment `
    -Capability 'ui.input.key@1' `
    -SessionId $windowTarget
# 验证待迁移窗口能力保持 unavailable。
$pending = Invoke-Assessment `
    -Capability 'window.capture.preflight@1' `
    -SessionId $windowTarget
# 验证精确目标不发布进程能力时 unsupported。
$unsupported = Invoke-Assessment `
    -Capability 'process.metadata.read@1' `
    -SessionId $windowTarget
# 验证未知公共能力返回 capability-gap。
$gap = Invoke-Assessment `
    -Capability 'unknown.read@1' `
    -SessionId $windowTarget
# 验证文本创建器仍需确认且使用 host-background。
$textCreate = Invoke-Assessment `
    -Capability 'text.document.create@1' `
    -SessionId $textTarget

# 核对所有关键决策与 realm。
if ($hostRead.decision -ne 'executable-background' -or
    # 应用启动必须确认。
    $applicationLaunch.decision -ne 'confirmation-required' -or
    # 进程读取必须后台可执行。
    $processRead.decision -ne 'executable-background' -or
    # 窗口读取必须后台可执行。
    $windowRead.decision -ne 'executable-background' -or
    # 树读取必须使用隔离 worker realm。
    $treeRead.executionRealm -ne 'isolated-worker' -or
    # 前台输入必须单独取得同意。
    $foreground.decision -ne 'foreground-consent-required' -or
    # 未迁移能力不得启用。
    $pending.decision -ne 'unavailable' -or
    # 错误目标类别必须不支持。
    $unsupported.decision -ne 'unsupported' -or
    # 未知能力必须保持目录缺口。
    $gap.decision -ne 'capability-gap' -or
    # 文本创建必须逐操作确认。
    $textCreate.decision -ne 'confirmation-required' -or
    # 文本创建使用稳定 host-background realm。
    $textCreate.executionRealm -ne 'host-background') {
    # 任一语义漂移都使门禁失败。
    throw 'Rust assessment decision matrix drifted.'
}

# 验证 stale 目标使用非零退出码和结构化错误。
$stale = Invoke-JsonCommand -ExpectedExitCode 2 -CommandArguments @(
    # 调用窗口元数据 assessment。
    'assess', 'app',
    # 指定合法 capability。
    '--capability', 'window.metadata.read@1',
    # 提供 canonical 但不存在的 opaque 目标。
    '--target', 'sessionId=s2:w:0000000000000000'
)
# stale 不能被误报成 assessment 成功。
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    # 返回稳定回归原因。
    throw 'Rust assessment stale semantics drifted.'
}

# 通过统一 launcher 调用同一 host assessment。
$launcherText = & powershell.exe -NoProfile -ExecutionPolicy Bypass `
    -File $launcher assess app `
    --capability application.discover@1 `
    --target "sessionId=$hostTarget"
# launcher 必须成功。
if ($LASTEXITCODE -ne 0) {
    # 禁止回退或吞掉 Rust 失败。
    throw 'Launcher assessment failed.'
}
# 解析 launcher 的唯一 JSON 输出。
$launcherRead = $launcherText | ConvertFrom-Json
# launcher 与 Rust 直接结果必须逐字段一致。
if (($launcherRead | ConvertTo-Json -Depth 20 -Compress) -ne
    # 比较 direct 结果。
    ($hostRead | ConvertTo-Json -Depth 20 -Compress)) {
    # 禁止 launcher 二次实现 assessment。
    throw 'Launcher assessment differs from direct Rust output.'
}

# 读取 schema 并验证八种 decision 与全部 runtime realm。
$schema = [IO.File]::ReadAllText($schemaPath) | ConvertFrom-Json
# 读取 decision 封闭枚举。
$decisions = @($schema.properties.decision.enum)
# 读取 execution realm 封闭枚举。
$realms = @($schema.properties.executionRealm.enum)
# schema 必须恰好声明八种 decision。
if ($decisions.Count -ne 8 -or
    # 必须包含 Stage 3E 使用的 host-background。
    $realms -notcontains 'host-background' -or
    # 必须包含隔离 worker realm。
    $realms -notcontains 'isolated-worker' -or
    # 必须包含禁止执行的 none。
    $realms -notcontains 'none') {
    # schema 缺口使门禁失败。
    throw 'Capability assessment schema is incomplete.'
}

# 输出自动化可消费的门禁摘要。
[ordered]@{
    # 标记门禁成功。
    ok = $true
    # 输出 host 只读决策。
    hostRead = $hostRead.decision
    # 输出精确动态应用写确认决策。
    applicationLaunch = $applicationLaunch.decision
    # 输出进程只读决策。
    processRead = $processRead.decision
    # 输出窗口只读决策。
    windowRead = $windowRead.decision
    # 输出可访问性 realm。
    treeRealm = $treeRead.executionRealm
    # 输出前台同意决策。
    foreground = $foreground.decision
    # 输出待迁移决策。
    pending = $pending.decision
    # 输出错误目标类别决策。
    unsupported = $unsupported.decision
    # 输出目录缺口决策。
    gap = $gap.decision
    # 输出 stale 错误。
    stale = $stale.error.code
    # 标记 launcher 等价。
    launcherMatches = $true
    # 输出 schema decision 数量。
    schemaDecisions = $decisions.Count
# 压缩为单行 JSON。
} | ConvertTo-Json -Compress
