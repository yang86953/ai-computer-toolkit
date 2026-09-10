[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$executable = Join-Path `
    $projectRoot 'target\debug\ai-computer-toolkit.exe'
$policy = Get-Content -LiteralPath (
    Join-Path $projectRoot 'tests\contracts\capture-preflight-policy.json'
) -Raw | ConvertFrom-Json

# 定位 Rust cargo proxy。
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
# 缺失 Rust 工具链时失败闭合。
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    # 禁止回退 C++。
    throw 'cargo is unavailable for the Rust capture preflight gate.'
}
# 定位仓库权威 manifest。
$manifestPath = Join-Path $projectRoot 'Cargo.toml'
# 构建 Rust 主程序与固定 companions。
& $cargo build --quiet --bins --manifest-path $manifestPath
if ($LASTEXITCODE -ne 0) {
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
    foreach ($argument in $CommandArguments) {
        if ($argument -match '[\s"]') {
            throw "Test argument requires unsupported quoting: $argument"
        }
    }
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

$sessions = Invoke-JsonCommand -CommandArguments @(
    'sessions', 'window', '--max-items', '100'
)
$tested = 0
$eligibility = @{}
$wgcRuntime = @{}
$wgcItemInterop = @{}
foreach ($session in $sessions.sessions) {
    $result = Invoke-JsonCommand -CommandArguments @(
        'preflight-capture'
        'app'
        '--target'
        "sessionId=$($session.sessionId)"
    )
    if (
        -not $result.ok -or
        $result.data.capability -ne $policy.capability -or
        -not $result.data.readOnly -or
        -not $result.data.foregroundUnchanged -or
        $result.data.foregroundRequired -or
        -not $result.data.captureExecutionMigrated -or
        $result.data.safety.pixelsRead -or
        $result.data.safety.captureSessionStarted -or
        $result.data.safety.framePoolCreated -or
        $result.data.safety.fileWritten -or
        $result.data.safety.windowActivated -or
        $result.data.safety.inputSent -or
        $result.data.safety.nativeTargetExposed
    ) {
        throw 'Capture preflight violated its read-only safety envelope.'
    }
    if (
        $policy.allowedContentProtection -notcontains
            $result.data.evidence.contentProtection
    ) {
        throw 'Capture preflight returned an unknown protection state.'
    }
    if (
        $policy.allowedWgcRuntime -notcontains
            $result.data.evidence.wgcRuntime -or
        $policy.allowedWgcItemInterop -notcontains
            $result.data.evidence.wgcItemInterop
    ) {
        throw 'Capture preflight returned an unknown WGC state.'
    }
    $serialized = $result | ConvertTo-Json -Depth 20 -Compress
    foreach ($field in $policy.forbiddenPublicFields) {
        if ($serialized -match "`"$([regex]::Escape($field))`"\s*:") {
            throw "Capture preflight leaked forbidden field: $field"
        }
    }
    $name = [string] $result.data.evidence.eligibility
    $current = 0
    if ($eligibility.ContainsKey($name)) {
        $current = [int] $eligibility[$name]
    }
    $eligibility[$name] = 1 + $current
    $runtimeName = [string] $result.data.evidence.wgcRuntime
    $runtimeCount = 0
    if ($wgcRuntime.ContainsKey($runtimeName)) {
        $runtimeCount = [int] $wgcRuntime[$runtimeName]
    }
    $wgcRuntime[$runtimeName] = 1 + $runtimeCount
    $itemName = [string] $result.data.evidence.wgcItemInterop
    $itemCount = 0
    if ($wgcItemInterop.ContainsKey($itemName)) {
        $itemCount = [int] $wgcItemInterop[$itemName]
    }
    $wgcItemInterop[$itemName] = 1 + $itemCount
    $tested++
}

$stale = Invoke-JsonCommand -CommandArguments @(
    'preflight-capture'
    'app'
    '--target'
    'sessionId=s2:w:0000000000000000'
) -ExpectedExitCode 2
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    throw 'Capture preflight did not preserve stale exact-target semantics.'
}

if ($sessions.sessions.Count -gt 0) {
    $target = "sessionId=$($sessions.sessions[0].sessionId)"
    $preflightAssessment = Invoke-JsonCommand -CommandArguments @(
        'assess'
        'app'
        '--capability'
        $policy.capability
        '--target'
        $target
    )
    if (
        -not $preflightAssessment.ok -or
        $preflightAssessment.decision -ne 'executable-background' -or
        $preflightAssessment.executionRealm -ne 'host-headless'
    ) {
        throw 'Preflight capability assessment is not executable read-only.'
    }

    $captureAssessment = Invoke-JsonCommand -CommandArguments @(
        'assess'
        'app'
        '--capability'
        $policy.captureCapability
        '--target'
        $target
    )
    if (
        -not $captureAssessment.ok -or
        $captureAssessment.decision -ne 'confirmation-required' -or
        $captureAssessment.executionRealm -ne 'isolated-worker' -or
        -not $captureAssessment.requiresConfirmation
    ) {
        throw 'Migrated Rust screenshot assessment is not confirmation-gated.'
    }
}

$backend = Join-Path `
    $projectRoot 'src\adapters\window_capture_preflight_windows.rs'
foreach ($api in $policy.forbiddenPreflightTokens) {
    if (Select-String -LiteralPath $backend -Pattern $api -SimpleMatch) {
        throw "Capture preflight uses forbidden API: $api"
    }
}

[PSCustomObject]@{
    ok = $true
    inspectedWindows = $tested
    eligibility = $eligibility
    wgcRuntime = $wgcRuntime
    wgcItemInterop = $wgcItemInterop
    foregroundUnchanged = $true
    nativeIdentifierLeak = $false
    pixelsRead = $false
    captureSessionsStarted = 0
    framePoolsCreated = 0
    filesWritten = $false
    staleTargetError = $stale.error.code
    screenshotExecution = 'available-confirmed'
} | ConvertTo-Json -Depth 10
