[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1')
if ($LASTEXITCODE -ne 0) {
    throw "C++ discovery slice build failed with exit code $LASTEXITCODE."
}

function Invoke-JsonCommand {
    param(
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        [int] $ExpectedExitCode = 0
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $executable
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $CommandArguments) {
        if ($argument -match '[\s"]') {
            throw "Test argument requires unsupported command-line quoting: $argument"
        }
    }
    $startInfo.Arguments = $CommandArguments -join ' '
    $process = [System.Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()

    if ($process.ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected exit code $($process.ExitCode): $stdout $stderr"
    }
    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        throw "Command wrote unexpected stderr: $stderr"
    }
    try {
        return $stdout | ConvertFrom-Json
    } catch {
        throw "Command did not write one valid JSON result: $stdout"
    }
}

$catalog = Invoke-JsonCommand -CommandArguments @('capabilities', 'app')
if (-not $catalog.ok -or
    $catalog.data.supportLevel -ne 'L2-confirmed-background') {
    throw 'C++ catalog did not report the confirmed background slice.'
}

$status = Invoke-JsonCommand -CommandArguments @('status', 'app')
if (
    -not $status.ok -or
    -not $status.data.canDiscoverApplications -or
    -not $status.data.writesEnabled -or
    -not $status.data.allWritesRequireConfirmation -or
    $status.data.writableCapabilities -notcontains
        'text.document.create@1' -or
    $status.data.writableCapabilities -notcontains
        'window.close@1'
) {
    throw 'C++ status did not separate read discovery from confirmed writes.'
}

$sessions = Invoke-JsonCommand -CommandArguments @(
    'sessions', 'app', '--max-items', '100'
)
if (-not $sessions.ok -or -not $sessions.data.foregroundUnchanged) {
    throw 'C++ sessions did not preserve the foreground invariant.'
}
$serializedSessions = $sessions | ConvertTo-Json -Depth 20 -Compress
if (
    $serializedSessions -match '"(hwnd|pid|processId|nativeHandle|providerId)"'
) {
    throw 'C++ public discovery result leaked a native or provider identifier.'
}
foreach ($session in $sessions.data.sessions) {
    $validWindow =
        $session.sessionId -match '^s2:w:[0-9a-f]{16}$'
    $validTextCreator =
        $session.sessionId -match '^s2:a:[0-9a-f]{16}$' -and
        $session.kind -eq 'application' -and
        $session.capabilities.id -contains
            'text.document.create@1'
    $validStandardEdit =
        $session.sessionId -match '^s2:c:[0-9a-f]{16}$' -and
        $session.kind -eq 'control' -and
        $session.capabilities.id -contains
            'ui.text.input@1'
    if (-not $validWindow -and
        -not $validTextCreator -and
        -not $validStandardEdit) {
        throw "Invalid opaque session ID: $($session.sessionId)"
    }
}

$inspectOutcome = 'skipped-no-visible-window'
$windowSessions = @(
    $sessions.data.sessions |
        Where-Object {
            $_.sessionId -match '^s2:w:[0-9a-f]{16}$'
        }
)
if ($windowSessions.Count -gt 0) {
    $sessionId = $windowSessions[0].sessionId
    $inspection = Invoke-JsonCommand -CommandArguments @(
        'inspect', 'app', '--target', "sessionId=$sessionId"
    )
    if (
        -not $inspection.ok -or
        -not $inspection.data.readOnly -or
        -not $inspection.data.foregroundUnchanged -or
        $inspection.data.accessibility.scope -ne 'root-only'
    ) {
        throw 'C++ UIA inspection violated the read-only root-only contract.'
    }
    $serializedInspection = $inspection | ConvertTo-Json -Depth 20 -Compress
    if (
        $serializedInspection -match
        '"(hwnd|pid|processId|nativeHandle|providerId)"'
    ) {
        throw 'C++ inspection leaked a native or provider identifier.'
    }
    $inspectOutcome = 'passed'
}

$stale = Invoke-JsonCommand -CommandArguments @(
    'inspect', 'app', '--target', 'sessionId=s2:w:0000000000000000'
) -ExpectedExitCode 2
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ inspect did not preserve stale exact-target semantics.'
}

$mutation = Invoke-JsonCommand -CommandArguments @(
    'run', 'app', 'press-key', '--confirm', '--allow-foreground'
) -ExpectedExitCode 2
if ($mutation.ok -or $mutation.error.code -ne 'CAPABILITY_GAP') {
    throw 'Unmigrated C++ mutation did not fail closed with CAPABILITY_GAP.'
}

[PSCustomObject]@{
    ok = $true
    sessionCount = $sessions.data.sessions.Count
    inspect = $inspectOutcome
    nativeIdentifierLeak = $false
    foregroundUnchanged = $true
    staleTargetError = $stale.error.code
    unmigratedMutationError = $mutation.error.code
} | ConvertTo-Json
