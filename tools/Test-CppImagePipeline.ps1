[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
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

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null

$pixelTest = Join-Path $outputRoot 'act-pixel-buffer-test.exe'
& $compiler @common `
    (Join-Path $sourceRoot 'tests\pixel_buffer_test.cpp') `
    (Join-Path $sourceRoot 'src\components\pixel_buffer.cpp') `
    -o $pixelTest
if ($LASTEXITCODE -ne 0) {
    throw 'Pixel buffer test build failed.'
}
$pixel = & $pixelTest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $pixel.ok -or
    $pixel.pixelFormat -ne 'rgba8' -or
    -not $pixel.paddingIgnored -or
    -not $pixel.boundsFailClosed) {
    throw 'Pixel buffer test failed.'
}

$pngTest = Join-Path $outputRoot 'act-png-encoder-test.exe'
& $compiler @common `
    -Wno-nonportable-include-path `
    "-I$cppWinRt" `
    (Join-Path $sourceRoot 'tests\png_encoder_test.cpp') `
    (Join-Path $sourceRoot 'src\platform\windows\png_encoder.cpp') `
    -lole32 `
    -lwindowscodecs `
    -o $pngTest
if ($LASTEXITCODE -ne 0) {
    throw 'PNG encoder test build failed.'
}
$png = & $pngTest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $png.ok -or
    -not $png.pngSignature -or
    $png.ihdrWidth -ne 2 -or
    $png.ihdrHeight -ne 2 -or
    -not $png.memoryOnly -or
    -not $png.boundsFailClosed) {
    throw 'PNG encoder test failed.'
}

[PSCustomObject]@{
    ok = $true
    pixelFormat = $pixel.pixelFormat
    paddingIgnored = $pixel.paddingIgnored
    pixelBoundsFailClosed = $pixel.boundsFailClosed
    pngSignature = $png.pngSignature
    pngIhdr = "$($png.ihdrWidth)x$($png.ihdrHeight)"
    pngBoundsFailClosed = $png.boundsFailClosed
    filesWritten = 0
} | ConvertTo-Json
