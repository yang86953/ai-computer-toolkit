$ErrorActionPreference = 'Stop'
$CommandArguments = @($args)
$projectRoot = Split-Path -Parent $PSScriptRoot
# 生产 launcher 只定位唯一受支持的 Rust runtime。
$rust = Join-Path (
    Join-Path $projectRoot 'target\debug'
) 'ai-computer-toolkit.exe'

function Write-LauncherFailure {
    param(
        [Parameter(Mandatory)]
        [string] $Code,
        [Parameter(Mandatory)]
        [string] $Message
    )
    [ordered]@{
        ok = $false
        error = [ordered]@{
            code = $Code
            message = $Message
        }
    } | ConvertTo-Json -Compress
}

if ($CommandArguments.Count -eq 0) {
    Write-LauncherFailure `
        -Code 'INVALID_ARGUMENT' `
        -Message 'A command is required.'
    exit 2
}

# 缺少唯一 Rust runtime 时必须稳定失败闭合。
if (-not (Test-Path -LiteralPath $rust)) {
    # 通过统一失败外壳输出不暴露本机路径的结构化 JSON。
    Write-LauncherFailure `
        -Code 'COMPATIBILITY_RUNTIME_UNAVAILABLE' `
        -Message 'The Rust compatibility runtime is not built.'
    exit 2
}

# 所有命令不经请求内容分流，直接交给 Rust 主入口。
& $rust @CommandArguments
exit $LASTEXITCODE
