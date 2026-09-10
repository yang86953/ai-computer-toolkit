[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$executable = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$fixture = Join-Path $outputRoot 'act-worker-fixture.exe'
$lifecycleTest = Join-Path $outputRoot 'act-worker-process-test.exe'

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1')
if ($LASTEXITCODE -ne 0) {
    throw "C++ build failed with exit code $LASTEXITCODE."
}

$compiler = (Get-Command clang++ -ErrorAction Stop).Source
$commonArguments = @(
    '-std=c++23'
    '-Wall'
    '-Wextra'
    '-Wpedantic'
    '-Werror'
    '-DUNICODE'
    '-D_UNICODE'
    '-DWIN32_LEAN_AND_MEAN'
    '-DNOMINMAX'
    '-D_WIN32_WINNT=0x0A00'
    "-I$(Join-Path $sourceRoot 'include')"
    "-I$(Join-Path $sourceRoot 'src')"
)

& $compiler @commonArguments `
    (Join-Path $sourceRoot 'tests\worker_fixture_main.cpp') `
    -o $fixture
if ($LASTEXITCODE -ne 0) {
    throw "Worker fixture build failed with exit code $LASTEXITCODE."
}

& $compiler @commonArguments `
    (Join-Path $sourceRoot 'tests\worker_process_test.cpp') `
    (Join-Path $sourceRoot 'src\components\cancellation.cpp') `
    (Join-Path $sourceRoot 'src\components\json.cpp') `
    (Join-Path $sourceRoot 'src\components\worker_process.cpp') `
    -o $lifecycleTest
if ($LASTEXITCODE -ne 0) {
    throw "Worker lifecycle test build failed with exit code $LASTEXITCODE."
}

$lifecycle = & $lifecycleTest | ConvertFrom-Json
if (
    -not $lifecycle.ok -or
    -not $lifecycle.timeoutJobTerminated -or
    -not $lifecycle.cancellationJobTerminated
) {
    throw 'Worker lifecycle test did not prove termination.'
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
            throw "Test argument requires unsupported quoting: $argument"
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
        throw "Unexpected stderr: $stderr"
    }
    return $stdout | ConvertFrom-Json
}

$sessions = Invoke-JsonCommand -CommandArguments @(
    'sessions', 'app', '--max-items', '100'
)
$windowSessions = @(
    $sessions.data.sessions |
        Where-Object {
            $_.kind -eq 'window' -and
            $_.targetKind -eq 'application-window' -and
            $_.capabilities.id -contains
                'accessibility.tree.read@1'
        }
)
$normalOutcome = 'skipped-no-visible-window'
$timeoutOutcome = 'skipped-no-visible-window'
if ($windowSessions.Count -gt 0) {
    $sessionId = $windowSessions[0].sessionId
    $normal = Invoke-JsonCommand -CommandArguments @(
        'inspect-tree'
        'app'
        '--target'
        "sessionId=$sessionId"
        '--max-depth'
        '2'
        '--max-items'
        '100'
        '--view'
        'control'
        '--timeout-ms'
        '5000'
    )
    if (
        -not $normal.ok -or
        $normal.data.safety.providerTimeoutIsolation -ne
            'job-bounded-worker' -or
        -not $normal.data.safety.workerCancellable -or
        -not $normal.data.foregroundUnchanged
    ) {
        throw 'Normal isolated UIA observation violated its contract.'
    }
    $normalOutcome = 'passed'

    $timeout = Invoke-JsonCommand -CommandArguments @(
        'inspect-tree'
        'app'
        '--target'
        "sessionId=$sessionId"
        '--max-depth'
        '20'
        '--max-items'
        '4096'
        '--view'
        'raw'
        '--timeout-ms'
        '1'
    ) -ExpectedExitCode 2
    if ($timeout.ok -or $timeout.error.code -ne 'TIMEOUT') {
        throw 'The public CLI did not preserve worker timeout semantics.'
    }
    $timeoutOutcome = 'passed'
}

Start-Sleep -Milliseconds 100
$remainingWorkers = @(
    Get-Process -Name 'ai-computer-toolkit-observation-worker' `
        -ErrorAction SilentlyContinue
)
if ($remainingWorkers.Count -ne 0) {
    throw 'An isolated observation worker remained after the test.'
}

$forbidden = @(
    'SetForegroundWindow'
    'SendInput'
    'SetFocus'
    'SetClipboardData'
    'IUIAutomationInvokePattern'
    'IUIAutomationValuePattern'
    'IUIAutomationTextPattern'
    'GetCurrentPattern'
)
$workerSources = @(
    Join-Path $sourceRoot 'src\worker\observation_worker_main.cpp'
    Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp'
)
foreach ($api in $forbidden) {
    $matches = Select-String -LiteralPath $workerSources -Pattern $api `
        -SimpleMatch
    if ($matches) {
        throw "Forbidden UI/write API found in worker sources: $api"
    }
}

[PSCustomObject]@{
    ok = $true
    lifecycleTimeout = 'job-terminated'
    lifecycleCancellation = 'job-terminated'
    normalObservation = $normalOutcome
    publicTimeout = $timeoutOutcome
    remainingWorkers = 0
    foregroundUnchanged = $true
    forbiddenWorkerApis = 0
} | ConvertTo-Json
