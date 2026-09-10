[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$test = Join-Path $outputRoot 'act-media-worker-process-test.exe'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    "-I$(Join-Path $sourceRoot 'src')" `
    (Join-Path $sourceRoot 'tests\media_worker_process_test.cpp') `
    (Join-Path $sourceRoot 'src\components\cancellation.cpp') `
    (Join-Path $sourceRoot 'src\components\json.cpp') `
    (Join-Path $sourceRoot 'src\components\worker_process.cpp') `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Media worker lifecycle test build failed.'
}

$lifecycle = & $test | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $lifecycle.ok -or
    -not $lifecycle.timeoutJobTerminated -or
    -not $lifecycle.normalReadRecovered -or
    -not $lifecycle.foregroundUnchanged -or
    -not $lifecycle.metadataRead -or
    $lifecycle.writeMethodsCalled -or
    -not $lifecycle.staleTargetRefused) {
    throw 'Media worker lifecycle test failed.'
}

$cppResult = & $cpp sessions media-session `
    --max-items 128 --timeout-ms 5000 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $cppResult.ok -or
    $cppResult.data.capability -ne 'media.session.discover@1' -or
    -not $cppResult.data.readOnly -or
    $cppResult.data.executionDomain -ne 'isolated-worker' -or
    -not $cppResult.data.foregroundUnchanged -or
    $cppResult.data.count -ne $cppResult.data.sessions.Count) {
    throw 'C++ media-session public contract failed.'
}

$rustResult = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- sessions media-session --max-items 128 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $rustResult.ok -or
    -not $rustResult.readOnly -or
    -not $rustResult.foreground.unchanged) {
    throw 'Rust media-session compatibility read failed.'
}
if ($cppResult.data.total -ne $rustResult.total) {
    throw 'Rust/C++ media-session totals differ.'
}
$catalog = & $cpp capabilities media-session | ConvertFrom-Json
$discoveryCapability = @(
    $catalog.data.capabilities |
        Where-Object { $_.id -eq 'media.session.discover@1' }
)
$stateCapability = @(
    $catalog.data.capabilities |
        Where-Object { $_.id -eq 'media.playback.state.read@1' }
)
$controlCapability = @(
    $catalog.data.capabilities |
        Where-Object { $_.id -eq 'media.playback.control@1' }
)
if ($discoveryCapability.Count -ne 1 -or
    $discoveryCapability[0].status -ne 'available' -or
    $stateCapability.Count -ne 1 -or
    $stateCapability[0].status -ne 'available' -or
    $controlCapability.Count -ne 1 -or
    $controlCapability[0].status -ne 'available-confirmed' -or
    -not $controlCapability[0].requiresConfirmation) {
    throw 'Media catalog omitted the confirmed isolated control route.'
}

$stale = & $cpp inspect media-session `
    --target sessionId=s2:m:0000000000000000 `
    --timeout-ms 5000 |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ media inspect accepted a stale exact target.'
}

$forbiddenWorkerMethods = @(
    'TryPlayAsync'
    'TryPauseAsync'
    'TryTogglePlayPauseAsync'
    'TrySkipNextAsync'
    'TrySkipPreviousAsync'
    'SetForegroundWindow'
    'SendInput'
)
$workerSource = Join-Path `
    $sourceRoot 'src\worker\media_observation_worker_main.cpp'
foreach ($term in $forbiddenWorkerMethods) {
    if (Select-String -LiteralPath $workerSource `
        -Pattern $term -SimpleMatch) {
        throw "Media observation worker contains forbidden method: $term"
    }
}

$serialized = $cppResult | ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'sourceAppId'
    'foreground.before'
    'foreground.after'
    'native'
    'hwnd'
    'pid'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "C++ media result leaked provider/native field: $term"
    }
}

[PSCustomObject]@{
    ok = $true
    cppTotal = $cppResult.data.total
    rustTotal = $rustResult.total
    totalsEquivalent = $true
    timeoutJobTerminated = $true
    staleTargetRefused = $true
    foregroundUnchanged = $true
    writeMethodsCalled = $false
    nativeIdentifierLeak = $false
    readCapabilitiesAvailable = $true
    controlCapabilityMigrated = $true
} | ConvertTo-Json
