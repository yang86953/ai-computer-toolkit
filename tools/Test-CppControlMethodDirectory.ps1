[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$null = Get-Content -LiteralPath (
    Join-Path `
        $projectRoot 'contracts\v1\control-method-directory.schema.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$cppMethods = & $cpp methods | ConvertFrom-Json
$rustMethods = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- methods | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $cppMethods.ok -or
    -not $rustMethods.ok -or
    ($cppMethods | ConvertTo-Json -Depth 20 -Compress) -cne
    ($rustMethods | ConvertTo-Json -Depth 20 -Compress)) {
    throw 'Control method directory command failed.'
}
$cppIds = @($cppMethods.methods.id | Sort-Object)
$rustIds = @($rustMethods.methods.id | Sort-Object)
if (($cppIds -join "`n") -ne ($rustIds -join "`n")) {
    throw 'C++ method IDs differ from the Rust compatibility directory.'
}
foreach ($rustMethod in $rustMethods.methods) {
    $cppStatus = & $cpp capabilities method $rustMethod.id |
        ConvertFrom-Json
    $cppMethod = @(
        $cppStatus.data.methods |
            Where-Object { $_.id -eq $rustMethod.id }
    )
    if ($LASTEXITCODE -ne 0 -or
        -not $cppStatus.ok -or
        $cppMethod.Count -ne 1 -or
        $cppMethod[0].availability -ne
            $rustMethod.availability -or
        $cppMethod[0].executionScope -ne
            $rustMethod.executionScope -or
        [string]::IsNullOrWhiteSpace($cppMethod[0].cppStatus) -or
        [string]::IsNullOrWhiteSpace(
            $cppMethod[0].safetyBoundary)) {
        throw "Method compatibility mismatch: $($rustMethod.id)"
    }
}

$media = & $cpp capabilities method media-session |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    $media.data.methods.Count -ne 1 -or
    $media.data.methods[0].cppStatus -ne
        'available-confirmed') {
    throw 'Media method did not expose the confirmed isolated C++ route.'
}
$wgc = & $cpp capabilities method windows-graphics-capture |
    ConvertFrom-Json
if ($wgc.data.methods[0].cppStatus -ne
    'available-confirmed') {
    throw 'WGC method did not expose the confirmed opaque-target route.'
}
$standardEdit = & $cpp capabilities method win32-message |
    ConvertFrom-Json
if ($standardEdit.data.methods[0].cppStatus -ne
    'available-confirmed') {
    throw 'Win32 method did not expose the confirmed Standard Edit route.'
}
$browser = & $cpp capabilities method headless-browser |
    ConvertFrom-Json
if ($browser.data.methods[0].cppStatus -ne
    'available-confirmed') {
    throw 'Headless browser method did not expose the isolated C++ route.'
}
$unknown = & $cpp methods not-a-method | ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $unknown.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Unknown method was accepted.'
}

[PSCustomObject]@{
    ok = $true
    methodCount = $cppIds.Count
    rustIdsEquivalent = $true
    legacyJsonEquivalent = $true
    cppMigrationStatusExplicit = $true
    screenshotConfirmedCppRoute = $true
    standardEditConfirmedCppRoute = $true
    browserConfirmedCppRoute = $true
    mediaControlConfirmedCppRoute = $true
    unknownMethodRefused = $true
} | ConvertTo-Json
$global:LASTEXITCODE = 0
