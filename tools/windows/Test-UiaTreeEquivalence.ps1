[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cppExecutable = Join-Path (
    $projectRoot
) 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$rustExecutable = Join-Path (
    $projectRoot
) 'target\debug\ai-computer-toolkit.exe'
$policy = Get-Content -LiteralPath (
    Join-Path $projectRoot 'tests\contracts\uia-tree-equivalence-policy.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1')
if ($LASTEXITCODE -ne 0) {
    throw "C++ build failed with exit code $LASTEXITCODE."
}
& cargo build --manifest-path (Join-Path $projectRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    throw "Rust compatibility build failed with exit code $LASTEXITCODE."
}

function Invoke-JsonCommand {
    param(
        [string] $Executable,
        [string[]] $CommandArguments,
        [int] $ExpectedExitCode = 0
    )
    foreach ($argument in $CommandArguments) {
        if ($argument -match '[\s"]') {
            throw "Test argument requires unsupported quoting: $argument"
        }
    }
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.Arguments = $CommandArguments -join ' '
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne $ExpectedExitCode) {
        throw "Unexpected exit $($process.ExitCode): $stdout $stderr"
    }
    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        throw "Command wrote unexpected stderr: $stderr"
    }
    try {
        return $stdout | ConvertFrom-Json
    } catch {
        throw "Command did not return one JSON value: $stdout"
    }
}

function Get-FactSet {
    param([object[]] $Nodes)
    $set = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($node in $Nodes) {
        [void] $set.Add(
            "$($node.depth)|$($node.name)|$($node.className)|$($node.controlType)"
        )
    }
    return ,$set
}

function Get-Jaccard {
    param(
        [System.Collections.Generic.HashSet[string]] $Left,
        [System.Collections.Generic.HashSet[string]] $Right
    )
    $intersection = 0
    foreach ($value in $Left) {
        if ($Right.Contains($value)) {
            $intersection++
        }
    }
    $union = $Left.Count + $Right.Count - $intersection
    if ($union -eq 0) {
        return 1.0
    }
    return [double] $intersection / [double] $union
}

$cppSessions = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @('sessions', 'app', '--max-items', '4096')
$rustSessions = Invoke-JsonCommand -Executable $rustExecutable `
    -CommandArguments @(
        'sessions', 'window', '--max-items', '4096'
    )

$matchedCpp = $null
$matchedRust = $null
foreach ($cppSession in $cppSessions.data.sessions) {
    $candidates = @(
        $rustSessions.sessions | Where-Object {
            $_.sessionId -eq $cppSession.sessionId
        }
    )
    if ($candidates.Count -eq 1) {
        $matchedCpp = $cppSession
        $matchedRust = $candidates[0]
        break
    }
}
if ($null -eq $matchedCpp) {
    throw 'No uniquely matching visible window exists for UIA double-run.'
}

$cppTree = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @(
        'inspect-tree',
        'app',
        '--target',
        "sessionId=$($matchedCpp.sessionId)",
        '--max-depth',
        [string] $policy.maximumDepth,
        '--max-items',
        [string] $policy.maximumItems,
        '--view',
        'control'
    )
$rustTree = Invoke-JsonCommand -Executable $rustExecutable `
    -CommandArguments @(
        'inspect-tree',
        'uia',
        '--target',
        "sessionId=$($matchedRust.sessionId)",
        '--max-depth',
        [string] $policy.maximumDepth,
        '--max-items',
        [string] $policy.maximumItems,
        '--view',
        'control',
        '--timeout-ms',
        [string] $policy.defaultTimeoutMs
    )

