[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$manifest = Join-Path `
    $projectRoot 'contracts\compat\legacy-public-catalog-v1.json'
$catalog = Get-Content -LiteralPath $manifest -Raw |
    ConvertFrom-Json
$null = Get-Content -LiteralPath (
    Join-Path `
        $projectRoot `
        'contracts\v1\legacy-descriptor-compatibility.schema.json'
) -Raw | ConvertFrom-Json
if ($catalog.contractVersion -ne
        'act/legacy-public-catalog/v1' -or
    $catalog.apps.Count -ne 9 -or
    @($catalog.apps.operations | ForEach-Object { $_ }).Count -ne 21) {
    throw 'Legacy descriptor manifest is incomplete.'
}

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$rust = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- catalog | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $rust.ok) {
    throw 'Rust descriptor baseline is unavailable.'
}
$cppCatalog = & $cpp catalog | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $cppCatalog.ok -or
    ($cppCatalog | ConvertTo-Json -Depth 30 -Compress) -cne
    ($rust | ConvertTo-Json -Depth 30 -Compress)) {
    throw 'C++ full legacy catalog is not equivalent to Rust.'
}
$checkedApps = 0
$checkedOperations = 0
foreach ($rustApp in $rust.apps) {
    $cppFiltered = & $cpp catalog $rustApp.id |
        ConvertFrom-Json
    $expectedFiltered = [PSCustomObject][ordered]@{
        apps = @($rustApp)
        ok = $true
        policy = $rust.policy
    }
    if ($LASTEXITCODE -ne 0 -or
        ($cppFiltered | ConvertTo-Json -Depth 30 -Compress) -cne
        ($expectedFiltered | ConvertTo-Json -Depth 30 -Compress)) {
        throw "C++ filtered catalog mismatch: $($rustApp.id)"
    }
    $cppApp = & $cpp describe $rustApp.id | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $cppApp.ok -or
        ($cppApp | ConvertTo-Json -Depth 30 -Compress) -cne
        ([PSCustomObject][ordered]@{
            descriptor = $rustApp
            ok = $true
        } | ConvertTo-Json -Depth 30 -Compress)) {
        throw "C++ app descriptor failed: $($rustApp.id)"
    }
    $cppAppStatus = & $cpp capabilities descriptor $rustApp.id |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        $cppAppStatus.data.descriptor.cppDirectoryStatus -ne
        'compatibility-descriptor') {
        throw "C++ app status failed: $($rustApp.id)"
    }
    ++$checkedApps

    foreach ($rustOperation in $rustApp.operations) {
        $cppOperation = & $cpp describe `
            $rustApp.id $rustOperation.operation |
            ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or
            -not $cppOperation.ok -or
            ($cppOperation | ConvertTo-Json -Depth 30 -Compress) -cne
            ([PSCustomObject][ordered]@{
                descriptor = $rustOperation
                ok = $true
            } | ConvertTo-Json -Depth 30 -Compress)) {
            throw "C++ operation descriptor opened execution: $($rustOperation.id)"
        }
        $cppOperationStatus = & $cpp capabilities descriptor `
            $rustApp.id $rustOperation.operation |
            ConvertFrom-Json
        $cppScreenshot =
            $rustOperation.id -in @(
                'desktop.screenshot',
                'app.screenshot'
            )
        $cppTextCreate =
            $rustOperation.id -in @(
                'app.create',
                'notepad.open-and-write-text'
            )
        $cppStandardEdit =
            $rustOperation.id -in @(
                'app.apply',
                'win32-control.set-text',
                'desktop.type-text'
            )
        $cppWindowClose =
            $rustOperation.id -eq 'app.close'
        $cppBrowser =
            $rustOperation.id -eq 'browser.screenshot'
        $cppRecording =
            $rustOperation.id -in @(
                'desktop.record',
                'app.record'
            )
        $cppMediaControl =
            $rustOperation.id -like 'media-session.*'
        $cppExecution =
            $cppScreenshot -or $cppTextCreate -or
            $cppStandardEdit -or $cppWindowClose -or
            $cppBrowser -or $cppRecording -or
            $cppMediaControl
        $expectedStatus = if ($cppScreenshot -or
            $rustOperation.id -eq 'app.create') {
            'available-confirmed-opaque-target'
        } elseif ($cppStandardEdit) {
            'available-confirmed-opaque-target'
        } elseif ($cppWindowClose) {
            'available-confirmed-opaque-target'
        } elseif ($cppBrowser) {
            'available-confirmed-isolated'
        } elseif ($cppRecording -or $cppMediaControl) {
            'available-confirmed-opaque-target'
        } elseif ($rustOperation.id -eq
            'notepad.open-and-write-text') {
            'available-confirmed'
        } else {
            'rust-compatibility-only'
        }
        if ($LASTEXITCODE -ne 0 -or
            $cppOperationStatus.data.descriptor.cppStatus -ne
            $expectedStatus -or
            [bool]$cppOperationStatus.data.descriptor.cppExecutionEnabled -ne
            $cppExecution) {
            throw "Operation status mismatch: $($rustOperation.id)"
        }
        ++$checkedOperations
    }
}

$unknownApp = & $cpp describe not-an-app | ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $unknownApp.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Unknown app descriptor was accepted.'
}
$unknownCatalog = & $cpp catalog not-an-app | ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $unknownCatalog.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Unknown catalog application was accepted.'
}
$unknownOperation = & $cpp describe window not-an-operation |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $unknownOperation.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Unknown operation descriptor was accepted.'
}

$missingFixture = Join-Path `
    $projectRoot 'build\cpp-main\descriptor-missing-manifest'
