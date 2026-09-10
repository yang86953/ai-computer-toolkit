[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$null = Get-Content -LiteralPath (
    Join-Path $projectRoot 'contracts\v1\doctor-result.schema.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$cppDoctor = & $cpp doctor | ConvertFrom-Json
$rustDoctor = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- doctor | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $cppDoctor.ok -or
    $cppDoctor.policy -ne 'background-preferred' -or
    $cppDoctor.cppPolicy -ne
        'capability-first-no-silent-fallback' -or
    -not $cppDoctor.readOnly -or
    $cppDoctor.allCppExecutionAvailable -or
    -not $rustDoctor.ok) {
    throw 'Doctor aggregation command failed.'
}
$cppIds = @($cppDoctor.results.app | Sort-Object)
$rustIds = @($rustDoctor.results.app | Sort-Object)
if (($cppIds -join "`n") -ne ($rustIds -join "`n") -or
    $cppIds.Count -ne 9) {
    throw 'C++ doctor surface directory differs from Rust.'
}
foreach ($id in @('app', 'uia', 'window', 'process')) {
    $entry = @(
        $cppDoctor.results |
            Where-Object { $_.app -eq $id }
    )
    if ($entry.Count -ne 1 -or
        $entry[0].cppStatus -ne 'available-read-only' -or
        -not $entry[0].ok) {
        throw "Migrated read-only doctor failed: $id"
    }
}
$media = @(
    $cppDoctor.results |
        Where-Object { $_.app -eq 'media-session' }
)
if ($media.Count -ne 1 -or
    $media[0].cppStatus -ne
        'available-confirmed-opaque-control' -or
    -not $media[0].diagnostic.controlWorkerBundled -or
    -not $media[0].diagnostic.controlRequiresConfirmation -or
    $media[0].diagnostic.controlCppStatus -ne
        'available-confirmed') {
    throw 'Media doctor did not expose the isolated confirmed route.'
}
foreach ($id in @('notepad')) {
    $entry = @(
        $cppDoctor.results |
            Where-Object { $_.app -eq $id }
    )
    if ($entry.Count -ne 1 -or
        $entry[0].cppStatus -ne
        'runtime-observed-execution-rust-compatibility' -or
        $entry[0].diagnostic.cppExecutionEnabled -or
        $entry[0].diagnostic.writesEnabled -or
        -not $entry[0].diagnostic.foregroundUnchanged) {
        throw "Compatibility doctor misreported C++ execution: $id"
    }
}
$desktop = @(
    $cppDoctor.results |
        Where-Object { $_.app -eq 'desktop' }
)
if ($desktop.Count -ne 1 -or
    $desktop[0].cppStatus -ne
        'available-confirmed-capture' -or
    -not $desktop[0].diagnostic.cppExecutionEnabled -or
    -not $desktop[0].diagnostic.writesEnabled -or
    -not $desktop[0].diagnostic.recordingWorkerBundled -or
    $desktop[0].diagnostic.recordingCppStatus -ne
        'available-confirmed-bounded-streaming') {
    throw 'Desktop doctor did not expose the bounded recording route.'
}
$browser = @(
    $cppDoctor.results |
        Where-Object { $_.app -eq 'browser' }
)
if ($browser.Count -ne 1 -or
    $browser[0].cppStatus -ne
        'available-confirmed-isolated' -or
    -not $browser[0].diagnostic.cppExecutionEnabled -or
    -not $browser[0].diagnostic.writesEnabled -or
    -not $browser[0].diagnostic.foregroundUnchanged -or
    $browser[0].diagnostic.runtimePathExposed) {
    throw 'Browser doctor did not expose the isolated C++ route.'
}
$win32Control = @(
    $cppDoctor.results |
        Where-Object { $_.app -eq 'win32-control' }
)
if ($win32Control.Count -ne 1 -or
    $win32Control[0].cppStatus -ne
        'available-confirmed-opaque-target' -or
    -not $win32Control[0].diagnostic.readOnly -or
    -not $win32Control[0].diagnostic.cppExecutionEnabled -or
    -not $win32Control[0].diagnostic.writesEnabled -or
    -not $win32Control[0].diagnostic.foregroundUnchanged -or
    $win32Control[0].diagnostic.nativeIdentifiersExposed) {
    throw 'Standard Edit doctor did not expose the confirmed C++ route.'
}

$single = & $cpp doctor window | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    $single.results.Count -ne 1 -or
    $single.results[0].app -ne 'window') {
    throw 'Filtered doctor result failed.'
}
$unknown = & $cpp doctor not-an-app | ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $unknown.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Unknown doctor surface was accepted.'
}
$serialized = $cppDoctor | ConvertTo-Json -Depth 30 -Compress
foreach ($term in @(
    'hwnd'
    'processId'
    'browserPath'
    'notepadPath'
    'cargo run'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "C++ doctor leaked native/runtime detail: $term"
    }
}

[PSCustomObject]@{
    ok = $true
    surfaceCount = 9
    migratedReadDiagnostics = 6
    runtimeObservedCompatibilityDiagnostics = 1
    browserConfirmedCppRoute = $true
    mediaControlConfirmedCppRoute = $true
    standardEditConfirmedCppRoute = $true
    desktopRecordingConfirmedCppRoute = $true
    cargoRuntimeCalledByCpp = $false
    nativeIdentifierLeak = $false
    unknownSurfaceRefused = $true
} | ConvertTo-Json
$global:LASTEXITCODE = 0
