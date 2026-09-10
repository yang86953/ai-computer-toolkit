[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'ai-computer-toolkit-cpp.exe'
$null = Get-Content -LiteralPath (
    Join-Path $projectRoot (
        'contracts\v1\environment-capability-status.schema.json'
    )
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') |
    Out-Null
$rustDoctor = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- doctor | ConvertFrom-Json
$statuses = [ordered]@{}
foreach ($surface in @(
    'browser',
    'win32-control',
    'notepad',
    'desktop'
)) {
    $result = & $cpp status $surface | ConvertFrom-Json
    $expectedExecution =
        $surface -in @('browser', 'win32-control', 'desktop')
    if ($LASTEXITCODE -ne 0 -or
        -not $result.ok -or
        $result.data.surface -ne $surface -or
        -not $result.data.readOnly -or
        [bool]$result.data.cppExecutionEnabled -ne
            $expectedExecution -or
        [bool]$result.data.writesEnabled -ne
            $expectedExecution -or
        -not $result.data.foregroundUnchanged -or
        $result.data.nativeIdentifiersExposed -or
        $result.data.runtimePathExposed) {
        throw "Unsafe environment status: $surface"
    }
    $statuses[$surface] = $result.data
}

$rustBrowser = @(
    $rustDoctor.results |
        Where-Object { $_.app -eq 'browser' }
)[0]
$rustNotepad = @(
    $rustDoctor.results |
        Where-Object { $_.app -eq 'notepad' }
)[0]
$rustDesktop = @(
    $rustDoctor.results |
        Where-Object { $_.app -eq 'desktop' }
)[0]
if ($statuses.browser.runtimeDetected -ne
        [bool]$rustBrowser.available -or
    $statuses.notepad.runtimeDetected -ne
        (-not [string]::IsNullOrWhiteSpace(
            $rustNotepad.notepadPath)) -or
    $statuses.desktop.ffmpegRuntimeDetected -ne
        [bool]$rustDesktop.videoRecording.available -or
    -not $statuses.'win32-control'.runtimeDetected -or
    $statuses.'win32-control'.activeWriteProbes -ne 0 -or
    $statuses.'win32-control'.controlCount -ne
        ($statuses.'win32-control'.requiresConfirmationCount +
         $statuses.'win32-control'.permissionBlockedCount +
         $statuses.'win32-control'.indeterminateCount) -or
    -not $statuses.desktop.captureWorkerBundled -or
    -not $statuses.desktop.recordingWorkerBundled -or
    $statuses.desktop.recordingCppStatus -ne
        'available-confirmed-bounded-streaming') {
    throw 'C++ environment facts differ from the compatibility baseline.'
}

$serialized = $statuses |
    ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'browserPath',
    'notepadPath',
    'executablePath',
    'processId',
    'className',
    'hwnd'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "Environment status leaked a forbidden field: $term"
    }
}
if ($serialized -match '(?i)[A-Z]:\\\\') {
    throw 'Environment status leaked an absolute Windows path.'
}

foreach ($surface in @(
    'browser',
    'win32-control',
    'notepad',
    'desktop'
)) {
    $blocked = & $cpp run $surface not-migrated |
        ConvertFrom-Json
    if ($LASTEXITCODE -eq 0 -or
        $blocked.error.code -ne 'CAPABILITY_GAP') {
        throw "Environment observation opened execution: $surface"
    }
}

[PSCustomObject]@{
    ok = $true
    surfacesObserved = $statuses.Count
    runtimeFactsEquivalent = $true
    foregroundUnchanged = $true
    nativeIdentifierLeak = $false
    runtimePathLeak = $false
    certifiedExecutionOpened = @(
        'browser',
        'win32-control',
        'desktop'
    )
    userApplicationsStarted = $false
    writesPerformed = $false
} | ConvertTo-Json
