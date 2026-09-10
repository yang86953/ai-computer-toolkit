[CmdletBinding()]
param(
    [switch] $SkipBuild
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$fixtureRoot = Join-Path $outputRoot 'text-document-equivalence'
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
$mapperTest =
    Join-Path $outputRoot 'act-text-document-compatibility-test.exe'
$existingFixtureRoot =
    Join-Path $fixtureRoot 'existing-notepad-fixture'
$existingFixture =
    Join-Path $existingFixtureRoot 'Notepad.exe'
$staleRequestPath = Join-Path $fixtureRoot 'stale-request.json'
$existingRequestPath = Join-Path $fixtureRoot 'existing-request.json'
$rustRequestPath = Join-Path $fixtureRoot 'rust-request.json'
$cppRequestPath = Join-Path $fixtureRoot 'cpp-request.json'
$rustOutputPath = Join-Path $fixtureRoot 'rust-output.json'
$cppOutputPath = Join-Path $fixtureRoot 'cpp-output.json'
$rustErrorPath = Join-Path $fixtureRoot 'rust-error.txt'
$cppErrorPath = Join-Path $fixtureRoot 'cpp-error.txt'
$policyPath = Join-Path $projectRoot (
    'tests\contracts\text-document-create-compatibility-policy.json'
)

$resolvedBuild = [IO.Path]::GetFullPath($outputRoot)
$resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
if (-not $resolvedFixture.StartsWith(
    $resolvedBuild + [IO.Path]::DirectorySeparatorChar,
    [StringComparison]::OrdinalIgnoreCase
)) {
    throw 'Text document fixture escaped the build directory.'
}
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
New-Item -ItemType Directory -Path $existingFixtureRoot -Force |
    Out-Null

function Invoke-AppCreate {
    param(
        [Parameter(Mandatory)]
        [string] $Runtime,
        [Parameter(Mandatory)]
        [string] $Request,
        [Parameter(Mandatory)]
        [string] $Output,
        [Parameter(Mandatory)]
        [string] $ErrorOutput
    )
    [IO.File]::Delete($Output)
    [IO.File]::Delete($ErrorOutput)
    $process = Start-Process `
        -FilePath $Runtime `
        -ArgumentList @(
            'run'
            'app'
            'create'
            '--input'
            $Request
            '--confirm'
        ) `
        -RedirectStandardOutput $Output `
        -RedirectStandardError $ErrorOutput `
        -WindowStyle Hidden `
        -PassThru
    $runtimeProcessId = $process.Id
    $ownedProcessIds =
        [Collections.Generic.HashSet[int]]::new()
    [void]$ownedProcessIds.Add($runtimeProcessId)
    for ($attempt = 0; $attempt -lt 100; ++$attempt) {
        $snapshot = Get-CimInstance Win32_Process
        foreach ($candidate in $snapshot) {
            if ($candidate.Name -eq 'Notepad.exe' -and
                $ownedProcessIds.Contains(
                    [int]$candidate.ParentProcessId
                )) {
                [void]$ownedProcessIds.Add(
                    [int]$candidate.ProcessId
                )
            }
        }
        if ($null -eq (
            Get-Process `
                -Id $runtimeProcessId `
                -ErrorAction SilentlyContinue
        )) {
            break
        }
        Start-Sleep -Milliseconds 100
    }
    if ($null -ne (
        Get-Process `
            -Id $runtimeProcessId `
            -ErrorAction SilentlyContinue
    )) {
        Stop-Process -Id $runtimeProcessId -Force
        throw 'The app create CLI did not exit within 10 seconds.'
    }
    for ($attempt = 0; $attempt -lt 5; ++$attempt) {
        $snapshot = Get-CimInstance Win32_Process
        foreach ($candidate in $snapshot) {
            if ($candidate.Name -eq 'Notepad.exe' -and
                $ownedProcessIds.Contains(
                    [int]$candidate.ParentProcessId
                )) {
                [void]$ownedProcessIds.Add(
                    [int]$candidate.ProcessId
                )
            }
        }
        Start-Sleep -Milliseconds 50
    }
    foreach ($ownedProcessId in $ownedProcessIds) {
        if ($ownedProcessId -eq $runtimeProcessId) {
            continue
        }
        $owned = Get-CimInstance Win32_Process -Filter (
            "ProcessId=$ownedProcessId"
        )
        if ($null -ne $owned -and
            $owned.Name -eq 'Notepad.exe') {
            Stop-Process -Id $ownedProcessId -Force
        }
    }
    $process.Dispose()
    $raw = ''
    for ($attempt = 0; $attempt -lt 20; ++$attempt) {
        $raw = [IO.File]::ReadAllText($Output)
        if (-not [string]::IsNullOrWhiteSpace($raw)) {
            break
        }
        Start-Sleep -Milliseconds 50
    }
    if ([string]::IsNullOrWhiteSpace($raw)) {
        throw 'The app create CLI returned no JSON result.'
    }
    return $raw | ConvertFrom-Json
}

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
    cargo build --quiet --manifest-path (
        Join-Path $projectRoot 'Cargo.toml'
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Rust compatibility build failed.'
    }

    $compiler = (Get-Command clang++ -ErrorAction Stop).Source
    & $compiler @(
        '-std=c++23'
        '-Wall'
        '-Wextra'
        '-Wpedantic'
        '-Werror'
        "-I$(Join-Path $sourceRoot 'src')"
        (Join-Path $sourceRoot 'tests\text_document_compatibility_test.cpp')
        (Join-Path $sourceRoot 'src\components\json.cpp')
        (Join-Path $sourceRoot 'src\components\utf8.cpp')
        (Join-Path $sourceRoot 'src\modules\text_document_compatibility_module.cpp')
        '-o'
        $mapperTest
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Text compatibility mapper test build failed.'
    }
    & $compiler @(
        '-std=c++23'
        '-Wall'
        '-Wextra'
        '-Wpedantic'
        '-Werror'
        (Join-Path $sourceRoot 'tests\named_process_fixture.cpp')
        '-o'
        $existingFixture
    )
    if ($LASTEXITCODE -ne 0) {
        throw 'Named process fixture build failed.'
    }
}
$mapper = & $mapperTest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $mapper.utf8Validation -or
    -not $mapper.legacyShape -or
    -not $mapper.facadeSanitized -or
    -not $mapper.incompleteResultRefused) {
    throw 'Text compatibility mapper policy failed.'
}
$policy = Get-Content -LiteralPath $policyPath -Raw |
    ConvertFrom-Json
