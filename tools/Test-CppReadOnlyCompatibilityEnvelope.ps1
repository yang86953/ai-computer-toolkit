[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'ai-computer-toolkit-cpp.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') |
    Out-Null

$statusSurfaces = [ordered]@{
    app = 'app'
    uia = 'uia'
    window = 'window'
    process = 'process'
    browser = 'browser'
    'win32-control' = 'win32-control'
    notepad = 'notepad'
    'media-session' = 'media-session'
    desktop = 'desktop'
}
$statusChecked = 0
foreach ($entry in $statusSurfaces.GetEnumerator()) {
    $result = & $cpp status $entry.Key |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $result.ok -or
        $result.app -ne $entry.Value -or
        -not $result.readOnly -or
        $null -eq $result.data) {
        throw "Read-only status envelope failed: $($entry.Key)"
    }
    ++$statusChecked
}

$sessionSurfaces = [ordered]@{
    app = 'app'
    window = 'window'
    process = 'process'
    uia = 'uia'
    'media-session' = 'media-session'
}
$sessionResults = [ordered]@{}
foreach ($entry in $sessionSurfaces.GetEnumerator()) {
    $result = & $cpp sessions $entry.Key --max-items 1 |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $result.ok -or
        $result.app -ne $entry.Value -or
        -not $result.readOnly -or
        $result.count -ne $result.sessions.Count -or
        ($result.sessions | ConvertTo-Json -Depth 20 -Compress) -cne
        ($result.data.sessions |
            ConvertTo-Json -Depth 20 -Compress)) {
        throw "Read-only sessions envelope failed: $($entry.Key)"
    }
    $sessionResults[$entry.Key] = $result
}

$inspectChecked = 0
foreach ($surface in @('app', 'window', 'process', 'uia')) {
    $sessions = $sessionResults[$surface]
    if ($sessions.sessions.Count -eq 0) {
        continue
    }
    $sessionId = $sessions.sessions[0].sessionId
    $result = & $cpp inspect $surface `
        --target "sessionId=$sessionId" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $result.ok -or
        $result.app -ne $surface -or
        -not $result.readOnly -or
        $null -eq $result.data) {
        throw "Read-only inspect envelope failed: $surface"
    }
    ++$inspectChecked
}

$serialized = [PSCustomObject]@{
    statuses = $statusChecked
    sessions = $sessionResults
} | ConvertTo-Json -Depth 30 -Compress
foreach ($term in @(
    'hwnd',
    'processId',
    'className',
    'browserPath',
    'notepadPath',
    'executablePath',
    'providerId',
    'nativeWindow',
    'nativeProcessId',
    'token',
    'sid'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "Compatibility envelope leaked a forbidden field: $term"
    }
}

[PSCustomObject]@{
    ok = $true
    statusSurfaces = $statusChecked
    sessionSurfaces = $sessionResults.Count
    exactInspectSurfaces = $inspectChecked
    topLevelAndDataShareFacts = $true
    opaqueTargetsOnly = $true
    nativeIdentifierLeak = $false
    readOnlyObservationOnly = $true
    certifiedWritesUseSeparateFacade = $true
} | ConvertTo-Json
