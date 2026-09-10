[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$null = Get-Content -LiteralPath (
    Join-Path `
        $projectRoot 'contracts\v1\process-observation.schema.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$status = & $cpp status process | ConvertFrom-Json
$sessions = & $cpp sessions process --max-items 4096 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $status.ok -or
    $status.data.capability -ne 'process.discover@1' -or
    -not $status.data.readOnly -or
    -not $sessions.ok -or
    $sessions.data.capability -ne 'process.discover@1' -or
    -not $sessions.data.foregroundUnchanged -or
    $sessions.data.count -ne $sessions.data.sessions.Count) {
    throw 'C++ direct process observation contract failed.'
}

$rust = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- sessions process --max-items 4096 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or -not $rust.ok) {
    throw 'Rust process compatibility read failed.'
}
$cppNames = @(
    $sessions.data.sessions.processName |
        ForEach-Object { $_.ToLowerInvariant() } |
        Where-Object {
            $_ -notlike 'ai-computer-toolkit*.exe' -and
            $_ -notin @('cargo.exe', 'clang++.exe', 'ld.lld.exe')
        } |
        Sort-Object -Unique
)
$rustNames = @(
    $rust.sessions.processName |
        ForEach-Object { $_.ToLowerInvariant() } |
        Where-Object {
            $_ -notlike 'ai-computer-toolkit*.exe' -and
            $_ -notin @('cargo.exe', 'clang++.exe', 'ld.lld.exe')
        } |
        Sort-Object -Unique
)
$intersection = @(
    $cppNames | Where-Object { $rustNames -contains $_ }
)
$union = @($cppNames + $rustNames | Sort-Object -Unique)
$jaccard = if ($union.Count -eq 0) {
    1.0
} else {
    $intersection.Count / $union.Count
}
if ($jaccard -lt 0.95) {
    throw "Rust/C++ process name Jaccard too low: $jaccard"
}

if ($sessions.data.sessions.Count -gt 0) {
    $target = $sessions.data.sessions[0].sessionId
    $inspect = & $cpp inspect process `
        --target "sessionId=$target" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $inspect.ok -or
        $inspect.data.capability -ne 'process.metadata.read@1' -or
        $inspect.data.process.sessionId -ne $target) {
        throw 'C++ exact process inspection failed.'
    }
}
$stale = & $cpp inspect process `
    --target sessionId=s2:p:0000000000000000 |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ process inspect accepted a stale target.'
}

$serialized = $sessions | ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'processId'
    'processPath'
    'executablePath'
    'nativeProcessId'
    'token'
    'sid'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "C++ process result leaked native field: $term"
    }
}

[PSCustomObject]@{
    ok = $true
    cppCount = $sessions.data.total
    rustCount = $rust.total
    processNameJaccard = [math]::Round($jaccard, 4)
    exactInspect = $sessions.data.sessions.Count -gt 0
    staleTargetRefused = $true
    foregroundUnchanged = $true
    nativeIdentifierLeak = $false
} | ConvertTo-Json
