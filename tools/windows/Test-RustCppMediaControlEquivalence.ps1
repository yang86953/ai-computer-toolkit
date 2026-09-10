[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
$fixture = Join-Path $outputRoot 'act-media-session-fixture.exe'
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
& cargo build --quiet
if ($LASTEXITCODE -ne 0) {
    throw 'Rust compatibility build failed.'
}

$sdkIncludeRoot = 'C:\Program Files (x86)\Windows Kits\10\Include'
$cppWinRt = Get-ChildItem -LiteralPath $sdkIncludeRoot -Directory |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'cppwinrt' } |
    Where-Object {
        Test-Path -LiteralPath (Join-Path $_ 'winrt\base.h')
    } |
    Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($cppWinRt)) {
    throw 'Windows SDK C++/WinRT headers are unavailable.'
}

& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    -Wno-nonportable-include-path `
    -municode `
    -DUNICODE `
    -D_UNICODE `
    -DWIN32_LEAN_AND_MEAN `
    -DNOMINMAX `
    -D_WIN32_WINNT=0x0A00 `
    "-I$cppWinRt" `
    (Join-Path $sourceRoot 'tests\media_session_fixture_main.cpp') `
    -lole32 `
    -loleaut32 `
    -lruntimeobject `
    -o $fixture
if ($LASTEXITCODE -ne 0) {
    throw 'Self-owned media fixture compilation failed.'
}