if (
    -not $cppTree.ok -or
    $cppTree.contractVersion -ne $policy.contractVersion -or
    $cppTree.data.scope -ne 'bounded-tree' -or
    -not $cppTree.data.readOnly -or
    -not $cppTree.data.foregroundUnchanged -or
    $cppTree.data.safety.valueContentRead -or
    $cppTree.data.safety.textContentRead -or
    $cppTree.data.safety.writePatternsQueried -or
    $cppTree.data.safety.boundsExposed
) {
    throw 'C++ bounded UIA tree violated its public safety contract.'
}
if (
    @($cppTree.data.nodes).Count -gt $policy.maximumItems -or
    @($cppTree.data.nodes | Where-Object {
        $_.depth -gt $policy.maximumDepth
    }).Count -ne 0
) {
    throw 'C++ bounded UIA tree exceeded requested limits.'
}

$serialized = $cppTree | ConvertTo-Json -Depth 30 -Compress
foreach ($field in $policy.forbiddenPublicFields) {
    if ($serialized -match ('"' + [Regex]::Escape($field) + '":')) {
        throw "C++ UIA tree leaked forbidden public field $field."
    }
}

$cppRoot = @($cppTree.data.nodes)[0]
$rustRoot = @($rustTree.nodes)[0]
if (
    $cppRoot.name -ne $rustRoot.name -or
    $cppRoot.className -ne $rustRoot.className -or
    $cppRoot.controlType -ne $rustRoot.controlType
) {
    throw 'Rust/C++ UIA root facts are not equivalent.'
}
$nodeFactJaccard = Get-Jaccard `
    (Get-FactSet @($cppTree.data.nodes)) `
    (Get-FactSet @($rustTree.nodes))
if ($nodeFactJaccard -lt $policy.minimumNodeFactJaccard) {
    throw "UIA node fact Jaccard $nodeFactJaccard is below policy."
}
if (
    $rustTree.capability -ne 'accessibility.tree.read@1' -or
    -not $rustTree.foregroundUnchanged -or
    $rustTree.safety.providerTimeoutIsolation -ne 'job-bounded-worker' -or
    -not $rustTree.safety.workerCancellable -or
    $rustTree.safety.valueContentRead -or
    $rustTree.safety.textContentRead -or
    $rustTree.safety.writePatternsQueried -or
    $rustTree.safety.boundsExposed
) {
    throw 'Rust bounded UIA tree violated its public safety contract.'
}

$rustSerialized = $rustTree | ConvertTo-Json -Depth 30 -Compress
foreach ($field in $policy.forbiddenPublicFields) {
    if ($rustSerialized -match ('"' + [Regex]::Escape($field) + '":')) {
        throw "Rust UIA tree leaked forbidden public field $field."
    }
}

$rawTree = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @(
        'inspect-tree',
        'app',
        '--target',
        "sessionId=$($matchedCpp.sessionId)",
        '--max-depth',
        '1',
        '--max-items',
        '20',
        '--view',
        'raw'
    )
if (
    -not $rawTree.ok -or
    $rawTree.data.view -ne 'raw' -or
    @($rawTree.data.nodes).Count -gt 20 -or
    -not $rawTree.data.foregroundUnchanged
) {
    throw 'C++ raw UIA view did not remain bounded and read-only.'
}

$rustRawTree = Invoke-JsonCommand -Executable $rustExecutable `
    -CommandArguments @(
        'inspect-tree',
        'accessibility',
        '--target',
        "sessionId=$($matchedRust.sessionId)",
        '--max-depth',
        '1',
        '--max-items',
        '20',
        '--view',
        'raw',
        '--timeout-ms',
        [string] $policy.defaultTimeoutMs
    )
if (
    $rustRawTree.view -ne 'raw' -or
    @($rustRawTree.nodes).Count -gt 20 -or
    -not $rustRawTree.foregroundUnchanged
) {
    throw 'Rust raw UIA view did not remain bounded and read-only.'
}

$invalidView = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @(
        'inspect-tree',
        'app',
        '--target',
        "sessionId=$($matchedCpp.sessionId)",
        '--view',
        'content'
    ) -ExpectedExitCode 2
if ($invalidView.ok -or $invalidView.error.code -ne 'INVALID_ARGUMENT') {
    throw 'C++ UIA tree accepted an unregistered view.'
}

$rustInvalidView = Invoke-JsonCommand -Executable $rustExecutable `
    -CommandArguments @(
        'inspect-tree',
        'uia',
        '--target',
        "sessionId=$($matchedRust.sessionId)",
        '--view',
        'content'
    ) -ExpectedExitCode 2
