[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$manifest = Get-Content -LiteralPath (
    Join-Path $projectRoot 'contracts\release\cpp-bundle-v1.json'
) -Raw | ConvertFrom-Json
if ($manifest.contractVersion -ne
    'act/cpp-release-bundle/v1') {
    throw 'C++ release bundle contract version is invalid.'
}

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') -Clean |
    Out-Null
$evidence = @()
foreach ($artifact in $manifest.artifacts) {
    $path = Join-Path $outputRoot $artifact.path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Release artifact is missing: $($artifact.path)"
    }
    $item = Get-Item -LiteralPath $path
    if ($item.Length -le 0) {
        throw "Release artifact is empty: $($artifact.path)"
    }
    $evidence += [PSCustomObject]@{
        path = $artifact.path
        role = $artifact.role
        bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
}
foreach ($entry in $manifest.forbiddenBundleEntries) {
    if (Test-Path -LiteralPath (Join-Path $outputRoot $entry)) {
        throw "Rust build entry leaked into C++ bundle: $entry"
    }
}
$main = Join-Path $outputRoot $manifest.mainExecutable
$version = & $main build-info | ConvertFrom-Json
$methods = & $main methods media-session |
    ConvertFrom-Json
$describe = & $main capabilities descriptor media-session play |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $version.ok -or
    $version.data.mainLanguage -ne 'C++23' -or
    -not $methods.ok -or
    -not $describe.ok -or
    -not $describe.data.descriptor.cppExecutionEnabled) {
    throw 'Built C++ release bundle did not start independently.'
}

[PSCustomObject]@{
    ok = $true
    artifactCount = $evidence.Count
    artifacts = $evidence
    cargoManifestBundled = $false
    rustSourceBundled = $false
    cargoRuntimeRequired = $false
    mainStarts = $true
    workersBundled = @(
        $evidence | Where-Object { $_.role -like '*worker*' }
    ).Count
    descriptorManifestBundled = $true
    rustCompatibilityRepositoryRetained = (
        Test-Path -LiteralPath (Join-Path $projectRoot 'Cargo.toml')
    )
} | ConvertTo-Json -Depth 5
