[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $BundlePath,
    [Parameter(Mandatory)]
    [string] $Destination,
    [switch] $ConfirmInstall
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CppBundleSupport.ps1')
if (-not $ConfirmInstall) {
    throw 'CONFIRMATION_REQUIRED: installation requires -ConfirmInstall.'
}
$bundle = [System.IO.Path]::GetFullPath($BundlePath)
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$parent = Split-Path -Parent $destinationPath
if ((Test-CppPathWithin -Candidate $bundle -Root $destinationPath) -or
    (Test-CppPathWithin -Candidate $destinationPath -Root $bundle) -or
    [string]::IsNullOrWhiteSpace($parent) -or
    $destinationPath.Length -lt 8) {
    throw 'Installation source or destination is unsafe.'
}
if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    throw 'Installation destination parent must already exist.'
}

$manifest = Test-CppBundle -Path $bundle
$backup = "$destinationPath.rollback"
$stage = "$destinationPath.install.$([guid]::NewGuid().ToString('N'))"
if (Test-Path -LiteralPath $backup) {
    throw 'A rollback version already exists; resolve it before upgrading.'
}
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Copy-Item -LiteralPath (
        Join-Path $bundle 'bundle-manifest.json'
    ) -Destination $stage
    foreach ($artifact in $manifest.artifacts) {
        Copy-Item -LiteralPath (
            Join-Path $bundle $artifact.path
        ) -Destination $stage
    }
    $null = Test-CppBundle -Path $stage
    if (Test-Path -LiteralPath $destinationPath) {
        Move-Item -LiteralPath $destinationPath -Destination $backup
    }
    try {
        Move-Item -LiteralPath $stage -Destination $destinationPath
    } catch {
        if (Test-Path -LiteralPath $backup) {
            Move-Item -LiteralPath $backup -Destination $destinationPath
        }
        throw
    }
} finally {
    if (Test-Path -LiteralPath $stage) {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
}

[PSCustomObject]@{
    ok = $true
    destination = $destinationPath
    installedVersion = $manifest.bundleVersion
    previousVersionRetained = (Test-Path -LiteralPath $backup)
    rollbackPath = $backup
} | ConvertTo-Json