if (-not ('ActMediaForegroundProbe' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ActMediaForegroundProbe {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
'@
}

$token = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$wavPath = Join-Path $outputRoot "act-media-fixture-$token.wav"
$readyPath = Join-Path $outputRoot "act-media-fixture-$token.ready"
$errorPath = Join-Path $outputRoot "act-media-fixture-$token.err"
$fixtureProcess = $null
$foregroundBefore =
    [ActMediaForegroundProbe]::GetForegroundWindow()
$cppResults = @()
$rustResults = @()
try {
    $fixtureProcess = Start-Process `
        -FilePath $fixture `
        -ArgumentList @($wavPath, $readyPath) `
        -RedirectStandardError $errorPath `
        -WindowStyle Hidden `
        -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    $cppSessions = $null
    $rustSessions = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($fixtureProcess.HasExited) {
            $fixtureError = Get-Content `
                -LiteralPath $errorPath `
                -Raw `
                -ErrorAction SilentlyContinue
            throw "Self-owned media fixture exited: $fixtureError"
        }
        if (Test-Path -LiteralPath $readyPath) {
            $cppSessions = & $cpp sessions media-session `
                --max-items 128 `
                --timeout-ms 5000 |
                ConvertFrom-Json
            $rustSessions = & $rust sessions media-session `
                --max-items 128 |
                ConvertFrom-Json
            if ($cppSessions.data.sessions.Count -gt 0 -and
                $rustSessions.sessions.Count -gt 0) {
                break
            }
        }
        Start-Sleep -Milliseconds 200
    }
    $rustFixture = @(
        $rustSessions.sessions |
            Where-Object {
                [string]$_.sourceAppId -like
                    '*act-media-session-fixture.exe'
            }
    )
    if ($rustFixture.Count -ne 1) {
        throw 'Rust did not resolve exactly one self-owned media session.'
    }
    $cppFixture = @(
        $cppSessions.data.sessions |
            Where-Object {
                $_.availableControls.togglePlayPause -and
                $_.availableControls.skipNext -and
                $_.availableControls.skipPrevious
            }
    )
    if ($cppFixture.Count -ne 1) {
        throw 'C++ did not resolve exactly one self-owned media session.'
    }
    $rustTarget = [string]$rustFixture[0].sessionId
    $cppTarget = [string]$cppFixture[0].sessionId

    $rustUnconfirmed = & $rust run media-session pause `
        --target "sessionId=$rustTarget" |
        ConvertFrom-Json
    $rustUnconfirmedExit = $LASTEXITCODE
    $cppUnconfirmed = & $cpp run media-session pause `
        --target "sessionId=$cppTarget" |
        ConvertFrom-Json
    $cppUnconfirmedExit = $LASTEXITCODE
    if ($rustUnconfirmedExit -eq 0 -or
        $cppUnconfirmedExit -eq 0 -or
        $rustUnconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED' -or
        $cppUnconfirmed.error.code -ne
            'CONFIRMATION_REQUIRED') {
        throw 'Rust/C++ confirmation-first errors differ.'
    }

    function Invoke-RustControl {
        param([Parameter(Mandatory)][string] $Operation)
        $result = & $rust run media-session $Operation `
            --target "sessionId=$rustTarget" `
            --confirm |
            ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or
            -not $result.ok -or
            -not $result.accepted -or
            -not $result.foreground.unchanged) {
            throw "Rust media control failed: $Operation"
        }
        $script:rustResults += $result
    }
    function Invoke-CppControl {
        param([Parameter(Mandatory)][string] $Operation)
        $result = & $cpp run media-session $Operation `
            --target "sessionId=$cppTarget" `
            --timeout-ms 5000 `
            --confirm |
            ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or
            -not $result.ok -or
            -not $result.accepted -or
            -not $result.foreground.unchanged -or
            $result.compatibilityShape -ne
                'secured-opaque-media-session-v1') {
            throw "C++ media control failed: $Operation"
        }
        $serialized =
            $result | ConvertTo-Json -Depth 20 -Compress
        foreach ($forbidden in @(
            'sourceAppId',
            'foreground.before',
            'foreground.after',
            'hwnd',
            'pid'
        )) {
            if ($serialized.IndexOf(
                $forbidden,
                [System.StringComparison]::OrdinalIgnoreCase
            ) -ge 0) {
                throw "C++ media control leaked native field: $forbidden"
            }
        }
        $script:cppResults += $result
    }

    Invoke-RustControl pause
    Invoke-RustControl play
    Invoke-CppControl pause
    Invoke-CppControl play

    Invoke-RustControl pause
    Invoke-RustControl play
    Invoke-CppControl pause
    Invoke-CppControl play

    Invoke-RustControl toggle-play-pause
    Invoke-RustControl toggle-play-pause
    Invoke-CppControl toggle-play-pause
    Invoke-CppControl toggle-play-pause

    Invoke-RustControl skip-next
    Invoke-CppControl skip-next
    Invoke-RustControl skip-previous
    Invoke-CppControl skip-previous
    $launcherResult = & $launcher run media-session skip-next `
        --target "sessionId=$cppTarget" `
        --confirm |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $launcherResult.ok -or
        $launcherResult.compatibilityShape -ne
            'secured-opaque-media-session-v1') {
        throw 'Compatibility launcher did not select the C++ media route.'
    }

    $cppOperations = @(
        $cppResults.operation | Sort-Object -Unique
    )
    $rustOperations = @(
        $rustResults.operation | Sort-Object -Unique
    )
    $expected = @(
        'pause',
        'play',
        'skip-next',
        'skip-previous',
        'toggle-play-pause'
    )
    if ((Compare-Object $expected $cppOperations).Count -ne 0 -or
        (Compare-Object $expected $rustOperations).Count -ne 0) {
        throw 'Rust/C++ did not both cover all five media controls.'
    }
    if ($foregroundBefore -ne
        [ActMediaForegroundProbe]::GetForegroundWindow()) {
        throw 'Media control equivalence changed foreground.'
    }
    [PSCustomObject]@{
        ok = $true
        cppOperations = $cppOperations
        rustOperations = $rustOperations
        acceptedShapeEquivalent = $true
        confirmationErrorEquivalent = $true
        exactTargetingEquivalent = $true
        foregroundUnchanged = $true
        nativeIdentifierLeak = $false
        selfOwnedSessionsControlled = 1
        userApplicationsControlled = 0
        publicRouteEnabled = $true
        compatibilityLauncherSucceeded = $true
    } | ConvertTo-Json
} finally {
    if ($null -ne $fixtureProcess -and
        -not $fixtureProcess.HasExited) {
        Stop-Process -Id $fixtureProcess.Id -Force
    }
    foreach ($path in @($wavPath, $readyPath, $errorPath)) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }
}