if ($rustInvalidView.error.code -ne 'INVALID_ARGUMENT') {
    throw 'Rust UIA tree accepted an unregistered view.'
}

$stale = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @(
        'inspect-tree',
        'app',
        '--target',
        'sessionId=s2:w:0000000000000000'
    ) -ExpectedExitCode 2
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ UIA tree accepted a stale target.'
}

$rustStale = Invoke-JsonCommand -Executable $rustExecutable `
    -CommandArguments @(
        'inspect-tree',
        'uia',
        '--target',
        'sessionId=s2:w:0000000000000000'
    ) -ExpectedExitCode 2
if ($rustStale.error.code -ne 'STALE_SESSION') {
    throw 'Rust UIA tree accepted a stale target.'
}

$sourceFiles = Get-ChildItem -LiteralPath (
    Join-Path $projectRoot 'cpp\src'
) -Recurse -File
foreach ($sourceFile in $sourceFiles) {
    $source = Get-Content -LiteralPath $sourceFile.FullName -Raw
    foreach ($api in $policy.forbiddenCppApis) {
        $callOnly = $api -in @(
            'SetForegroundWindow',
            'SendInput',
            'SetFocus',
            'SetClipboardData'
        )
        $found = if ($callOnly) {
            $source -cmatch (
                '\b' + [regex]::Escape($api) + '\s*\('
            )
        } else {
            $source.Contains($api)
        }
        if ($found) {
            throw "$($sourceFile.FullName) contains forbidden UIA/write API $api."
        }
    }
}

$rustWorkerSource = Get-Content -LiteralPath (
    Join-Path $projectRoot 'src\observation_worker.rs'
) -Raw
foreach ($api in $policy.forbiddenRustWorkerApis) {
    if ($rustWorkerSource.Contains($api)) {
        throw "Rust observation worker contains forbidden UIA/write API $api."
    }
}

$obsidianOutcome = 'not-running-safe-skip'
$obsidian = Get-Process -Name 'Obsidian' -ErrorAction SilentlyContinue
if ($null -ne $obsidian) {
    $obsidianSession = @(
        $cppSessions.data.sessions | Where-Object {
            $_.applicationName -ieq 'Obsidian.exe'
        } | Select-Object -First 1
    )
    if ($obsidianSession.Count -eq 0) {
        $obsidianOutcome = 'running-without-visible-titled-window'
    } else {
        $obsidianTree = Invoke-JsonCommand -Executable $cppExecutable `
            -CommandArguments @(
                'inspect-tree',
                'app',
                '--target',
                "sessionId=$($obsidianSession[0].sessionId)",
                '--max-depth',
                '2',
                '--max-items',
                '100',
                '--view',
                'control'
            )
        if (
            -not $obsidianTree.ok -or
            -not $obsidianTree.data.foregroundUnchanged
        ) {
            throw 'Running Obsidian read-only verification failed.'
        }
        $obsidianOutcome = 'running-read-only-tree-passed'
    }
}

[PSCustomObject]@{
    ok = $true
    matchedApplication = $matchedCpp.applicationName
    matchedTitle = $matchedCpp.title
    cppNodeCount = @($cppTree.data.nodes).Count
    rustNodeCount = @($rustTree.nodes).Count
    nodeFactJaccard = [Math]::Round($nodeFactJaccard, 4)
    controlViewPassed = $true
    rawViewPassed = $true
    bounded = $true
    nativeOrContentLeak = $false
    foregroundUnchanged = $true
    staleTargetError = $stale.error.code
    obsidian = $obsidianOutcome
} | ConvertTo-Json
