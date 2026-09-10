[CmdletBinding()]
param(
    [switch] $SkipBuild
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$fixtureRoot = Join-Path $outputRoot 'window-close-equivalence'
$fixture = Join-Path $fixtureRoot (
    'act-window-close-equivalence-fixture.exe'
)
$moduleTest = Join-Path $outputRoot (
    'act-window-close-module-test.exe'
)
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
$rustRequestPath = Join-Path $fixtureRoot 'rust-request.json'
$cppRequestPath = Join-Path $fixtureRoot 'cpp-request.json'
$policyPath = Join-Path $projectRoot (
    'tests\contracts\window-close-compatibility-policy.json'
)
$resolvedBuild = [IO.Path]::GetFullPath($outputRoot)
$resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
if (-not $resolvedFixture.StartsWith(
    $resolvedBuild + [IO.Path]::DirectorySeparatorChar,
    [StringComparison]::OrdinalIgnoreCase
)) {
    throw 'Window close fixture escaped the build directory.'
}
New-Item -ItemType Directory -Path $fixtureRoot -Force |
    Out-Null
$policy = Get-Content -LiteralPath $policyPath -Raw |
    ConvertFrom-Json
if ($policy.capability -ne 'window.close@1' -or
    $policy.executionRealm -ne 'same-session-no-focus' -or
    $policy.foregroundTargetAllowed -or
    $policy.timeout.retrySafe -or
    -not $policy.timeout.targetMayCloseLater -or
    $policy.compatibilityShape -ne
        'provider-neutral-window-close-v1' -or
    $policy.primaryImplementation -ne 'rust' -or
    $policy.launcherRoute -ne 'rust' -or
    $policy.cppRole -ne 'direct-equivalence-reference') {
    throw 'Window close compatibility policy is invalid.'
}

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
    cargo build --quiet --manifest-path (
        Join-Path $projectRoot 'Cargo.toml'
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Rust compatibility build failed.'
    }
    $compiler = (Get-Command clang++ -ErrorAction Stop).Source
    $common = @(
        '-std=c++23'
        '-Wall'
        '-Wextra'
        '-Wpedantic'
        '-Werror'
        '-DUNICODE'
        '-D_UNICODE'
        '-DWIN32_LEAN_AND_MEAN'
        '-DNOMINMAX'
        "-I$(Join-Path $sourceRoot 'src')"
    )
    & $compiler @common @(
        (Join-Path $sourceRoot (
            'tests\screenshot_equivalence_fixture.cpp'
        ))
        '-luser32'
        '-mwindows'
        '-o'
        $fixture
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Window close equivalence fixture build failed.'
    }
    & $compiler @common @(
        (Join-Path $sourceRoot 'tests\window_close_module_test.cpp')
        (Join-Path $sourceRoot 'src\components\json.cpp')
        (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
        (Join-Path $sourceRoot 'src\modules\window_close_module.cpp')
        (Join-Path $sourceRoot (
            'src\modules\window_close_compatibility_module.cpp'
        ))
        (Join-Path $sourceRoot (
            'src\platform\windows\discovery_backend.cpp'
        ))
        (Join-Path $sourceRoot (
            'src\platform\windows\text_codec.cpp'
        ))
        (Join-Path $sourceRoot (
            'src\platform\windows\window_close_backend.cpp'
        ))
        '-lole32'
        '-loleaut32'
        '-luuid'
        '-luser32'
        '-ldwmapi'
        '-o'
        $moduleTest
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Window close module test build failed.'
    }
}
$module = & $moduleTest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $module.ok -or
    -not $module.confirmationFirst -or
    -not $module.staleTargetRefused -or
    -not $module.timeoutBounded -or
    -not $module.exactOpaqueTarget -or
    -not $module.closed -or
    -not $module.timeoutOutcomeUnknown -or
    $module.retrySafe -or
    -not $module.foregroundUnchanged -or
    -not $module.facadeMappedSafely -or
    $module.userWindowsClosed -ne 0) {
    throw 'Window close module safety gate failed.'
}

