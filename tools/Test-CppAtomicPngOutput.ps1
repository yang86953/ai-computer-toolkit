[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$fixture = Join-Path (
    Join-Path $outputRoot 'atomic-png-fixtures'
) ([guid]::NewGuid().ToString('N'))
$test = Join-Path $outputRoot 'act-png-file-output-test.exe'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source

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

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
New-Item -ItemType Directory -Path (
    Split-Path -Parent $fixture
) -Force | Out-Null

& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    -Wno-nonportable-include-path `
    -DUNICODE `
    -D_UNICODE `
    -DWIN32_LEAN_AND_MEAN `
    -DNOMINMAX `
    "-I$cppWinRt" `
    "-I$(Join-Path $sourceRoot 'src')" `
    (Join-Path $sourceRoot 'tests\png_file_output_test.cpp') `
    (Join-Path $sourceRoot 'src\platform\windows\png_encoder.cpp') `
    (Join-Path $sourceRoot 'src\platform\windows\png_file_output.cpp') `
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp') `
    -lole32 `
    -lwindowscodecs `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Atomic PNG output test build failed.'
}

$result = & $test $fixture | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $result.ok -or
    -not $result.initialWrite -or
    -not $result.unconfirmedOverwriteRefused -or
    -not $result.confirmedReplace -or
    $result.temporaryFilesRemaining -ne 0 -or
    -not $result.normalizedPath) {
    throw 'Atomic PNG output test failed.'
}
if (Test-Path -LiteralPath $fixture) {
    throw 'Atomic PNG fixture directory was not cleaned.'
}

[PSCustomObject]@{
    ok = $true
    outputScope = 'project-build-fixture-only'
    initialWrite = $true
    unconfirmedOverwriteRefused = $true
    confirmedReplace = $true
    temporaryFilesRemaining = 0
    fixtureCleaned = $true
    userFilesTouched = 0
} | ConvertTo-Json
