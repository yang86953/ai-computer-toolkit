[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$executable = Join-Path `
    $projectRoot 'target\debug\ai-computer-toolkit.exe'
$policy = Get-Content -LiteralPath (
    Join-Path `
        $projectRoot `
        'tests\contracts\capture-frame-probe-policy.json'
) -Raw | ConvertFrom-Json

# 优先使用 PATH 中的 cargo。
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
# 命中 PATH 时保存可执行文件绝对路径。
if ($null -ne $cargo) {
    # 将 CommandInfo 收敛为字符串路径。
    $cargo = $cargo.Source
}
# PATH 缺失时使用用户默认安装位置。
if ($null -eq $cargo) {
    # 构造用户级 cargo 路径。
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}
# cargo 缺失时禁止使用旧二进制。
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    # 返回明确 Rust 工具链缺口。
    throw 'cargo is unavailable for the Rust capture frame probe gate.'
}
# 构建主程序和固定 capture worker companion。
& $cargo build --quiet --bins --manifest-path (
    # 使用仓库权威 Cargo manifest。
    Join-Path $projectRoot 'Cargo.toml'
)
# 构建失败立即终止。
if ($LASTEXITCODE -ne 0) {
    # 返回稳定 Rust 构建错误。
    throw "Rust build failed with exit code $LASTEXITCODE."
}

function Invoke-JsonCommand {
    param(
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        [int] $ExpectedExitCode = 0
    )
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $executable
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.Arguments = $CommandArguments -join ' '
    $process = [System.Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected exit code $($process.ExitCode): $stdout $stderr"
    }
    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        throw "Unexpected stderr: $stderr"
    }
    return $stdout | ConvertFrom-Json
}

$missingConfirmation = Invoke-JsonCommand -CommandArguments @(
    'probe-capture-frame'
    'app'
    '--target'
    'sessionId=not-even-a-valid-target'
) -ExpectedExitCode 2
if (
    $missingConfirmation.ok -or
    $missingConfirmation.error.code -ne
        $policy.missingConfirmationError
) {
    throw 'Capture probe did not reject before target resolution.'
}

$stale = Invoke-JsonCommand -CommandArguments @(
    'probe-capture-frame'
    'app'
    '--target'
    'sessionId=s2:w:0000000000000000'
    '--confirm'
) -ExpectedExitCode 2
if ($stale.ok -or $stale.error.code -ne $policy.confirmedStaleTargetError) {
    throw 'Confirmed capture probe did not preserve stale-target semantics.'
}

$catalog = Invoke-JsonCommand -CommandArguments @(
    'capabilities', 'app'
)
$descriptor = @(
    $catalog.data.capabilities |
        Where-Object { $_.id -eq $policy.capability }
)
if (
    $descriptor.Count -ne 1 -or
    $descriptor[0].status -ne 'available' -or
    $descriptor[0].risk -ne 'read-sensitive' -or
    $descriptor[0].executionDomain -ne $policy.executionDomain -or
    -not $descriptor[0].requiresConfirmation
) {
    throw 'Capture frame probe catalog descriptor is invalid.'
}

$sessions = Invoke-JsonCommand -CommandArguments @(
    # 使用窗口 surface 取得 canonical s2:w 夹具。
    'sessions', 'window', '--max-items', '1'
)
$assessmentChecked = $false
if ($sessions.sessions.Count -gt 0) {
    $assessment = Invoke-JsonCommand -CommandArguments @(
        'assess'
        'app'
        '--capability'
        $policy.capability
        '--target'
        "sessionId=$($sessions.sessions[0].sessionId)"
    )
    if (
        -not $assessment.ok -or
        $assessment.decision -ne 'confirmation-required' -or
        $assessment.executionRealm -ne $policy.executionDomain -or
        -not $assessment.requiresConfirmation -or
        $assessment.requiresForegroundConsent -or
        $assessment.evidence.implementationState -ne
            'rust-available-awaiting-confirmation'
    ) {
        throw 'Capture frame probe assessment did not remain confirmation-gated.'
    }
    $assessmentChecked = $true
}

$workerSources = @(
    # 扫描 Rust worker 协议入口。
    (Join-Path $projectRoot 'src\capture_worker.rs')
    # 扫描 Rust 主 Module。
    (Join-Path $projectRoot 'src\modules\window_capture_frame_probe.rs')
)
foreach ($source in $workerSources) {
    foreach ($term in $policy.forbiddenWorkerOperations) {
        if (Select-String -LiteralPath $source -Pattern $term -SimpleMatch) {
            throw "Capture worker contains forbidden operation: $term"
        }
    }
}

# 定位 Rust 自有 no-activate fixture。
$fixtureExecutable = Join-Path $projectRoot `
    'target\debug\ai-computer-toolkit-capture-fixture.exe'
# fixture 必须来自本次 Rust 构建。
if (-not (Test-Path -LiteralPath $fixtureExecutable -PathType Leaf)) {
    # 禁止使用真实用户窗口替代自有夹具。
    throw 'The Rust capture fixture executable is missing.'
}
# 构造仅含固定前缀和 GUID 的唯一 ASCII 标题。
$fixtureTitle = 'act-rust-capture-fixture-' + [Guid]::NewGuid().ToString('N')
# 保存运行前 capture worker 数量用于孤儿门禁。
$workersBefore = @(
    # 只读取同名 worker 进程。
    Get-Process -Name 'ai-computer-toolkit-capture-worker' `
        -ErrorAction SilentlyContinue
).Count
# 配置无控制台的自有 fixture 进程。
$fixtureStart = [System.Diagnostics.ProcessStartInfo]::new()
# 只允许固定 Rust fixture 二进制。
$fixtureStart.FileName = $fixtureExecutable
# 禁止 shell 改写参数。
$fixtureStart.UseShellExecute = $false
# 隐藏 console；fixture 自己只显示 no-activate 测试窗口。
$fixtureStart.CreateNoWindow = $true
# 传递经过 fixture 固定前缀校验的标题。
$fixtureStart.Arguments = $fixtureTitle
# 启动工具自有 fixture。
$fixtureProcess = [System.Diagnostics.Process]::Start($fixtureStart)
# 初始化端到端结果。
$probe = $null
# 确保所有路径回收自有 fixture。
try {
    # 初始化 opaque 目标。
    $fixtureSessionId = $null
    # 有界等待 fixture 进入窗口快照。
    foreach ($attempt in 1..50) {
        # 枚举当前 Rust 窗口 surface。
        $windowSnapshot = Invoke-JsonCommand -CommandArguments @(
            # 请求完整有界窗口清单。
            'sessions', 'window', '--max-items', '4096'
        )
        # 按唯一测试标题选择自有窗口。
        $fixtureWindow = @(
            # 只匹配本次自有 fixture。
            $windowSnapshot.sessions |
                Where-Object { $_.title -eq $fixtureTitle }
        )
        # 唯一命中后保存 opaque 目标。
        if ($fixtureWindow.Count -eq 1) {
            # 保存 canonical sessionId。
            $fixtureSessionId = [string] $fixtureWindow[0].sessionId
            # 结束轮询。
            break
        }
        # fixture 提前退出表示创建失败。
        if ($fixtureProcess.HasExited) {
            # 禁止回落真实用户窗口。
            throw 'The Rust capture fixture exited before discovery.'
        }
        # 短暂等待而不阻塞用户交互。
        Start-Sleep -Milliseconds 100
    }
    # 未发现自有 fixture 时失败。
    if ([string]::IsNullOrWhiteSpace($fixtureSessionId)) {
        # 禁止扩大到任意窗口。
        throw 'The Rust capture fixture was not uniquely discovered.'
    }
    # 对自有 fixture 提供明确测试确认并执行正式 launcher 路径。
    $probe = Invoke-JsonCommand -CommandArguments @(
        # 调用首帧元数据探针。
        'probe-capture-frame'
        # 使用统一 app surface。
        'app'
        # 传递 opaque 目标选项。
        '--target'
        # 只传递本次自有窗口 sessionId。
        "sessionId=$fixtureSessionId"
        # 明确确认工具自有 fixture。
        '--confirm'
        # 使用有界 worker deadline。
        '--timeout-ms'
        # 保持默认五秒门禁。
        '5000'
    )
    # 成功 envelope 必须来自 Rust。
    if (-not $probe.ok -or $probe.implementation -ne 'rust') {
        # 拒绝 fallback 或伪成功。
        throw 'The Rust capture frame probe did not succeed on its own fixture.'
    }
    # 核对公开 capability 与 opaque 目标。
    if ($probe.data.capability -ne $policy.capability -or
        # 目标必须精确回显 opaque session。
        $probe.data.targetId -ne $fixtureSessionId) {
        # 拒绝跨目标结果。
        throw 'The capture frame probe returned the wrong public target.'
    }
    # 首帧尺寸和驱动分类必须封闭有效。
    if ($probe.data.frame.width -lt 1 -or
        # 高度必须为正。
        $probe.data.frame.height -lt 1 -or
        # 驱动只能是硬件或 WARP。
        $probe.data.frame.deviceDriver -notin @('hardware', 'warp')) {
        # 拒绝无效帧元数据。
        throw 'The capture frame metadata is invalid.'
    }
    # 核对零 surface、零像素和零文件承诺。
    if (-not $probe.data.safety.frameAcquired -or
        # 禁止 surface 访问。
        $probe.data.safety.frameSurfaceAccessed -ne $policy.frameSurfaceAccessed -or
        # 禁止像素持久化。
        $probe.data.safety.pixelsPersisted -ne $policy.pixelsPersisted -or
        # 禁止文件写入。
        $probe.data.safety.fileWritten -or
        # 主进程和 worker 前景必须保持不变。
        -not $probe.data.foregroundUnchanged) {
        # 拒绝安全证明漂移。
        throw 'The capture frame probe safety proof is invalid.'
    }
    # 序列化公开 envelope 以扫描禁止字段。
    $probeJson = $probe | ConvertTo-Json -Depth 20 -Compress
    # 逐项检查禁止原生字段。
    foreach ($field in $policy.forbiddenPublicFields) {
        # 构造精确 JSON key 模式。
        $pattern = '"' + [Regex]::Escape($field) + '"\s*:'
        # 任一命中立即失败。
        if ($probeJson -match $pattern) {
            # 不回显可能敏感的完整结果。
            throw "Capture probe leaked forbidden field '$field'."
        }
    }
# 无论成功或失败都回收自有 fixture。
} finally {
    # 仅终止本脚本创建且仍存活的精确进程。
    if ($null -ne $fixtureProcess -and -not $fixtureProcess.HasExited) {
        # 终止工具自有 fixture。
        $fixtureProcess.Kill()
        # 等待进程退出，防止夹具残留。
        $fixtureProcess.WaitForExit()
    }
}
# 等待 companion 退出状态稳定。
Start-Sleep -Milliseconds 100
# 保存运行后同名 worker 数量。
$workersAfter = @(
    # 只读取同名 worker 进程。
    Get-Process -Name 'ai-computer-toolkit-capture-worker' `
        -ErrorAction SilentlyContinue
).Count
# worker 数量不得增加。
if ($workersAfter -ne $workersBefore) {
    # 报告孤儿进程门禁失败。
    throw 'The capture frame probe left an orphan worker.'
}

[PSCustomObject]@{
    ok = $true
    capability = $policy.capability
    missingConfirmationError = $missingConfirmation.error.code
    confirmedStaleTargetError = $stale.error.code
    assessmentChecked = $assessmentChecked
    realApplicationFrameCaptured = $false
    selfOwnedFixtureFrameCaptured = $true
    targetResolvedBeforeConfirmation = $false
    nativeIdentifierLeak = $false
    filesWritten = 0
    foregroundUnchanged = $probe.data.foregroundUnchanged
    workerOrphans = 0
} | ConvertTo-Json
