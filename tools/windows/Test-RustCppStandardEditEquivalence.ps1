[CmdletBinding()]
param(
    [switch] $SkipBuild
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$fixtureRoot = Join-Path $outputRoot 'standard-edit-equivalence'
$fixture = Join-Path $fixtureRoot (
    'act-standard-edit-equivalence-fixture.exe'
)
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
$rustRequestPath = Join-Path $fixtureRoot 'rust-request.json'
$cppRequestPath = Join-Path $fixtureRoot 'cpp-request.json'
$rustOutputPath = Join-Path $fixtureRoot 'rust-readback.txt'
$cppOutputPath = Join-Path $fixtureRoot 'cpp-readback.txt'
$policyPath = Join-Path $projectRoot (
    'tests\contracts\standard-edit-compatibility-policy.json'
)
$resolvedBuild = [IO.Path]::GetFullPath($outputRoot)
$resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
if (-not $resolvedFixture.StartsWith(
    $resolvedBuild + [IO.Path]::DirectorySeparatorChar,
    [StringComparison]::OrdinalIgnoreCase
)) {
    throw 'Standard Edit fixture escaped the build directory.'
}
New-Item -ItemType Directory -Path $fixtureRoot -Force |
    Out-Null
$policy = Get-Content -LiteralPath $policyPath -Raw |
    ConvertFrom-Json
if ($policy.capability -ne 'ui.text.input@1' -or
    $policy.maximumUtf8Bytes -ne 65536 -or
    $policy.executionRealm -ne 'same-session-no-focus' -or
    $policy.compatibilityShape -ne
        'provider-neutral-standard-edit-v1' -or
    $policy.primaryImplementation -ne 'rust' -or
    $policy.launcherRoute -ne 'rust' -or
    $policy.cppRole -ne 'direct-equivalence-reference' -or
    $policy.timeout.retrySafe -or
    -not $policy.timeout.targetMayHaveMutated) {
    throw 'Standard Edit compatibility policy is invalid.'
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
    & $compiler @(
        '-std=c++23'
        '-Wall'
        '-Wextra'
        '-Wpedantic'
        '-Werror'
        '-DUNICODE'
        '-D_UNICODE'
        '-DWIN32_LEAN_AND_MEAN'
        '-DNOMINMAX'
        (Join-Path $sourceRoot (
            'tests\standard_edit_equivalence_fixture.cpp'
        ))
        '-luser32'
        '-mwindows'
        '-o'
        $fixture
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Standard Edit equivalence fixture build failed.'
    }
}

function Start-Fixture {
    param([Parameter(Mandatory)][string] $Output)
    [IO.File]::Delete($Output)
    $process = Start-Process `
        -FilePath $fixture `
        -ArgumentList @($Output) `
        -WindowStyle Hidden `
        -PassThru
    Start-Sleep -Milliseconds 300
    if ($process.HasExited) {
        throw "Standard Edit fixture exited early: $($process.ExitCode)"
    }
    return $process
}

function Wait-Fixture {
    param(
        [Parameter(Mandatory)] $Process,
        [Parameter(Mandatory)][string] $Output,
        [Parameter(Mandatory)][string] $Expected
    )
    if (-not $Process.WaitForExit(5000)) {
        Stop-Process -Id $Process.Id -Force
        throw 'Standard Edit fixture did not observe the mutation.'
    }
    if ($Process.ExitCode -ne 0 -or
        -not (Test-Path -LiteralPath $Output) -or
        [IO.File]::ReadAllText(
            $Output,
            [Text.Encoding]::UTF8
        ) -cne $Expected) {
        throw 'Standard Edit fixture readback diverged.'
    }
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

$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$unicodeSample = -join @(
    [char]0x7F16
    [char]0x8F91
    [char]0x2713
)
$expectedText = "Rust/C++ Standard Edit $stamp $unicodeSample"
$foregroundBefore = (
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ActForegroundProbe {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@ -PassThru
)::GetForegroundWindow()
$rustFixture = $null
$cppFixture = $null
try {
    $rustFixture = Start-Fixture -Output $rustOutputPath
    $rustSessions = & $rust sessions app `
        --max-items 4096 |
        ConvertFrom-Json
    $rustTargets = @(
        $rustSessions.sessions |
            Where-Object {
                $_.applicationName -eq (
                    Split-Path -Leaf $fixture
                ) -and
                $_.sessionId -like 's2:c:*' -and
                $_.capabilities.id -contains
                    'ui.text.input@1'
            }
    )
    if ($rustTargets.Count -ne 1) {
        throw 'Rust did not discover exactly one owned Edit target.'
    }
    $rustTarget = $rustTargets[0].sessionId
    $rustAssessment = & $rust assess app `
        --capability ui.text.input@1 `
        --target "sessionId=$rustTarget" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        $rustAssessment.decision -ne 'confirmation-required' -or
        $rustAssessment.executionRealm -ne
            'same-session-no-focus' -or
        -not $rustAssessment.requiresConfirmation -or
        $rustAssessment.requiresForegroundConsent -or
        $rustAssessment.constraints.readOnly -or
        $rustAssessment.evidence.targetKind -ne
            'standard-edit-control') {
        throw 'Rust Standard Edit assessment is not fail-closed.'
    }
    $rustRequest = [ordered]@{
        target = [ordered]@{ sessionId = $rustTarget }
        args = [ordered]@{
            capability = 'ui.text.input@1'
            input = [ordered]@{
                text = $expectedText
                timeoutMs = 2000
            }
        }
    }
    [IO.File]::WriteAllText(
        $rustRequestPath,
        ($rustRequest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    $rustUnconfirmed = & $rust run app apply `
        --input $rustRequestPath |
        ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or
        $rustUnconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED' -or
        (Test-Path -LiteralPath $rustOutputPath)) {
        throw 'Rust Standard Edit did not reject before mutation.'
    }
    $rustResult = & $rust run app apply `
        --input $rustRequestPath `
        --confirm |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $rustResult.ok -or
        $rustResult.capability -ne
            'ui.text.input@1' -or
        -not $rustResult.data.verifiedByReadback -or
        -not $rustResult.meta.foreground.unchanged -or
        $rustResult.targetId -ne $rustTarget -or
        $rustResult.compatibilityShape -ne
            'provider-neutral-standard-edit-v1') {
        throw (
            'Rust provider-neutral Standard Edit execution failed: ' +
            ($rustResult | ConvertTo-Json -Depth 20 -Compress)
        )
    }
    $rustSerialized = $rustResult |
        ConvertTo-Json -Depth 20 -Compress
    if ($rustSerialized -match
        '"(hwnd|processId|className|nativeControl)"') {
        throw 'Rust Standard Edit facade leaked a native identifier.'
    }
    Wait-Fixture `
        -Process $rustFixture `
        -Output $rustOutputPath `
        -Expected $expectedText
    $rustFixture.Dispose()
    $rustFixture = $null

    $cppFixture = Start-Fixture -Output $cppOutputPath
    $cppSessions = & $cpp sessions app --max-items 4096 |
        ConvertFrom-Json
    $cppTargets = @(
        $cppSessions.data.sessions |
            Where-Object {
                $_.applicationName -eq (
                    Split-Path -Leaf $fixture
                ) -and
                $_.sessionId -like 's2:c:*' -and
                $_.capabilities.id -contains
                    'ui.text.input@1'
            }
    )
    if ($cppTargets.Count -ne 1) {
        throw 'C++ did not discover exactly one owned opaque Edit target.'
    }
    $cppTarget = $cppTargets[0].sessionId
    $assessment = & $cpp assess app `
        --capability ui.text.input@1 `
        --target "sessionId=$cppTarget" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        $assessment.decision -ne 'confirmation-required' -or
        $assessment.executionRealm -ne
            'same-session-no-focus' -or
        -not $assessment.requiresConfirmation -or
        $assessment.requiresForegroundConsent -or
        $assessment.constraints.readOnly -or
        $assessment.evidence.targetKind -ne
            'standard-edit-control') {
        throw 'C++ Standard Edit assessment is not fail-closed.'
    }
    $request = [ordered]@{
        target = [ordered]@{ sessionId = $cppTarget }
        args = [ordered]@{
            capability = 'ui.text.input@1'
            input = [ordered]@{
                text = $expectedText
                timeoutMs = 2000
            }
        }
    }
    [IO.File]::WriteAllText(
        $cppRequestPath,
        ($request | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    $unconfirmed = & $cpp run app apply `
        --input $cppRequestPath |
        ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or
        $unconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED' -or
        (Test-Path -LiteralPath $cppOutputPath)) {
        throw 'C++ Standard Edit did not reject before mutation.'
    }
    $cppResult = & $cpp run app apply `
        --input $cppRequestPath `
        --confirm |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $cppResult.ok -or
        $cppResult.capability -ne
            'ui.text.input@1' -or
        -not $cppResult.data.verifiedByReadback -or
        -not $cppResult.meta.foreground.unchanged -or
        $cppResult.targetId -ne $cppTarget -or
        $cppResult.compatibilityShape -ne
            'provider-neutral-standard-edit-v1') {
        throw (
            'C++ provider-neutral Standard Edit execution failed: ' +
            ($cppResult | ConvertTo-Json -Depth 20 -Compress)
        )
    }
    $serialized = $cppResult |
        ConvertTo-Json -Depth 20 -Compress
    if ($serialized -match
        '"(hwnd|processId|className|nativeControl)"') {
        throw 'C++ Standard Edit facade leaked a native identifier.'
    }
    Wait-Fixture `
        -Process $cppFixture `
        -Output $cppOutputPath `
        -Expected $expectedText
    $cppFixture.Dispose()
    $cppFixture = $null
    $foregroundAfter =
        [ActForegroundProbe]::GetForegroundWindow()
    if ($foregroundBefore -ne $foregroundAfter) {
        throw 'Standard Edit equivalence changed the foreground.'
    }
} finally {
    Stop-Fixture -Process $rustFixture
    Stop-Fixture -Process $cppFixture
    foreach ($generated in @(
        $rustRequestPath
        $cppRequestPath
        $rustOutputPath
        $cppOutputPath
    )) {
        $resolved = [IO.Path]::GetFullPath($generated)
        if (-not $resolved.StartsWith(
            $resolvedFixture +
                [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw 'Refusing to clean a file outside the fixture directory.'
        }
        [IO.File]::Delete($resolved)
    }
}

[PSCustomObject]@{
    ok = $true
    rustTarget = $rustTarget
    cppTarget = $cppTarget
    exactUtf8ReadbackEquivalent = $true
    structuredAssessment = $true
    confirmationFirst = $true
    foregroundUnchanged = $true
    facadeSanitized = $true
    selfOwnedFixturesOnly = $true
    userApplicationsWritten = 0
} | ConvertTo-Json
$global:LASTEXITCODE = 0
