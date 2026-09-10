[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source
$fixture = Join-Path $outputRoot 'act-application-launch-fixture.exe'
$ready = Join-Path $outputRoot 'act-application-launch-fixture.ready'
$test = Join-Path $outputRoot 'act-application-launch-candidate-test.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    -municode `
    -mwindows `
    -DUNICODE `
    -D_UNICODE `
    -DWIN32_LEAN_AND_MEAN `
    -DNOMINMAX `
    -D_WIN32_WINNT=0x0A00 `
    (Join-Path $sourceRoot 'tests\application_launch_fixture_main.cpp') `
    -o $fixture
if ($LASTEXITCODE -ne 0) {
    throw 'Application launch fixture compilation failed.'
}

$sources = @(
    (Join-Path $sourceRoot 'tests\application_launch_candidate_test.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\modules\application_discovery_module.cpp')
    (Join-Path $sourceRoot 'src\modules\application_launch_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\application_launch_module.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\application_launch_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\installed_application_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\shell_application_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
    (Join-Path $sourceRoot 'src\systems\application_launch_command_system.cpp')
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
    -lshell32 `
    -lpropsys `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Application launch candidate compilation failed.'
}
& $test $fixture $ready
if ($LASTEXITCODE -ne 0) {
    throw 'Application launch candidate test failed.'
}
if (Test-Path -LiteralPath $ready) {
    throw 'Application launch fixture output was not cleaned.'
}
[PSCustomObject]@{
    ok = $true
    confirmationFirst = $true
    legacyPathLaunchPreservedByRust = $true
    exactOpaqueApplicationTargetRequired = $true
    staleTargetRefusedBeforeLaunch = $true
    selfOwnedShellLaunchSucceeded = $true
    foregroundUnchanged = $true
    nativeIdentityExposed = $false
    userApplicationsLaunched = 0
    publicRouteEnabled = $false
} | ConvertTo-Json
