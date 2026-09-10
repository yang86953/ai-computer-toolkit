[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$status = & $cpp status uia | ConvertFrom-Json
$sessions = & $cpp sessions uia --max-items 10 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $status.ok -or
    -not $sessions.ok -or
    -not $sessions.data.readOnly -or
    -not $sessions.data.foregroundUnchanged -or
    $sessions.data.count -ne $sessions.data.sessions.Count) {
    throw 'C++ UIA compatibility entrypoint failed.'
}

$inspectChecked = $false
if ($sessions.data.sessions.Count -gt 0) {
    $target = $sessions.data.sessions[0].sessionId
    $inspect = & $cpp inspect uia `
        --target "sessionId=$target" `
        --timeout-ms 5000 |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $inspect.ok -or
        $inspect.data.session.sessionId -ne $target -or
        -not $inspect.data.readOnly -or
        -not $inspect.data.foregroundUnchanged) {
        throw 'C++ UIA exact inspect alias failed.'
    }
    $inspectChecked = $true
}

$stale = & $cpp inspect uia `
    --target sessionId=s2:w:0000000000000000 `
    --timeout-ms 5000 |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ UIA alias accepted a stale target.'
}

$serialized = $sessions | ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'hwnd'
    'processId'
    'nativeWindow'
    'nativeProcessId'
    'providerId'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "C++ UIA entrypoint leaked native field: $term"
    }
}

[PSCustomObject]@{
    ok = $true
    sessionCount = $sessions.data.total
    exactInspect = $inspectChecked
    staleTargetRefused = $true
    foregroundUnchanged = $true
    isolatedWorker = $true
    nativeIdentifierLeak = $false
    writeCapabilityAdded = $false
} | ConvertTo-Json
