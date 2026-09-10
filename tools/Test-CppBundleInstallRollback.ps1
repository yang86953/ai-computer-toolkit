[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$buildRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $projectRoot 'build')
)
. (Join-Path $PSScriptRoot 'CppBundleSupport.ps1')

$policy = Get-Content -LiteralPath (
    Join-Path $projectRoot (
        'contracts\release\cpp-install-rollback-v1.json'
    )
) -Raw | ConvertFrom-Json
if ($policy.contractVersion -ne
    'act/cpp-install-rollback/v1' -or
    $policy.integrity.authenticityGuaranteed -or
    $policy.migrationBoundary.systemInstallationAuthorized -or
    $policy.migrationBoundary.skillEntrypointSwitchAuthorized -or
    $policy.migrationBoundary.rustCompatibilityDeletionAuthorized) {
    throw 'Install and rollback policy contract is invalid.'
}

$fixtureRoot = Join-Path $buildRoot (
    'install-rollback-fixtures\' +
    [guid]::NewGuid().ToString('N')
)
if (-not (Test-CppPathWithin -Candidate $fixtureRoot -Root $buildRoot) -or
    $fixtureRoot -eq $buildRoot) {
    throw 'Fixture path escaped the project build directory.'
}
$bundleV1 = Join-Path $fixtureRoot 'bundle-v1'
$bundleV2 = Join-Path $fixtureRoot 'bundle-v2'
$installParent = Join-Path $fixtureRoot 'installed'
$destination = Join-Path $installParent 'active'
$tamperBundle = Join-Path $fixtureRoot 'tampered-bundle'
$tamperDestination = Join-Path $installParent 'tampered-active'

$confirmationRejected = $false
$rollbackConfirmationRejected = $false
$tamperRejected = $false
$tamperedRollbackRejected = $false
$firstVersion = $null
$secondVersion = $null
$rolledBackVersion = $null
$toggledVersion = $null
$mainStarts = $false
$fixtureRemoved = $false

