[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source
$test = Join-Path $outputRoot 'act-foreground-input-candidate-test.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$sources = @(
    (Join-Path $sourceRoot 'tests\foreground_input_candidate_test.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\key_chord.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\modules\foreground_input_module.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\foreground_input_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
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
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Foreground input candidate compilation failed.'
}
& $test
if ($LASTEXITCODE -ne 0) {
    throw 'Foreground input candidate policy test failed.'
}
[PSCustomObject]@{
    ok = $true
    confirmationFirst = $true
    foregroundConsentSecond = $true
    exactOpaqueTargetRequired = $true
    certifiedKeyAllowlist = $true
    staleTargetRefusedBeforeActivation = $true
    liveForegroundChanged = $false
    inputEventsDispatched = 0
    publicRouteEnabled = $false
} | ConvertTo-Json