if ($policy.capability -ne 'text.document.create@1' -or
    $policy.maximumUtf8Bytes -ne 1048576 -or
    $policy.compatibilityShape -ne
        'provider-neutral-text-artifact-v1') {
    throw 'Text document compatibility policy is invalid.'
}
$preexistingNotepad = @(
    Get-Process Notepad -ErrorAction SilentlyContinue
)
if ($preexistingNotepad.Count -ne 0) {
    throw 'Actual text equivalence requires no pre-existing Notepad process.'
}

$cppSessions = & $cpp sessions app --max-items 4096 |
    ConvertFrom-Json
$rustSessions = & $rust sessions app --max-items 4096 |
    ConvertFrom-Json
$cppText = @(
    $cppSessions.sessions |
        Where-Object {
            $_.capabilities.id -contains 'text.document.create@1'
        }
)
$rustText = @(
    $rustSessions.sessions |
        Where-Object {
            $_.capabilities.id -contains 'text.document.create@1'
        }
)
if ($cppText.Count -ne 1 -or
    $rustText.Count -ne 1 -or
    # 两种实现都必须发布 canonical s2 应用目标.
    $cppText[0].sessionId -notlike 's2:a:*' -or
    # 固定文本创建器的稳定身份必须跨实现逐字节相等.
    $rustText[0].sessionId -cne $cppText[0].sessionId -or
    $cppText[0].kind -ne $rustText[0].kind -or
    $cppText[0].state -ne $rustText[0].state -or
    $cppText[0].title -ne $rustText[0].title) {
    throw 'Rust/C++ text document discovery contract diverged.'
}
$assessment = & $cpp assess app `
    --capability text.document.create@1 `
    --target "sessionId=$($cppText[0].sessionId)" |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    $assessment.decision -ne $policy.decision -or
    $assessment.executionRealm -ne $policy.executionRealm -or
    -not $assessment.requiresConfirmation -or
    $assessment.requiresForegroundConsent -or
    [bool]$assessment.constraints.readOnly -ne
        [bool]$policy.readOnly -or
    $assessment.evidence.targetKind -ne
        'installed-application') {
    throw 'Text document capability assessment is not fail-closed.'
}