try {
    & (Join-Path $PSScriptRoot 'New-CppReleaseBundle.ps1') `
        -OutputPath $bundleV1 `
        -BundleVersion 'fixture-v1' `
        -CleanBuild |
        Out-Null
    & (Join-Path $PSScriptRoot 'New-CppReleaseBundle.ps1') `
        -OutputPath $bundleV2 `
        -BundleVersion 'fixture-v2' |
        Out-Null
    New-Item -ItemType Directory -Path $installParent -Force |
        Out-Null

    try {
        & (Join-Path $PSScriptRoot 'Install-CppBundle.ps1') `
            -BundlePath $bundleV1 `
            -Destination $destination |
            Out-Null
    } catch {
        $confirmationRejected =
            $_.Exception.Message -like 'CONFIRMATION_REQUIRED:*'
    }
    if (-not $confirmationRejected -or
        (Test-Path -LiteralPath $destination)) {
        throw 'Unconfirmed installation was not rejected safely.'
    }

    & (Join-Path $PSScriptRoot 'Install-CppBundle.ps1') `
        -BundlePath $bundleV1 `
        -Destination $destination `
        -ConfirmInstall |
        Out-Null
    $first = Test-CppBundle -Path $destination
    $firstVersion = $first.bundleVersion
    $main = Join-Path $destination $first.mainExecutable
    $versionResult = & $main build-info | ConvertFrom-Json
    $mainStarts = $LASTEXITCODE -eq 0 -and
        $versionResult.ok -and
        $versionResult.data.mainLanguage -eq 'C++23'
    if ($firstVersion -ne 'fixture-v1' -or -not $mainStarts) {
        throw 'The first installed C++ bundle did not start.'
    }

    & (Join-Path $PSScriptRoot 'Install-CppBundle.ps1') `
        -BundlePath $bundleV2 `
        -Destination $destination `
        -ConfirmInstall |
        Out-Null
    $second = Test-CppBundle -Path $destination
    $retainedV1 = Test-CppBundle -Path "$destination.rollback"
    $secondVersion = $second.bundleVersion
    if ($secondVersion -ne 'fixture-v2' -or
        $retainedV1.bundleVersion -ne 'fixture-v1') {
        throw 'Upgrade did not retain the exact previous version.'
    }

    try {
        & (Join-Path $PSScriptRoot 'Restore-CppBundle.ps1') `
            -Destination $destination |
            Out-Null
    } catch {
        $rollbackConfirmationRejected =
            $_.Exception.Message -like 'CONFIRMATION_REQUIRED:*'
    }
    if (-not $rollbackConfirmationRejected -or
        (Test-CppBundle -Path $destination).bundleVersion -ne
        'fixture-v2') {
        throw 'Unconfirmed rollback changed the active version.'
    }

    & (Join-Path $PSScriptRoot 'Restore-CppBundle.ps1') `
        -Destination $destination `
        -ConfirmRollback |
        Out-Null
    $rolledBackVersion = (
        Test-CppBundle -Path $destination
    ).bundleVersion
    if ($rolledBackVersion -ne 'fixture-v1' -or
        (Test-CppBundle -Path "$destination.rollback").bundleVersion -ne
        'fixture-v2') {
        throw 'Rollback did not restore the retained version.'
    }

    & (Join-Path $PSScriptRoot 'Restore-CppBundle.ps1') `
        -Destination $destination `
        -ConfirmRollback |
        Out-Null
    $toggledVersion = (
        Test-CppBundle -Path $destination
    ).bundleVersion
    if ($toggledVersion -ne 'fixture-v2' -or
        (Test-CppBundle -Path "$destination.rollback").bundleVersion -ne
        'fixture-v1') {
        throw 'The rollback directory swap was not reversible.'
    }

    New-Item -ItemType Directory -Path $tamperBundle |
        Out-Null
    $v1Manifest = Test-CppBundle -Path $bundleV1
    Copy-Item -LiteralPath (
        Join-Path $bundleV1 'bundle-manifest.json'
    ) -Destination $tamperBundle
    foreach ($artifact in $v1Manifest.artifacts) {
        Copy-Item -LiteralPath (
            Join-Path $bundleV1 $artifact.path
        ) -Destination $tamperBundle
    }
    Add-Content -LiteralPath (
        Join-Path $tamperBundle 'legacy-public-catalog-v1.json'
    ) -Value "`n "
    try {
        & (Join-Path $PSScriptRoot 'Install-CppBundle.ps1') `
            -BundlePath $tamperBundle `
            -Destination $tamperDestination `
            -ConfirmInstall |
            Out-Null
    } catch {
        $tamperRejected =
            $_.Exception.Message -like (
                'Bundle artifact verification failed:*'
            )
    }
    if (-not $tamperRejected -or
        (Test-Path -LiteralPath $tamperDestination)) {
        throw 'A tampered bundle was not rejected before installation.'
    }

    Add-Content -LiteralPath (
        Join-Path "$destination.rollback" (
            'legacy-public-catalog-v1.json'
        )
    ) -Value "`n "
    try {
        & (Join-Path $PSScriptRoot 'Restore-CppBundle.ps1') `
            -Destination $destination `
            -ConfirmRollback |
            Out-Null
    } catch {
        $tamperedRollbackRejected =
            $_.Exception.Message -like (
                'Bundle artifact verification failed:*'
            )
    }
    if (-not $tamperedRollbackRejected -or
        (Test-CppBundle -Path $destination).bundleVersion -ne
        'fixture-v2') {
        throw 'Tampered rollback content changed the active version.'
    }
} finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        if (-not (Test-CppPathWithin `
            -Candidate $fixtureRoot `
            -Root $buildRoot) -or
            $fixtureRoot -eq $buildRoot) {
            throw 'Refusing unsafe fixture cleanup.'
        }
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
    }
    $fixtureRemoved = -not (Test-Path -LiteralPath $fixtureRoot)
}

if (-not $fixtureRemoved) {
    throw 'Install and rollback fixture cleanup was incomplete.'
}

[PSCustomObject]@{
    ok = $true
    confirmationRejected = $confirmationRejected
    rollbackConfirmationRejected =
        $rollbackConfirmationRejected
    tamperRejected = $tamperRejected
    tamperedRollbackRejected = $tamperedRollbackRejected
    installedVersion = $firstVersion
    upgradedVersion = $secondVersion
    rolledBackVersion = $rolledBackVersion
    toggledVersion = $toggledVersion
    mainStarts = $mainStarts
    exactPreviousVersionRetained = $true
    directorySwapWithoutDeletion = $true
    fixtureRootRemoved = $fixtureRemoved
    systemInstallTouched = $false
    skillEntrypointChanged = $false
    rustCompatibilityDeleted = $false
} | ConvertTo-Json
