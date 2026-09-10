[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Destination,
    [switch] $ConfirmRollback
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CppBundleSupport.ps1')
if (-not $ConfirmRollback) {
    throw 'CONFIRMATION_REQUIRED: rollback requires -ConfirmRollback.'
}
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$backup = "$destinationPath.rollback"
$parent = Split-Path -Parent $destinationPath
if ([string]::IsNullOrWhiteSpace($parent) -or
    $destinationPath.Length -lt 8 -or
    -not (Test-Path -LiteralPath $destinationPath -PathType Container) -or
    -not (Test-Path -LiteralPath $backup -PathType Container)) {
    throw 'Active or rollback bundle is unavailable.'
}
$activeBefore = Test-CppBundle -Path $destinationPath
$retainedBefore = Test-CppBundle -Path $backup
$swap = "$destinationPath.swap.$([guid]::NewGuid().ToString('N'))"
Move-Item -LiteralPath $destinationPath -Destination $swap
try {
    Move-Item -LiteralPath $backup -Destination $destinationPath
    Move-Item -LiteralPath $swap -Destination $backup
    $active = Test-CppBundle -Path $destinationPath
    $retained = Test-CppBundle -Path $backup
} catch {
    $destinationExists = Test-Path -LiteralPath $destinationPath
    $backupExists = Test-Path -LiteralPath $backup
    $swapExists = Test-Path -LiteralPath $swap
    if ($destinationExists -and $backupExists -and
        -not $swapExists) {
        $recovery = "$destinationPath.recover.$(
            [guid]::NewGuid().ToString('N')
        )"
        Move-Item -LiteralPath $backup -Destination $recovery
        Move-Item -LiteralPath $destinationPath -Destination $backup
        Move-Item -LiteralPath $recovery -Destination $destinationPath
    } elseif ($destinationExists -and -not $backupExists -and
        $swapExists) {
        Move-Item -LiteralPath $destinationPath -Destination $backup
        Move-Item -LiteralPath $swap -Destination $destinationPath
    } elseif (-not $destinationExists -and $backupExists -and
        $swapExists) {
        Move-Item -LiteralPath $swap -Destination $destinationPath
    }
    throw
}

[PSCustomObject]@{
    ok = $true
    destination = $destinationPath
    activeVersion = $active.bundleVersion
    retainedVersion = $retained.bundleVersion
    previousActiveVersion = $activeBefore.bundleVersion
    previousRetainedVersion = $retainedBefore.bundleVersion
    reversible = $true
} | ConvertTo-Json
