[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'ai-computer-toolkit-cpp.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') |
    Out-Null
$cppSessions = & $cpp sessions win32-control `
    --max-items 4096 |
    ConvertFrom-Json
$cppExitCode = $LASTEXITCODE
$rustSessions = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- sessions win32-control --max-items 4096 |
    ConvertFrom-Json
$rustExitCode = $LASTEXITCODE
if ($cppExitCode -ne 0 -or
    $rustExitCode -ne 0 -or
    -not $cppSessions.ok -or
    $cppSessions.app -ne 'win32-control' -or
    -not $cppSessions.readOnly -or
    -not $cppSessions.foregroundUnchanged -or
    $cppSessions.count -ne $cppSessions.sessions.Count -or
    $cppSessions.total -lt $cppSessions.count -or
    $cppSessions.truncated -ne
        ($cppSessions.total -gt $cppSessions.count) -or
    -not $rustSessions.ok -or
    -not $rustSessions.foreground.unchanged) {
    throw 'Standard Edit read-only discovery failed.'
}
foreach ($session in $cppSessions.sessions) {
    if ($session.assessment.decision -notin @(
            'requires-confirmation',
            'permission-blocked',
            'indeterminate'
        ) -or
        $session.assessment.safeToExecuteNow -or
        -not $session.assessment.requiresConfirmation -or
        $session.assessment.foregroundRequired -or
        $session.assessment.activeWriteProbePerformed) {
        throw 'Standard Edit static permission assessment is unsafe.'
    }
}

$cppFacts = @(
    $cppSessions.sessions |
        ForEach-Object {
            "$($_.applicationName.ToLowerInvariant())|$($_.visible)"
        } |
        Sort-Object -Unique
)
$rustFacts = @(
    $rustSessions.sessions |
        ForEach-Object {
            "$($_.processName.ToLowerInvariant())|$($_.visible)"
        } |
        Sort-Object -Unique
)
$intersection = @(
    $cppFacts | Where-Object { $rustFacts -contains $_ }
)
$union = @(
    $cppFacts + $rustFacts |
        Sort-Object -Unique
)
$jaccard = if ($union.Count -eq 0) {
    1.0
} else {
    $intersection.Count / $union.Count
}
if ($jaccard -lt 0.8) {
    throw "Standard Edit observation Jaccard too low: $jaccard"
}

$exactInspect = $false
if ($cppSessions.sessions.Count -gt 0) {
    $target = $cppSessions.sessions[0].sessionId
    $inspect = & $cpp inspect win32-control `
        --target "sessionId=$target" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $inspect.ok -or
        $inspect.app -ne 'win32-control' -or
        $inspect.control.sessionId -ne $target -or
        -not $inspect.readOnly -or
        -not $inspect.foregroundUnchanged) {
        throw 'Exact Standard Edit inspection failed.'
    }
    $exactInspect = $true
}
$stale = & $cpp inspect win32-control `
    --target sessionId=s2:c:0000000000000000 |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $stale.error.code -ne 'STALE_SESSION') {
    throw 'Standard Edit inspect accepted a stale target.'
}

$serialized = $cppSessions |
    ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'hwnd',
    'processId',
    'className',
    'nativeControl',
    'nativeProcessId'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "Standard Edit observation leaked: $term"
    }
}
$blocked = & $cpp run win32-control set-text |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $blocked.error.code -ne 'CONFIRMATION_REQUIRED') {
    throw 'Standard Edit mutation did not retain confirmation-first gating.'
}

[PSCustomObject]@{
    ok = $true
    cppCount = $cppSessions.total
    rustCount = $rustSessions.total
    factJaccard = [math]::Round($jaccard, 4)
    exactInspect = $exactInspect
    staleTargetRefused = $true
    foregroundUnchanged = $true
    opaqueTargetsOnly = $true
    nativeIdentifierLeak = $false
    publicRunOpened = $true
    staticPermissionAssessment = $true
    activeWriteProbes = 0
} | ConvertTo-Json
$global:LASTEXITCODE = 0
