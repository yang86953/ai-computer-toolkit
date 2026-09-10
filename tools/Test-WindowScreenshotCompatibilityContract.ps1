[CmdletBinding()]
param()

# 失败时立即终止截图门禁。
$ErrorActionPreference = 'Stop'
# 定位仓库根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位 Rust 主程序。
$executable = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
# 定位统一生产 launcher。
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 定位 Rust-only 截图策略文件。
$policyPath = Join-Path $projectRoot 'tests\contracts\window-screenshot-compatibility-policy.json'
# 读取 Rust-only 截图策略。
$policy = Get-Content -LiteralPath $policyPath -Raw | ConvertFrom-Json
# 定位截图公开 schema。
$schemaPath = Join-Path $projectRoot 'contracts\v1\window-screenshot.schema.json'
# 验证公开 schema 可解析。
$null = Get-Content -LiteralPath $schemaPath -Raw | ConvertFrom-Json

# 优先使用 PATH 中的 cargo。
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
# 命中 PATH 时保存绝对路径。
if ($null -ne $cargo) {
    # 将 CommandInfo 收窄为字符串。
    $cargo = $cargo.Source
}
# PATH 缺失时使用用户默认安装位置。
if ($null -eq $cargo) {
    # 构造用户级 cargo proxy 路径。
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}
# cargo 缺失时禁止使用旧二进制或 C++ fallback。
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    # 返回明确 Rust 工具链缺口。
    throw 'cargo is unavailable for the Rust window screenshot gate.'
}
# 构建 Rust 主程序、capture worker 与自有 fixture。
$manifestPath = Join-Path $projectRoot 'Cargo.toml'
# 使用仓库权威 manifest 构建全部二进制。
& $cargo build --quiet --bins --manifest-path $manifestPath
# 构建失败立即终止。
if ($LASTEXITCODE -ne 0) {
    # 返回稳定 Rust 构建错误。
    throw "Rust build failed with exit code $LASTEXITCODE."
}

# 执行 Rust 主程序并解析单行 JSON。
function Invoke-RustJson {
    param(
        # 接收固定 CLI 参数。
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        # 接收期望退出码。
        [int] $ExpectedExitCode = 0
    )
    # 创建无 shell 进程配置。
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    # 固定 Rust 主程序。
    $startInfo.FileName = $executable
    # 禁止 shell 改写参数。
    $startInfo.UseShellExecute = $false
    # 捕获唯一 stdout envelope。
    $startInfo.RedirectStandardOutput = $true
    # 捕获并要求 stderr 为空。
    $startInfo.RedirectStandardError = $true
    # 测试路径为无空格 ASCII build 路径，保持既有 CLI 引用规则。
    $startInfo.Arguments = $CommandArguments -join ' '
    # 启动进程。
    $process = [System.Diagnostics.Process]::Start($startInfo)
    # 读取完整 stdout。
    $stdout = $process.StandardOutput.ReadToEnd()
    # 读取完整 stderr。
    $stderr = $process.StandardError.ReadToEnd()
    # 等待进程退出。
    $process.WaitForExit()
    # 核对退出码与 stderr。
    if ($process.ExitCode -ne $ExpectedExitCode -or
        -not [string]::IsNullOrWhiteSpace($stderr)) {
        # 报告安全诊断。
        throw "Unexpected Rust screenshot command: exit=$($process.ExitCode) stderr=$stderr"
    }
    # 解析唯一 JSON envelope。
    return $stdout | ConvertFrom-Json
}

# 通过生产 launcher 执行 Rust 截图路线。
function Invoke-LauncherJson {
    param(
        # 接收固定 launcher 参数。
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        # 接收期望退出码。
        [int] $ExpectedExitCode = 0
    )
    # 调用统一 launcher 并捕获 stdout。
    $stdout = & $launcher @CommandArguments
    # 冻结 launcher 退出码。
    $exitCode = $LASTEXITCODE
    # 核对 Rust 路由退出码。
    if ($exitCode -ne $ExpectedExitCode) {
        # 不回显潜在路径或目标内容。
        throw "Unexpected launcher screenshot exit code: $exitCode"
    }
    # 解析唯一 JSON envelope。
    return $stdout | ConvertFrom-Json
}

# 为门禁创建仓库 build 下的唯一目录。
$gateRoot = Join-Path $projectRoot 'build\rust-window-screenshot-gate'
# 使用 GUID 隔离本次产物。
$outputRoot = Join-Path $gateRoot ([Guid]::NewGuid().ToString('N'))
# 创建精确测试目录。
$null = New-Item -ItemType Directory -Path $outputRoot
# 定义 stale 路径。
$stalePng = Join-Path $outputRoot 'stale.png'
# 定义 desktop 正式输出。
$desktopPng = Join-Path $outputRoot 'desktop.png'
# 定义 app facade 正式输出。
$appPng = Join-Path $outputRoot 'app.png'
# 定义 app facade 请求文件。
$appInput = Join-Path $outputRoot 'app-request.json'

