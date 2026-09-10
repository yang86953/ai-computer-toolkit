[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$test = Join-Path `
    $outputRoot 'act-standard-edit-module-test.exe'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') |
    Out-Null
$sources = @(
    (Join-Path $sourceRoot 'tests\standard_edit_module_test.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\static_permission_assessment.cpp')
    (Join-Path $sourceRoot 'src\modules\standard_edit_module.cpp')
    (Join-Path $sourceRoot 'src\modules\standard_edit_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\standard_edit_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
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
    -lruntimeobject `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Standard Edit candidate test build failed.'
}

$result = & $test | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $result.ok -or
    -not $result.selfOwnedFixtureOnly -or
    -not $result.confirmationFirst -or
    -not $result.staleTargetRefused -or
    -not $result.timeoutBounded -or
    -not $result.exactOpaqueTarget -or
    -not $result.readbackVerified -or
    -not $result.capabilityMapped -or
    -not $result.foregroundUnchanged -or
    $result.nativeIdentifierLeak -or
    -not $result.timeoutOutcomeUnknown -or
    $result.retrySafe -or
    -not $result.successMappedSafely -or
    -not $result.timeoutMappedSafely -or
    -not $result.appMappedSafely -or
    -not $result.desktopTypeTextMappedSafely -or
    -not $result.staticAssessmentBranches -or
    -not $result.publicRunOpened -or
    $result.userApplicationsWritten -ne 0) {
    throw 'Standard Edit candidate safety test failed.'
}

$cpp = Join-Path `
    $outputRoot 'ai-computer-toolkit-cpp.exe'
$catalog = & $cpp capabilities descriptor `
    win32-control set-text |
    ConvertFrom-Json
$desktopCatalog = & $cpp capabilities descriptor `
    desktop type-text |
    ConvertFrom-Json
$blocked = & $cpp run win32-control set-text |
    ConvertFrom-Json
$blockedExit = $LASTEXITCODE
$desktopBlocked = & $cpp run desktop type-text |
    ConvertFrom-Json
$desktopBlockedExit = $LASTEXITCODE
$method = & $cpp capabilities method win32-message |
    ConvertFrom-Json
$capabilities = & $cpp capabilities app |
    ConvertFrom-Json
$textCapability = @(
    $capabilities.data.capabilities |
        Where-Object { $_.id -eq 'ui.text.input@1' }
)
if ($catalog.data.descriptor.cppStatus -ne
        'available-confirmed-opaque-target' -or
    -not $catalog.data.descriptor.cppExecutionEnabled -or
    $desktopCatalog.data.descriptor.cppStatus -ne
        'available-confirmed-opaque-target' -or
    -not $desktopCatalog.data.descriptor.cppExecutionEnabled -or
    $method.data.methods[0].cppStatus -ne
        'available-confirmed' -or
    $textCapability.Count -ne 1 -or
    $textCapability[0].status -ne
        'available-confirmed' -or
    $blockedExit -eq 0 -or
    $blocked.error.code -ne 'CONFIRMATION_REQUIRED' -or
    $desktopBlockedExit -eq 0 -or
    $desktopBlocked.error.code -ne 'CONFIRMATION_REQUIRED') {
    throw 'Standard Edit certified route is not fail-closed.'
}

$result | ConvertTo-Json
$global:LASTEXITCODE = 0
