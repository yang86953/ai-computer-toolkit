[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source
$test = Join-Path $outputRoot 'act-media-control-candidate-test.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null

$sources = @(
    (Join-Path $sourceRoot 'tests\media_control_candidate_test.cpp')
    (Join-Path $sourceRoot 'src\components\cancellation.cpp')
    (Join-Path $sourceRoot 'src\components\companion_file.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\worker_process.cpp')
    (Join-Path $sourceRoot 'src\modules\media_control_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\media_session_module.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\media_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
    (Join-Path $sourceRoot 'src\systems\media_command_system.cpp')
)
& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    -DUNICODE `
    -D_UNICODE `
    -DWIN32_LEAN_AND_MEAN `
    -DNOMINMAX `
    -D_WIN32_WINNT=0x0A00 `
    "-I$(Join-Path $sourceRoot 'src')" `
    @sources `
    -lole32 `
    -loleaut32 `
    -luuid `
    -luser32 `
    -ladvapi32 `
    -ldwmapi `
    -lruntimeobject `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Media control candidate test compilation failed.'
}
& $test
if ($LASTEXITCODE -ne 0) {
    throw 'Media control candidate policy test failed.'
}

$observationSource = Get-Content -LiteralPath (
    Join-Path $sourceRoot 'src\worker\media_observation_worker_main.cpp'
) -Raw
foreach ($method in @(
    'TryPlayAsync',
    'TryPauseAsync',
    'TryTogglePlayPauseAsync',
    'TrySkipNextAsync',
    'TrySkipPreviousAsync'
)) {
    if ($observationSource.Contains($method)) {
        throw "Read-only media worker contains write method: $method"
    }
}

[PSCustomObject]@{
    ok = $true
    confirmationFirst = $true
    exactOpaqueTargetRequired = $true
    legacyTargetPreservedByLauncher = $true
    staleTargetRefusedBeforeWrite = $true
    observationWorkerWriteMethods = 0
    publicRouteEnabled = $true
    liveMutationPerformed = $false
} | ConvertTo-Json