function Start-Fixture {
    param([Parameter(Mandatory)][string] $Title)
    $process = Start-Process `
        -FilePath $fixture `
        -ArgumentList @($Title) `
        -WindowStyle Hidden `
        -PassThru
    Start-Sleep -Milliseconds 300
    if ($process.HasExited) {
        throw "Window close fixture exited early: $($process.ExitCode)"
    }
    return $process
}

function Stop-Fixture {
    param($Process)
    if ($null -ne $Process) {
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force
            [void]$Process.WaitForExit(5000)
        }
        $Process.Dispose()
    }
}

function Write-Request {
    param(
        [Parameter(Mandatory)][string] $Path,
        [Parameter(Mandatory)][string] $SessionId,
        [switch] $Confirmed
    )
    $request = [ordered]@{
        target = [ordered]@{ sessionId = $SessionId }
        args = [ordered]@{
            capability = 'window.close@1'
            input = [ordered]@{ timeoutMs = 2000 }
        }
        confirmed = [bool]$Confirmed
    }
    [IO.File]::WriteAllText(
        $Path,
        ($request | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
}

$foregroundProbe = (
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ActWindowCloseForegroundProbe {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@ -PassThru
)
$foregroundBefore =
    [ActWindowCloseForegroundProbe]::GetForegroundWindow()
$rustFixture = $null
$cppFixture = $null
try {
    $stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $rustTitle = "ACT_RUST_WINDOW_CLOSE_$stamp"
    $rustFixture = Start-Fixture -Title $rustTitle
    $rustSessions = & $rust sessions app --max-items 4096 |
        ConvertFrom-Json
    $rustTargets = @(
        $rustSessions.sessions |
            Where-Object {
                $_.title -eq $rustTitle -and
                $_.capabilities.id -contains 'window.close@1'
            }
    )
    # Rust 反向迁移后必须发布 canonical s2:w 窗口目标.
    if ($rustTargets.Count -ne 1 -or
        # 拒绝旧 s1:c2 会话回归.
        $rustTargets[0].sessionId -notlike 's2:w:*') {
        throw 'Rust did not discover one exact close fixture.'
    }
    $rustAssessment = & $rust assess app `
        --capability window.close@1 `
        --target "sessionId=$($rustTargets[0].sessionId)" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        $rustAssessment.decision -ne 'confirmation-required' -or
        $rustAssessment.executionRealm -ne 'same-session-no-focus' -or
        -not $rustAssessment.requiresConfirmation -or
        $rustAssessment.requiresForegroundConsent -or
        $rustAssessment.constraints.readOnly) {
        throw 'Rust window close assessment is not fail-closed.'
    }
    Write-Request `
        -Path $rustRequestPath `
        -SessionId $rustTargets[0].sessionId
    $rustUnconfirmed = & $rust run app close `
        --input $rustRequestPath |
        ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or
        $rustUnconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED' -or
        $rustFixture.HasExited) {
        throw 'Rust window close did not reject before mutation.'
    }
    Write-Request `
        -Path $rustRequestPath `
        -SessionId $rustTargets[0].sessionId `
        -Confirmed
    $rustResult = & $rust run app close `
        --input $rustRequestPath `
        --confirm |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $rustResult.ok -or
        $rustResult.capability -ne 'window.close@1' -or
        $rustResult.targetId -ne $rustTargets[0].sessionId -or
        -not $rustResult.data.closed -or
        -not $rustResult.meta.foreground.unchanged -or
        $rustResult.compatibilityShape -ne
            'provider-neutral-window-close-v1') {
        throw 'Rust app close execution failed.'
    }
    $rustSerialized = $rustResult |
        ConvertTo-Json -Depth 20 -Compress
    if ($rustSerialized -match
        '"(hwnd|processId|className|nativeWindow)"') {
        throw 'Rust window close facade leaked native identifiers.'
    }
    if (-not $rustFixture.WaitForExit(5000)) {
        throw 'Rust close fixture did not exit.'
    }
    $rustFixture.Dispose()
    $rustFixture = $null

    $cppTitle = "ACT_CPP_WINDOW_CLOSE_$stamp"
    $cppFixture = Start-Fixture -Title $cppTitle
    $cppSessions = & $cpp sessions app --max-items 4096 |
        ConvertFrom-Json
    $cppTargets = @(
        $cppSessions.data.sessions |
            Where-Object {
                $_.title -eq $cppTitle -and
                $_.sessionId -like 's2:w:*' -and
                $_.capabilities.id -contains 'window.close@1'
            }
    )
    if ($cppTargets.Count -ne 1) {
        throw 'C++ did not discover one exact opaque close fixture.'
    }
    $cppTarget = $cppTargets[0].sessionId
    $assessment = & $cpp assess app `
        --capability window.close@1 `
        --target "sessionId=$cppTarget" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        $assessment.decision -ne 'confirmation-required' -or
        $assessment.executionRealm -ne 'same-session-no-focus' -or
        -not $assessment.requiresConfirmation -or
        $assessment.requiresForegroundConsent -or
        $assessment.constraints.readOnly) {
        throw 'C++ window close assessment is not fail-closed.'
    }
    Write-Request -Path $cppRequestPath -SessionId $cppTarget
    $unconfirmed = & $cpp run app close `
        --input $cppRequestPath |
        ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or
        $unconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED' -or
        $cppFixture.HasExited) {
        throw 'C++ window close did not reject before mutation.'
    }
    Write-Request `
        -Path $cppRequestPath `
        -SessionId $cppTarget `
        -Confirmed
    $cppResult = & $cpp run app close `
        --input $cppRequestPath `
        --confirm |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $cppResult.ok -or
        $cppResult.capability -ne 'window.close@1' -or
        $cppResult.targetId -ne $cppTarget -or
        -not $cppResult.data.closed -or
        -not $cppResult.meta.foreground.unchanged -or
        $cppResult.compatibilityShape -ne
            'provider-neutral-window-close-v1') {
        throw 'C++ provider-neutral window close failed.'
    }
    if (-not $cppFixture.WaitForExit(5000)) {
        throw 'C++ close fixture did not exit.'
    }
    $serialized = $cppResult |
        ConvertTo-Json -Depth 20 -Compress
    if ($serialized -match
        '"(hwnd|processId|className|nativeWindow)"') {
        throw 'C++ window close facade leaked native identifiers.'
    }
    $cppFixture.Dispose()
    $cppFixture = $null
    if ($foregroundBefore -ne
        [ActWindowCloseForegroundProbe]::GetForegroundWindow()) {
        throw 'Window close equivalence changed foreground ownership.'
    }
} finally {
    Stop-Fixture -Process $rustFixture
    Stop-Fixture -Process $cppFixture
    foreach ($generated in @(
        $rustRequestPath
        $cppRequestPath
    )) {
        $resolved = [IO.Path]::GetFullPath($generated)
        if (-not $resolved.StartsWith(
            $resolvedFixture +
                [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw 'Refusing to clean outside the close fixture directory.'
        }
        [IO.File]::Delete($resolved)
    }
}

[PSCustomObject]@{
    ok = $true
    rustTarget = $rustTargets[0].sessionId
    cppTarget = $cppTarget
    exactCloseEquivalent = $true
    structuredAssessment = $true
    confirmationFirst = $true
    timeoutOutcomeUnknown = $true
    foregroundUnchanged = $true
    facadeSanitized = $true
    selfOwnedFixturesOnly = $true
    userWindowsClosed = 0
} | ConvertTo-Json
$global:LASTEXITCODE = 0