$unconfirmed = & $cpp run notepad open-and-write-text `
    --arg text=unconfirmed |
    ConvertFrom-Json
if ($unconfirmed.ok -or
    $unconfirmed.error.code -ne 'CONFIRMATION_REQUIRED') {
    throw 'C++ text create did not fail closed before argument dispatch.'
}

$staleRequest = [ordered]@{
    target = [ordered]@{
        sessionId = 's2:a:0000000000000000'
    }
    args = [ordered]@{
        capability = 'text.document.create@1'
        input = [ordered]@{ text = 'stale' }
    }
    confirmed = $true
}
[IO.File]::WriteAllText(
    $staleRequestPath,
    ($staleRequest | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)
$stale = & $cpp run app create --input $staleRequestPath |
    ConvertFrom-Json
if ($stale.ok -or
    $stale.error.code -ne 'TARGET_NOT_FOUND') {
    throw 'C++ app create accepted a stale opaque target.'
}

$existingRequest = [ordered]@{
    target = [ordered]@{
        sessionId = $cppText[0].sessionId
    }
    args = [ordered]@{
        capability = 'text.document.create@1'
        input = [ordered]@{ text = 'must-not-create' }
    }
    confirmed = $true
}
[IO.File]::WriteAllText(
    $existingRequestPath,
    ($existingRequest | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)
$beforeArtifacts = @(
    Get-ChildItem ([IO.Path]::GetTempPath()) `
        -Filter 'ai-computer-toolkit-notepad-*.txt' |
        Select-Object -ExpandProperty FullName
)
$existingFixtureProcess = Start-Process `
    -FilePath $existingFixture `
    -WindowStyle Hidden `
    -PassThru
try {
    Start-Sleep -Milliseconds 250
    $existingResult = & $cpp run app create `
        --input $existingRequestPath |
        ConvertFrom-Json
} finally {
    if ($null -ne $existingFixtureProcess -and
        -not $existingFixtureProcess.HasExited) {
        Stop-Process -Id $existingFixtureProcess.Id -Force
        [void]$existingFixtureProcess.WaitForExit(5000)
    }
    $existingFixtureProcess.Dispose()
}
$afterArtifacts = @(
    Get-ChildItem ([IO.Path]::GetTempPath()) `
        -Filter 'ai-computer-toolkit-notepad-*.txt' |
        Select-Object -ExpandProperty FullName
)
if ($existingResult.ok -or
    $existingResult.error.code -ne
        'BACKGROUND_OPERATION_UNAVAILABLE' -or
    $existingResult.error.details.reason -ne
        'existing-application-session-attachment-not-certified' -or
    $existingResult.error.details.artifactCreated -or
    $existingResult.error.details.safeToRetryAutomatically -or
    @($afterArtifacts | Where-Object {
        $_ -notin $beforeArtifacts
    }).Count -ne 0) {
    throw 'Existing Notepad attachment did not fail before artifact creation.'
}

$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$unicodeSample = -join @(
    [char]0x6587
    [char]0x672C
    [char]0x2713
)
$rustTextValue = "Rust/C++ equivalence $stamp`nUTF-8 $unicodeSample"
$cppTextValue = $rustTextValue
$artifacts = [Collections.Generic.List[string]]::new()
$generatedFiles = @(
    $staleRequestPath
    $existingRequestPath
    $rustRequestPath
    $cppRequestPath
    $rustOutputPath
    $cppOutputPath
    $rustErrorPath
    $cppErrorPath
)
try {
    $rustRequest = [ordered]@{
        target = [ordered]@{
            sessionId = $rustText[0].sessionId
        }
        args = [ordered]@{
            capability = 'text.document.create@1'
            input = [ordered]@{ text = $rustTextValue }
        }
        confirmed = $true
    }
    $cppRequest = [ordered]@{
        target = [ordered]@{
            sessionId = $cppText[0].sessionId
        }
        args = [ordered]@{
            capability = 'text.document.create@1'
            input = [ordered]@{ text = $cppTextValue }
        }
        confirmed = $true
    }
    [IO.File]::WriteAllText(
        $rustRequestPath,
        ($rustRequest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        $cppRequestPath,
        ($cppRequest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    $rustResult = Invoke-AppCreate `
        -Runtime $rust `
        -Request $rustRequestPath `
        -Output $rustOutputPath `
        -ErrorOutput $rustErrorPath
    if (-not $rustResult.ok) {
        throw 'Rust text document compatibility execution failed.'
    }
    $artifacts.Add([string]$rustResult.data.path)
    for ($attempt = 0; $attempt -lt 50; ++$attempt) {
        if (@(
            Get-Process Notepad -ErrorAction SilentlyContinue
        ).Count -eq 0) {
            break
        }
        Start-Sleep -Milliseconds 100
    }
    if (@(
        Get-Process Notepad -ErrorAction SilentlyContinue
    ).Count -ne 0) {
        throw 'The Rust test-owned Notepad process did not terminate.'
    }

    $cppResult = Invoke-AppCreate `
        -Runtime $cpp `
        -Request $cppRequestPath `
        -Output $cppOutputPath `
        -ErrorOutput $cppErrorPath
    if (-not $cppResult.ok) {
        throw "C++ text document execution failed: $($cppResult.error.code)"
    }
    $artifacts.Add([string]$cppResult.data.path)

    Start-Sleep -Milliseconds 500
    foreach ($pair in @(
        [pscustomobject]@{
            Result = $rustResult
            Text = $rustTextValue
        }
        [pscustomobject]@{
            Result = $cppResult
            Text = $cppTextValue
        }
    )) {
        if (-not $pair.Result.meta.foreground.unchanged -or
            $pair.Result.data.text -cne $pair.Text -or
            -not (Test-Path -LiteralPath $pair.Result.data.path) -or
            [IO.File]::ReadAllText(
                $pair.Result.data.path,
                [Text.Encoding]::UTF8
            ) -cne $pair.Text) {
            throw 'A text artifact failed foreground/content verification.'
        }
    }
} finally {
    foreach ($artifact in $artifacts) {
        $name = [IO.Path]::GetFileName($artifact)
        $ownedProcesses = @(
            Get-CimInstance Win32_Process |
                Where-Object {
                    $_.Name -eq 'Notepad.exe' -and
                    $_.CommandLine -like "*$name*"
                }
        )
        foreach ($ownedProcess in $ownedProcesses) {
            Stop-Process -Id $ownedProcess.ProcessId -Force
        }
    }
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    foreach ($artifact in $artifacts) {
        $resolved = [IO.Path]::GetFullPath($artifact)
        $safeName =
            [IO.Path]::GetFileName($resolved) -like
                'ai-computer-toolkit-notepad-*.txt'
        if (-not $resolved.StartsWith(
                $tempRoot,
                [StringComparison]::OrdinalIgnoreCase
            ) -or
            -not $safeName) {
            throw 'Refusing to clean an unexpected text artifact.'
        }
        if (Test-Path -LiteralPath $resolved) {
            [IO.File]::Delete($resolved)
        }
    }
    foreach ($generatedFile in $generatedFiles) {
        $resolvedGenerated = [IO.Path]::GetFullPath($generatedFile)
        if (-not $resolvedGenerated.StartsWith(
            $resolvedFixture + [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            throw 'Refusing to clean a fixture outside the build directory.'
        }
        [IO.File]::Delete($resolvedGenerated)
    }
}

[pscustomobject]@{
    cppSessionId = $cppText[0].sessionId
    rustSessionId = $rustText[0].sessionId
    discoveryEquivalent = $true
    structuredAssessment = $true
    confirmationGate = $true
    staleTargetRefused = $true
    existingApplicationRefusedBeforeArtifact = $true
    atomicUtf8ReadbackEquivalent = $true
    foregroundUnchanged = $true
    facadeMapperSanitized = $true
    userDocumentsModified = 0
} | ConvertTo-Json
