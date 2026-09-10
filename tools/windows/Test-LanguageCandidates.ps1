[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'spikes\language-evaluation'
$buildRoot = Join-Path $projectRoot 'build\language-evaluation'
New-Item -ItemType Directory -Force -Path $buildRoot | Out-Null

function Assert-ProbeResult {
    param(
        [Parameter(Mandatory)]
        [string] $ExpectedLanguage,
        [Parameter(Mandatory)]
        [string] $Executable
    )

    $raw = & $Executable
    if ($LASTEXITCODE -ne 0) {
        throw "$ExpectedLanguage probe exited with $LASTEXITCODE."
    }
    $result = $raw | ConvertFrom-Json
    if (
        $result.ok -ne $true -or
        $result.contractVersion -ne 'act/language-probe/v1' -or
        $result.implementationLanguage -ne $ExpectedLanguage -or
        $result.platform -ne 'windows' -or
        $result.observations.processCount -lt 1 -or
        $result.observations.topLevelWindowCount -lt 1 -or
        $result.observations.uiaClientInitialized -ne $true -or
        $result.observations.foregroundUnchanged -ne $true -or
        $result.safety.readOnly -ne $true -or
        $result.safety.inputSent -ne $false -or
        $result.safety.windowActivated -ne $false
    ) {
        throw "$ExpectedLanguage probe violated the shared result contract: $raw"
    }
    return $result
}

$cppExe = Join-Path $buildRoot 'probe-cpp.exe'
& clang++ -std=c++20 -Wall -Wextra -Werror `
    (Join-Path $sourceRoot 'cpp\main.cpp') `
    -o $cppExe -lole32 -loleaut32 -luuid -luser32 -lkernel32
if ($LASTEXITCODE -ne 0) {
    throw "C++ probe build failed with $LASTEXITCODE."
}

$vExe = Join-Path $buildRoot 'probe-vlang.exe'
$vTemporaryRoot = Join-Path (
    [System.IO.Path]::GetTempPath()
) ("act-v-probe-" + [System.Guid]::NewGuid().ToString('N'))
$vTemporaryExe = Join-Path $vTemporaryRoot 'probe-vlang.exe'
New-Item -ItemType Directory -Path $vTemporaryRoot | Out-Null
try {
    # V 0.5.2 forwards the output path through the active Windows code page.
    # Build in an ASCII-only temporary directory, then move the generated
    # artifact into this repository's Unicode path.
    & v -o $vTemporaryExe `
        (Join-Path $sourceRoot 'vlang\main.v')
    if ($LASTEXITCODE -ne 0) {
        throw "Vlang probe build failed with $LASTEXITCODE."
    }
    Move-Item -LiteralPath $vTemporaryExe -Destination $vExe -Force
} finally {
    $resolvedTemporaryRoot = [System.IO.Path]::GetFullPath($vTemporaryRoot)
    $resolvedSystemTemp = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::GetTempPath()
    )
    if (
        (Test-Path -LiteralPath $resolvedTemporaryRoot) -and
        $resolvedTemporaryRoot.StartsWith(
            $resolvedSystemTemp,
            [System.StringComparison]::OrdinalIgnoreCase
        )
    ) {
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}

$zigExe = Join-Path $buildRoot 'probe-zig.exe'
& zig build-exe (Join-Path $sourceRoot 'zig\main.zig') `
    "-femit-bin=$zigExe" -lole32 -loleaut32 -luuid -luser32 -lkernel32
if ($LASTEXITCODE -ne 0) {
    throw "Zig probe build failed with $LASTEXITCODE."
}

$results = @(
    Assert-ProbeResult -ExpectedLanguage 'cpp' -Executable $cppExe
    Assert-ProbeResult -ExpectedLanguage 'vlang' -Executable $vExe
    Assert-ProbeResult -ExpectedLanguage 'zig' -Executable $zigExe
)

$results | ConvertTo-Json -Depth 6
