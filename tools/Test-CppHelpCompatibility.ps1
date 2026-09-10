[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'ai-computer-toolkit-cpp.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') |
    Out-Null
$rust = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- help | ConvertFrom-Json
$cppHelp = & $cpp help | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    ($cppHelp | ConvertTo-Json -Depth 20 -Compress) -cne
    ($rust | ConvertTo-Json -Depth 20 -Compress)) {
    throw 'C++ help JSON is not equivalent to Rust.'
}

$prettyText = & $cpp help --pretty | Out-String
$prettyHelp = $prettyText | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    $prettyText -notmatch '\r?\n\s+"ok": true' -or
    ($prettyHelp | ConvertTo-Json -Depth 20 -Compress) -cne
    ($rust | ConvertTo-Json -Depth 20 -Compress)) {
    throw 'Global C++ --pretty did not preserve the help contract.'
}

$rustCatalog = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- catalog app | ConvertFrom-Json
$prettyCatalogText = & $cpp --pretty catalog app |
    Out-String
$prettyCatalog = $prettyCatalogText | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    $prettyCatalogText -notmatch '\r?\n\s+"apps": \[' -or
    ($prettyCatalog | ConvertTo-Json -Depth 30 -Compress) -cne
    ($rustCatalog | ConvertTo-Json -Depth 30 -Compress)) {
    throw 'Global --pretty was not accepted before the command.'
}

$invalid = & $cpp help unexpected | ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $invalid.error.code -ne 'INVALID_ARGUMENT') {
    throw 'C++ help accepted an unexpected positional argument.'
}
$rustVersion = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- version | ConvertFrom-Json
$cppVersion = & $cpp version | ConvertFrom-Json
$buildInfo = & $cpp build-info | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    ($cppVersion | ConvertTo-Json -Compress) -cne
    ($rustVersion | ConvertTo-Json -Compress) -or
    -not $buildInfo.ok -or
    $buildInfo.data.mainLanguage -ne 'C++23' -or
    $buildInfo.data.compatibilityVersion -ne
    $cppVersion.version) {
    throw 'Version compatibility and C++ build info diverged.'
}

[PSCustomObject]@{
    ok = $true
    helpEquivalent = $true
    usageEntries = $cppHelp.usage.Count
    prettyAfterCommand = $true
    prettyBeforeCommand = $true
    invalidArgumentRefused = $true
    versionEquivalent = $true
    cppBuildInfoSeparated = $true
    cargoRuntimeRequiredByCpp = $false
    mutationEnabled = $false
} | ConvertTo-Json