# 保存自有 fixture 与截图结果。
$fixtureProcess = $null
# 保存最终 desktop 结果。
$desktopResult = $null
# 保存最终 app 结果。
$appResult = $null
# 保存运行前 worker 数量。
$workersBefore = @(
    # 只读取固定 capture worker 进程名。
    Get-Process -Name 'ai-computer-toolkit-capture-worker' -ErrorAction SilentlyContinue
).Count

# 无论成功失败都回收自有进程和 build 夹具。
try {
    # 未确认必须先于非法目标与路径解析拒绝。
    $unconfirmed = Invoke-RustJson -CommandArguments @(
        # 调用 legacy desktop surface。
        'run', 'desktop', 'screenshot',
        # 提供故意非法目标。
        '--target', 'sessionId=not-valid',
        # 提供合法但不得触碰的路径。
        '--arg', "path=$stalePng"
    ) -ExpectedExitCode 2
    # 核对 confirmation-first。
    if ($unconfirmed.ok -or
        $unconfirmed.error.code -ne 'CONFIRMATION_REQUIRED' -or
        (Test-Path -LiteralPath $stalePng)) {
        # 拒绝确认顺序漂移。
        throw 'Rust window screenshot did not enforce confirmation-first.'
    }

    # 确认后的 stale 目标必须结构化失败且零文件。
    $stale = Invoke-LauncherJson -CommandArguments @(
        # 调用生产 desktop screenshot 路线。
        'run', 'desktop', 'screenshot',
        # 提供 canonical stale 目标。
        '--target', 'sessionId=s2:w:0000000000000000',
        # 提供隔离测试路径。
        '--arg', "path=$stalePng",
        # 提供基础确认。
        '--confirm'
    ) -ExpectedExitCode 2
    # 核对 Rust stale 分类和零文件。
    if ($stale.ok -or
        $stale.error.code -ne 'STALE_SESSION' -or
        (Test-Path -LiteralPath $stalePng)) {
        # 禁止 C++ fallback 或伪成功。
        throw 'Rust window screenshot did not preserve stale-target semantics.'
    }

    # 查询 Rust capability catalog。
    $catalog = Invoke-RustJson -CommandArguments @('capabilities', 'app')
    # 唯一选择截图 descriptor。
    $descriptor = @(
        # 只匹配稳定 capability ID。
        $catalog.data.capabilities | Where-Object { $_.id -eq $policy.capability }
    )
    # 核对 Rust catalog 声明。
    if ($descriptor.Count -ne 1 -or
        $descriptor[0].status -ne $policy.catalogStatus -or
        $descriptor[0].risk -ne 'read-sensitive' -or
        $descriptor[0].executionDomain -ne 'isolated-worker' -or
        -not $descriptor[0].requiresConfirmation) {
        # 拒绝目录状态漂移。
        throw 'Rust window screenshot catalog descriptor is invalid.'
    }

    # 定位 Rust 自有 no-activate fixture。
    $fixtureExecutable = Join-Path $projectRoot `
        'target\debug\ai-computer-toolkit-capture-fixture.exe'
    # 禁止使用真实用户窗口替代缺失 fixture。
    if (-not (Test-Path -LiteralPath $fixtureExecutable -PathType Leaf)) {
        # 报告自有 fixture 缺失。
        throw 'The Rust capture fixture executable is missing.'
    }
    # 构造固定前缀加 GUID 的唯一 ASCII 标题。
    $fixtureTitle = 'act-rust-capture-fixture-' + [Guid]::NewGuid().ToString('N')
    # 创建无 shell fixture 进程配置。
    $fixtureStart = [System.Diagnostics.ProcessStartInfo]::new()
    # 固定 Rust fixture 二进制。
    $fixtureStart.FileName = $fixtureExecutable
    # 禁止 shell 参数改写。
    $fixtureStart.UseShellExecute = $false
    # 隐藏 console，fixture 自己显示 no-activate 窗口。
    $fixtureStart.CreateNoWindow = $true
    # 传递经过 fixture 前缀校验的标题。
    $fixtureStart.Arguments = $fixtureTitle
    # 启动工具自有 fixture。
    $fixtureProcess = [System.Diagnostics.Process]::Start($fixtureStart)

    # 初始化自有 opaque 目标。
    $fixtureSessionId = $null
    # 有界等待 fixture 进入 Rust 窗口快照。
    foreach ($attempt in 1..50) {
        # 枚举完整有界窗口清单。
        $snapshot = Invoke-RustJson -CommandArguments @(
            # 使用 Rust window surface。
            'sessions', 'window', '--max-items', '4096'
        )
        # 按本次唯一标题选择自有窗口。
        $fixtureWindow = @(
            # 不允许匹配其他用户窗口。
            $snapshot.sessions | Where-Object { $_.title -eq $fixtureTitle }
        )
        # 唯一命中后保存 opaque ID。
        if ($fixtureWindow.Count -eq 1) {
            # 冻结 canonical sessionId。
            $fixtureSessionId = [string] $fixtureWindow[0].sessionId
            # 结束发现轮询。
            break
        }
        # fixture 提前退出表示创建失败。
        if ($fixtureProcess.HasExited) {
            # 禁止扩大到任意真实窗口。
            throw 'The Rust capture fixture exited before discovery.'
        }
        # 短暂轮询等待。
        Start-Sleep -Milliseconds 100
    }
    # 未唯一发现时失败闭合。
    if ([string]::IsNullOrWhiteSpace($fixtureSessionId)) {
        # 禁止使用用户窗口替代。
        throw 'The Rust capture fixture was not uniquely discovered.'
    }

    # 通过生产 launcher 执行 desktop 正式截图。
    $desktopResult = Invoke-LauncherJson -CommandArguments @(
        # 调用 desktop screenshot。
        'run', 'desktop', 'screenshot',
        # 只传递工具自有 opaque 目标。
        '--target', "sessionId=$fixtureSessionId",
        # 只写 build 夹具目录。
        '--arg', "path=$desktopPng",
        # 使用正式默认范围内 deadline。
        '--arg', 'timeoutMs=10000',
        # 提供逐操作确认。
        '--confirm'
    )
    # 核对正式 Rust 公开结果。
    if ($desktopResult.capability -ne $policy.capability -or
        $desktopResult.targetId -ne $fixtureSessionId -or
        $desktopResult.executionDomain -ne 'isolated-worker' -or
        $desktopResult.executionRealm -ne 'isolated-worker' -or
        -not $desktopResult.executionRealmCertified -or
        -not $desktopResult.atomicOutput -or
        -not $desktopResult.foregroundUnchanged -or
        $desktopResult.cursorCaptured -or
        -not $desktopResult.systemCaptureIndicatorMayAppear) {
        # 拒绝 schema 或安全证据漂移。
        throw ('The Rust desktop screenshot result is invalid: ' +
            ($desktopResult | ConvertTo-Json -Depth 20 -Compress))
    }
    # 核对 PNG 文件存在且字节数一致。
    if (-not (Test-Path -LiteralPath $desktopPng -PathType Leaf) -or
        (Get-Item -LiteralPath $desktopPng).Length -ne $desktopResult.bytes) {
        # 报告原子输出缺失。
        throw 'The Rust desktop screenshot output is missing or inconsistent.'
    }

    # 未确认覆盖必须保留已有 PNG。
    $desktopBefore = [IO.File]::ReadAllBytes($desktopPng)
    # 再次调用相同最终路径但不提供 overwrite。
    $overwriteDenied = Invoke-LauncherJson -CommandArguments @(
        # 调用正式 screenshot 路线。
        'run', 'desktop', 'screenshot',
        # 使用同一自有目标。
        '--target', "sessionId=$fixtureSessionId",
        # 指向已有测试 PNG。
        '--arg', "path=$desktopPng",
        # 提供基础确认但不提供覆盖许可。
        '--confirm'
    ) -ExpectedExitCode 2
    # 核对独立覆盖确认错误与原文件不变。
    if ($overwriteDenied.ok -or
        $overwriteDenied.error.code -ne 'OVERWRITE_CONFIRMATION_REQUIRED' -or
        -not [Linq.Enumerable]::SequenceEqual(
            [byte[]] $desktopBefore,
            [byte[]] [IO.File]::ReadAllBytes($desktopPng)
        )) {
        # 拒绝未确认覆盖。
        throw 'The Rust screenshot route changed an existing file without overwrite consent.'
    }

    # 构造 provider-neutral app facade 请求。
    $appRequest = [ordered]@{
        # 只传递 opaque 目标。
        target = [ordered]@{ sessionId = $fixtureSessionId }
        # 传递稳定 capability 与封闭 input。
        args = [ordered]@{
            # 指定 window screenshot capability。
            capability = $policy.capability
            # 只提供固定截图字段。
            input = [ordered]@{
                # 写入第二个 build fixture PNG。
                path = $appPng
                # 明确不覆盖。
                overwrite = $false
                # 使用有界 deadline。
                timeoutMs = 10000
            }
        }
        # 文件内不扩大 CLI 确认。
        confirmed = $false
    } | ConvertTo-Json -Depth 10
    # 以无 BOM UTF-8 写入一次性请求。
    [IO.File]::WriteAllText(
        # 使用精确 build fixture 路径。
        $appInput,
        # 写入 JSON 请求。
        $appRequest,
        # 禁止 BOM。
        [Text.UTF8Encoding]::new($false)
    )
    # 通过生产 launcher 执行 app facade 路线。
    $appResult = Invoke-LauncherJson -CommandArguments @(
        # 调用统一 app screenshot。
        'run', 'app', 'screenshot',
        # 传递一次性请求文件。
        '--input', $appInput,
        # 提供逐操作确认。
        '--confirm'
    )
    # 核对 app facade 仍返回同一 Rust capability 事实。
    if (-not $appResult.ok -or
        $appResult.capability -ne $policy.capability -or
        $appResult.targetId -ne $fixtureSessionId -or
        -not $appResult.data.atomicOutput -or
        $appResult.data.targetKind -ne 'application-window' -or
        $appResult.executionRealm -ne 'isolated-worker' -or
        -not $appResult.executionRealmCertified -or
        -not $appResult.meta.foreground.unchanged -or
        -not (Test-Path -LiteralPath $appPng -PathType Leaf)) {
        # 拒绝 facade fallback 或净化漂移。
        throw ('The Rust app facade screenshot route is invalid: ' +
            ($appResult | ConvertTo-Json -Depth 20 -Compress))
    }

    # 验证两个输出均具有 PNG signature。
    foreach ($png in @($desktopPng, $appPng)) {
        # 读取测试 PNG 字节。
        $bytes = [IO.File]::ReadAllBytes($png)
        # 核对八字节 PNG signature。
        if ($bytes.Length -lt 8 -or
            -not [Linq.Enumerable]::SequenceEqual(
                [byte[]] $bytes[0..7],
                [byte[]] @(137, 80, 78, 71, 13, 10, 26, 10)
            )) {
            # 拒绝错误容器。
            throw 'The Rust screenshot output is not a PNG.'
        }
    }
# 回收分支开始。
} finally {
    # 仅终止本脚本创建且仍存活的 fixture。
    if ($null -ne $fixtureProcess -and -not $fixtureProcess.HasExited) {
        # 终止工具自有 fixture。
        $fixtureProcess.Kill()
        # 等待自有进程退出。
        $fixtureProcess.WaitForExit()
    }
    # 精确验证删除目标位于本门禁根目录下。
    if ($outputRoot.StartsWith($gateRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $outputRoot)) {
        # 只清理本次 GUID 夹具目录。
        Remove-Item -LiteralPath $outputRoot -Recurse -Force
    }
}

# 等待 companion 退出状态稳定。
Start-Sleep -Milliseconds 100
# 保存运行后 worker 数量。
$workersAfter = @(
    # 只读取固定 capture worker 进程名。
    Get-Process -Name 'ai-computer-toolkit-capture-worker' -ErrorAction SilentlyContinue
).Count
# worker 数量不得增加。
if ($workersAfter -ne $workersBefore) {
    # 报告 Job 回收门禁失败。
    throw 'The Rust window screenshot route left an orphan worker.'
}
# 测试目录必须已清理。
if (Test-Path -LiteralPath $outputRoot) {
    # 报告 fixture 残留。
    throw 'The Rust window screenshot gate left output artifacts.'
}

# 输出稳定门禁摘要。
[PSCustomObject]@{
    # 标记门禁成功。
    ok = $true
    # 输出能力 ID。
    capability = $policy.capability
    # 声明唯一实现语言。
    implementation = $policy.implementation
    # 声明确认优先。
    confirmationFirst = $true
    # 声明 stale 失败闭合。
    staleTargetRefused = $true
    # 声明覆盖许可独立。
    overwriteRefused = $true
    # 声明 desktop 正式 Rust 路由通过。
    desktopRustRoute = $true
    # 声明 app facade 正式 Rust 路由通过。
    appRustRoute = $true
    # 声明原子 PNG 门禁通过。
    atomicOutputFixturePassed = $true
    # 声明只捕获工具自有窗口。
    selfOwnedFixtureCaptured = $true
    # 禁止把自动门禁误报为真实用户应用验收。
    realApplicationCaptured = $false
    # 声明未调用 C++。
    cppExecutionEnabled = $false
    # 声明未执行跨语言像素等价。
    cppPixelEquivalenceRequired = $false
    # 报告 worker 零孤儿。
    workerOrphans = 0
    # 报告测试产物零持久残留。
    persistentArtifacts = 0
} | ConvertTo-Json
# 保持脚本退出码成功。
$global:LASTEXITCODE = 0