$resolvedBuild = [System.IO.Path]::GetFullPath(
    (Join-Path $projectRoot 'build'))
$resolvedFixture = [System.IO.Path]::GetFullPath($missingFixture)
if (-not $resolvedFixture.StartsWith(
    $resolvedBuild + [System.IO.Path]::DirectorySeparatorChar,
    [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Descriptor fixture escaped the project build directory.'
}
if (Test-Path -LiteralPath $missingFixture) {
    Remove-Item -LiteralPath $missingFixture -Recurse -Force
}
New-Item -ItemType Directory -Path $missingFixture |
    Out-Null
$fixtureExecutable = Join-Path `
    $missingFixture 'ai-computer-toolkit-cpp.exe'
Copy-Item -LiteralPath $cpp -Destination $fixtureExecutable
$missing = & $fixtureExecutable describe media-session play |
    ConvertFrom-Json
$missingCatalog = & $fixtureExecutable catalog |
    ConvertFrom-Json
Remove-Item -LiteralPath $missingFixture -Recurse -Force
if ($LASTEXITCODE -eq 0 -or
    $missing.error.code -ne 'OPERATION_FAILED' -or
    $missingCatalog.error.code -ne 'OPERATION_FAILED' -or
    (Test-Path -LiteralPath $missingFixture)) {
    throw 'Missing descriptor companion did not fail closed and clean.'
}
$cppCapabilities = & $cpp capabilities app |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $cppCapabilities.ok -or
    $cppCapabilities.data.supportLevel -ne
        'L2-confirmed-background') {
    throw 'C++ migration capabilities were not separated from catalog.'
}

[PSCustomObject]@{
    ok = $true
    fullCatalogEquivalent = $true
    filteredCatalogsEquivalent = $checkedApps
    appDescriptorsEquivalent = $checkedApps
    operationDescriptorsEquivalent = $checkedOperations
    cargoRuntimeRequiredByCpp = $false
    cppExecutionRoutes = 16
    confirmedCppOperations = @(
        'desktop.screenshot',
        'app.screenshot',
        'app.create',
        'notepad.open-and-write-text',
        'app.apply',
        'win32-control.set-text',
        'desktop.type-text',
        'app.close',
        'browser.screenshot'
        'desktop.record'
        'app.record'
        'media-session.toggle-play-pause'
        'media-session.play'
        'media-session.pause'
        'media-session.skip-next'
        'media-session.skip-previous'
    )
    missingCompanionFailedClosed = $true
    unknownDescriptorsRefused = $true
    migrationCapabilitiesSeparated = $true
    legacyDescribeJsonEquivalent = $true
} | ConvertTo-Json
