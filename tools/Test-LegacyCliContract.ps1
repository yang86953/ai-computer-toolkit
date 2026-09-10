[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$casesPath = Join-Path $projectRoot 'tests\contracts\legacy-cli-cases.json'
$executable = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'

& cargo build --manifest-path (Join-Path $projectRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    throw "Rust compatibility CLI build failed with $LASTEXITCODE."
}

$cases = Get-Content -LiteralPath $casesPath -Raw | ConvertFrom-Json
$results = foreach ($case in $cases) {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $executable
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $commandArguments = @($case.argv | ForEach-Object { [string] $_ })
    foreach ($argument in $commandArguments) {
        if ($argument -match '[\s"]') {
            throw "$($case.name) contains an argument requiring unsupported command-line quoting."
        }
    }
    $startInfo.Arguments = $commandArguments -join ' '

    $process = [System.Diagnostics.Process]::Start($startInfo)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()

    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        throw "$($case.name) wrote unexpected stderr: $stderr"
    }
    try {
        $json = $stdout | ConvertFrom-Json
    } catch {
        throw "$($case.name) did not write one valid JSON result: $stdout"
    }
    if ($process.ExitCode -ne $case.expectedExitCode) {
        throw "$($case.name) exit code $($process.ExitCode), expected $($case.expectedExitCode)."
    }
    if ($json.ok -ne $case.expectedOk) {
        throw "$($case.name) ok=$($json.ok), expected $($case.expectedOk)."
    }
    if (
        $null -ne $case.expectedErrorCode -and
        $json.error.code -ne $case.expectedErrorCode
    ) {
        throw "$($case.name) error=$($json.error.code), expected $($case.expectedErrorCode)."
    }

    [PSCustomObject]@{
        name = $case.name
        exitCode = $process.ExitCode
        ok = $json.ok
        errorCode = $json.error.code
    }
}

$results | ConvertTo-Json -Depth 4
