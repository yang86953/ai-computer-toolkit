[CmdletBinding()]
param(
    [string] $OutputPath = "",
    [string] $BundleVersion = "0.1.0-migration",
    [switch] $CleanBuild
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CppBundleSupport.ps1')
$projectRoot = Split-Path -Parent $PSScriptRoot
$buildRoot = Join-Path $projectRoot 'build'
$sourceRoot = Join-Path $buildRoot 'cpp-main'
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $buildRoot 'release\cpp-bundle-v1'
}
$resolvedBuild = [System.IO.Path]::GetFullPath($buildRoot)
$resolvedOutput = [System.IO.Path]::GetFullPath($OutputPath)
$resolvedSource = [System.IO.Path]::GetFullPath($sourceRoot)
if (-not (Test-CppBundleVersion $BundleVersion)) {
    throw 'Bundle version must use 1-64 safe ASCII version characters.'
}
if (-not $resolvedOutput.StartsWith(
    $resolvedBuild + [System.IO.Path]::DirectorySeparatorChar,
    [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Release output must remain inside the project build directory.'
}
if ($resolvedOutput -eq $resolvedSource -or
    $resolvedOutput.StartsWith(
        $resolvedSource + [System.IO.Path]::DirectorySeparatorChar,
        [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Release output must not overlap the compiler output directory.'
}

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') -Clean:$CleanBuild |
    Out-Null
$contract = Get-Content -LiteralPath (
    Join-Path $projectRoot 'contracts\release\cpp-bundle-v1.json'
) -Raw | ConvertFrom-Json
if ($contract.contractVersion -ne
    'act/cpp-release-bundle/v1') {
    throw 'Release bundle contract is invalid.'
}
if (Test-Path -LiteralPath $resolvedOutput) {
    Remove-Item -LiteralPath $resolvedOutput -Recurse -Force
}
New-Item -ItemType Directory -Path $resolvedOutput -Force |
    Out-Null

$artifacts = @()
foreach ($entry in $contract.artifacts) {
    $source = Join-Path $sourceRoot $entry.path
    $destination = Join-Path $resolvedOutput $entry.path
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Release source artifact is missing: $($entry.path)"
    }
    Copy-Item -LiteralPath $source -Destination $destination
    $item = Get-Item -LiteralPath $destination
    $artifacts += [ordered]@{
        path = $entry.path
        role = $entry.role
        bytes = $item.Length
        sha256 = (
            Get-FileHash -LiteralPath $destination -Algorithm SHA256
        ).Hash
    }
}
$manifest = [ordered]@{
    contractVersion = 'act/installable-cpp-bundle/v1'
    bundleVersion = $BundleVersion
    product = 'ai-computer-toolkit'
    mainExecutable = $contract.mainExecutable
    artifacts = $artifacts
}
$manifestPath = Join-Path $resolvedOutput 'bundle-manifest.json'
$manifest | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath $manifestPath -Encoding utf8
$null = Test-CppBundle -Path $resolvedOutput

[PSCustomObject]@{
    ok = $true
    bundlePath = $resolvedOutput
    bundleVersion = $BundleVersion
    artifactCount = $artifacts.Count
    manifestPath = $manifestPath
    cargoRuntimeRequired = $false
} | ConvertTo-Json
