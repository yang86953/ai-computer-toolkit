# 声明脚本支持跳过重复构建。
[CmdletBinding()]
# 接受可选构建开关。
param(
    # 已完成 cargo build 时跳过构建。
    [switch] $SkipBuild
)

# 让断言或系统错误立即终止门禁。
$ErrorActionPreference = 'Stop'
# 解析仓库根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位 Rust 主程序。
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
# 定位逐 capability launcher。
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 定位版本化迁移策略。
$policyPath = Join-Path $projectRoot (
    'tests\contracts\text-document-create-compatibility-policy.json'
)
# 把夹具限制在项目 build 目录。
$buildRoot = Join-Path $projectRoot 'build'
# 建立本次唯一夹具目录。
$fixtureRoot = Join-Path $buildRoot (
    "rust-text-document-$([guid]::NewGuid().ToString('N'))"
)
# 规范化 build 根目录。
$resolvedBuild = [IO.Path]::GetFullPath($buildRoot)
# 规范化夹具目录。
$resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
# 拒绝逃逸出 build 根目录的夹具。
if (-not $resolvedFixture.StartsWith(
    $resolvedBuild + [IO.Path]::DirectorySeparatorChar,
    [StringComparison]::OrdinalIgnoreCase
)) {
    # 停止不安全的文件操作。
    throw 'Rust text document fixture escaped the build directory.'
}
# 创建唯一夹具目录。
New-Item -ItemType Directory -Path $resolvedFixture -Force |
    Out-Null
# 定位请求文件。
$requestPath = Join-Path $resolvedFixture 'request.json'
# 定位 stale 请求文件。
$stalePath = Join-Path $resolvedFixture 'stale.json'
# 定位 stdout 文件。
$stdoutPath = Join-Path $resolvedFixture 'stdout.json'
# 定位 stderr 文件。
$stderrPath = Join-Path $resolvedFixture 'stderr.txt'
# 定位伪 Notepad fixture。
$namedFixture = Join-Path $resolvedFixture 'Notepad.exe'
# 保存本次成功创建的 artifact 路径。
$artifactPath = $null
# 保存现有进程 fixture 句柄。
$existingProcess = $null

# 安全读取当前工具命名 artifact 集合。
function Get-ToolkitArtifacts {
    # 只在系统临时目录按固定前缀枚举。
    return @(
        Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) `
            -Filter 'ai-computer-toolkit-notepad-*.txt' `
            -File `
            -ErrorAction SilentlyContinue |
            # 只返回规范化完整路径。
            ForEach-Object { $_.FullName }
    )
}

# 只清理精确工具自有 artifact。
function Remove-OwnedArtifact {
    # 接收待清理路径。
    param([Parameter(Mandatory)][string] $Path)
    # 规范化系统临时目录。
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    # 规范化候选路径。
    $resolved = [IO.Path]::GetFullPath($Path)
    # 读取候选文件名。
    $name = [IO.Path]::GetFileName($resolved)
    # 要求路径位于系统临时目录且匹配固定前缀。
    if (-not $resolved.StartsWith(
        $tempRoot,
        [StringComparison]::OrdinalIgnoreCase
    ) -or $name -notlike 'ai-computer-toolkit-notepad-*.txt') {
        # 拒绝删除未知路径。
        throw 'Refusing to clean an unexpected text artifact.'
    }
    # 文件仍存在时删除本次 artifact。
    if (Test-Path -LiteralPath $resolved -PathType Leaf) {
        # 删除可重新生成的测试 artifact。
        Remove-Item -LiteralPath $resolved -Force
    }
}

# 终止只打开本次唯一 artifact 的 Notepad。
function Stop-OwnedNotepad {
    # 接收精确 artifact 路径。
    param([Parameter(Mandatory)][string] $Path)
    # 使用唯一文件名匹配命令行。
    $name = [IO.Path]::GetFileName($Path)
    # 查找固定进程名且命令行含本次文件名的进程。
    $owned = @(
        Get-CimInstance Win32_Process |
            Where-Object {
                $_.Name -eq 'Notepad.exe' -and
                $_.CommandLine -like "*$name*"
            }
    )
    # 逐个终止严格命中的测试进程。
    foreach ($process in $owned) {
        # 只使用已验证进程 ID。
        Stop-Process -Id ([int]$process.ProcessId) -Force
    }
}

# 通过独立进程执行 app create 并保留原始 stdio/exit 证据。
function Invoke-RustCreate {
    # 接收请求路径。
    param([Parameter(Mandatory)][string] $InputPath)
    # 拒绝会破坏固定命令行引号的路径。
    if ($InputPath.Contains('"')) {
        # 停止不安全夹具路径。
        throw 'Rust text document request path contains a quote.'
    }
    # 建立不经过 shell 的进程启动配置。
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    # 固定 Rust executable。
    $startInfo.FileName = $rust
    # 使用受控参数与已验证引号路径。
    $startInfo.Arguments =
        "run app create --input `"$InputPath`" --confirm"
    # 禁止 shell 解析。
    $startInfo.UseShellExecute = $false
    # 捕获唯一 JSON stdout。
    $startInfo.RedirectStandardOutput = $true
    # 捕获 stderr 泄漏。
    $startInfo.RedirectStandardError = $true
    # 不创建 CLI 控制台窗口。
    $startInfo.CreateNoWindow = $true
    # 建立进程对象。
    $process = [Diagnostics.Process]::new()
    # 应用固定启动信息。
    $process.StartInfo = $startInfo
    # 启动失败时停止门禁。
    if (-not $process.Start()) {
        # 返回明确 harness 错误。
        throw 'Rust text document CLI could not start.'
    }
    # 立即读取有界 stdout 直到 CLI 关闭管道。
    $stdout = $process.StandardOutput.ReadToEnd()
    # 读取有界 stderr 直到 CLI 关闭管道。
    $stderr = $process.StandardError.ReadToEnd()
    # 只等待 CLI 进程对象。
    $cliExited = $process.WaitForExit(10000)
    # 超过十秒仍未退出时只终止 CLI 自身并失败。
    if (-not $cliExited) {
        # 终止失控 CLI。
        Stop-Process -Id $process.Id -Force
        # 报告有界等待失败。
        throw 'Rust text document CLI did not exit within 10 seconds.'
    }
    # 保存退出码。
    $exitCode = $process.ExitCode
    # 回收 PowerShell process wrapper。
    $process.Dispose()
    # 返回原始证据对象。
    return [pscustomobject]@{
        # 保存退出码。
        ExitCode = $exitCode
        # 保存 stdout。
        Stdout = $stdout
        # 保存 stderr。
        Stderr = $stderr
        # 解析唯一 JSON envelope。
        Json = $stdout | ConvertFrom-Json
    }
}

# 在未显式跳过时构建 Rust 主程序。
if (-not $SkipBuild) {
    # 使用仓库固定 Cargo manifest 构建。
    cargo build --quiet --manifest-path (
        Join-Path $projectRoot 'Cargo.toml'
    )
    # 构建失败时停止门禁。
    if ($LASTEXITCODE -ne 0) {
        # 返回明确构建失败。
        throw 'Rust text document build failed.'
    }
}

# 拒绝在用户已有 Notepad 时执行真实写入门禁。
$preexisting = @(Get-Process Notepad -ErrorAction SilentlyContinue)
# 不得终止或附着用户已有实例。
if ($preexisting.Count -ne 0) {
    # 报告真实环境阻塞而非伪造成功。
    throw 'Rust text document gate requires no pre-existing Notepad process.'
}

# 使用受控 try/finally 清理所有测试所有物。
try {
    # 读取 Stage 4A 迁移状态契约。
    $policy = [IO.File]::ReadAllText($policyPath) |
        ConvertFrom-Json
    # 核对 Rust launcher authority 与 C++ 对照角色。
    if ($policy.rustStatus -ne 'available-confirmed' -or
        $policy.launcherAuthority -ne 'rust' -or
        $policy.cppRole -ne 'direct-compatibility-evidence') {
        # 停止迁移状态漂移。
        throw 'Text document migration policy does not select Rust authority.'
    }
    # 读取 Rust 发布的 app sessions。
    $sessions = & $rust sessions app --max-items 4096 |
        ConvertFrom-Json
    # 找到唯一文本创建器。
    $textSessions = @(
        $sessions.sessions |
            Where-Object {
                $_.capabilities.id -contains 'text.document.create@1'
            }
    )
    # 必须恰好发布一个 canonical s2:a 目标。
    if ($LASTEXITCODE -ne 0 -or
        $textSessions.Count -ne 1 -or
        $textSessions[0].sessionId -notlike 's2:a:*') {
        # 停止错误的 discovery 形状。
        throw 'Rust did not publish one canonical text document session.'
    }
    # 保存精确目标。
    $sessionId = [string]$textSessions[0].sessionId
    # 调用无副作用 assessment。
    $assessment = & $rust assess app `
        --capability text.document.create@1 `
        --target "sessionId=$sessionId" |
        ConvertFrom-Json
    # 核对确认型后台 realm。
    if ($LASTEXITCODE -ne 0 -or
        $assessment.decision -ne 'confirmation-required' -or
        $assessment.executionRealm -ne 'host-background' -or
        -not $assessment.requiresConfirmation -or
        $assessment.requiresForegroundConsent -or
        $assessment.constraints.readOnly) {
        # assessment 不得放松写入门禁。
        throw 'Rust text document assessment diverged from the contract.'
    }
    # 记录确认前 artifact 集合。
    $beforeUnconfirmed = Get-ToolkitArtifacts
    # 缺确认调用 legacy 路径。
    $unconfirmed = & $rust run notepad open-and-write-text `
        --arg text=unconfirmed |
        ConvertFrom-Json
    # 记录确认后 artifact 集合。
    $afterUnconfirmed = Get-ToolkitArtifacts
    # 确认错误必须先于写入。
    if ($unconfirmed.ok -or
        $unconfirmed.error.code -ne 'CONFIRMATION_REQUIRED' -or
        @($afterUnconfirmed | Where-Object {
            $_ -notin $beforeUnconfirmed
        }).Count -ne 0) {
        # 停止 confirmation-first 违规。
        throw 'Rust text document confirmation-first gate failed.'
    }
    # 构造 stale 精确目标请求。
    $staleRequest = [ordered]@{
        # 使用不存在的 canonical 目标。
        target = [ordered]@{ sessionId = 's2:a:0000000000000000' }
        # 提供合法 capability 和文本。
        args = [ordered]@{
            capability = 'text.document.create@1'
            input = [ordered]@{ text = 'stale' }
        }
        # 明确确认，确保测试目标解析。
        confirmed = $true
    }
    # 以无 BOM UTF-8 写入夹具。
    [IO.File]::WriteAllText(
        $stalePath,
        ($staleRequest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    # 调用 stale 路径。
    $stale = & $rust run app create --input $stalePath |
        ConvertFrom-Json
    # stale 目标不得进入 Module。
    if ($stale.ok -or
        $stale.error.code -notin @('TARGET_NOT_FOUND', 'STALE_SESSION')) {
        # 停止宽松目标解析。
        throw 'Rust text document accepted a stale target.'
    }
    # 复制当前 PowerShell 为仅供进程名门禁的自有 fixture。
    Copy-Item -LiteralPath (
        (Get-Process -Id $PID).Path
    ) -Destination $namedFixture
    # 启动固定名称且无子进程的睡眠 fixture。
    $existingProcess = Start-Process `
        -FilePath $namedFixture `
        -ArgumentList @(
            '-NoProfile'
            '-Command'
            'Start-Sleep -Seconds 30'
        ) `
        -WindowStyle Hidden `
        -PassThru
    # 等待进程进入快照。
    Start-Sleep -Milliseconds 250
    # 构造合法请求以验证写前拒绝。
    $existingRequest = [ordered]@{
        # 使用当前精确创建器。
        target = [ordered]@{ sessionId = $sessionId }
        # 使用合法输入。
        args = [ordered]@{
            capability = 'text.document.create@1'
            input = [ordered]@{ text = 'must-not-create' }
        }
        # 明确确认。
        confirmed = $true
    }
    # 写入合法请求夹具。
    [IO.File]::WriteAllText(
        $requestPath,
        ($existingRequest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    # 记录拒绝前 artifact 集合。
    $beforeExisting = Get-ToolkitArtifacts
    # 执行已有 Notepad 门禁。
    $existingResult = & $rust run app create --input $requestPath |
        ConvertFrom-Json
    # 记录拒绝后 artifact 集合。
    $afterExisting = Get-ToolkitArtifacts
    # 精确终止自有 fixture。
    Stop-Process -Id $existingProcess.Id -Force
    # 等待 fixture 退出。
    [void]$existingProcess.WaitForExit(5000)
    # 回收 wrapper。
    $existingProcess.Dispose()
    # 清除已回收标记。
    $existingProcess = $null
    # 验证已有实例在写入前拒绝。
    if ($existingResult.ok -or
        $existingResult.error.code -ne 'BACKGROUND_OPERATION_UNAVAILABLE' -or
        $existingResult.error.details.reason -ne
            'existing-application-session-attachment-not-certified' -or
        $existingResult.error.details.artifactCreated -or
        $existingResult.error.details.safeToRetryAutomatically -or
        @($afterExisting | Where-Object {
            $_ -notin $beforeExisting
        }).Count -ne 0) {
        # 停止既有应用附着违规。
        throw 'Rust existing-Notepad gate wrote before refusing attachment.'
    }
    # 构造唯一 Unicode 成功文本。
    $text = "Rust Stage 4A $([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())`nUTF-8 文本✓"
    # 构造成功请求。
    $request = [ordered]@{
        # 使用当前精确创建器。
        target = [ordered]@{ sessionId = $sessionId }
        # 提供 provider-neutral 输入。
        args = [ordered]@{
            capability = 'text.document.create@1'
            input = [ordered]@{ text = $text }
        }
        # 明确确认。
        confirmed = $true
    }
    # 写入成功请求夹具。
    [IO.File]::WriteAllText(
        $requestPath,
        ($request | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    # 独立执行并捕获 stdio。
    $execution = Invoke-RustCreate -InputPath $requestPath
    # 保存 JSON 结果。
    $result = $execution.Json
    # 成功 envelope 立即登记 artifact，确保后续任何断言失败仍可清理。
    if ($result.ok) {
        # 保存受控 artifact 路径。
        $artifactPath = [string]$result.data.path
    }
    # 成功必须 exit 0、stderr 空且 stdout 单 JSON。
    if ($execution.ExitCode -ne 0 -or
        -not [string]::IsNullOrWhiteSpace($execution.Stderr) -or
        -not $result.ok) {
        # 提取稳定错误码用于门禁诊断。
        $resultCode = [string]$result.error.code
        # 停止 stdio/exit 违规且不回显路径或输入。
        throw "Rust text document stdio contract failed: exit=$($execution.ExitCode), stderrBytes=$([Text.Encoding]::UTF8.GetByteCount($execution.Stderr)), code=$resultCode."
    }
    # 核对 provider-neutral 结果与磁盘回读。
    if ($result.data.text -cne $text -or
        $result.data.encoding -ne 'utf-8' -or
        $result.data.mediaType -ne 'text/plain' -or
        -not $result.meta.foreground.unchanged -or
        -not (Test-Path -LiteralPath $artifactPath -PathType Leaf) -or
        [IO.File]::ReadAllText(
            $artifactPath,
            [Text.Encoding]::UTF8
        ) -cne $text) {
        # 停止 artifact 或前台证据不一致。
        throw 'Rust text document artifact verification failed.'
    }
    # 序列化公共结果以检查原生字段泄漏。
    $serialized = $result | ConvertTo-Json -Depth 12 -Compress
    # facade 不得公开 provider、PID、handle 或 executable。
    if ($serialized -match
        'launcherProcessId|notepadPath|hwnd|processId|System32') {
        # 停止公共泄漏。
        throw 'Rust text document facade leaked native provider details.'
    }
    # 只终止命令行含本次 artifact 的测试 Notepad。
    Stop-OwnedNotepad -Path $artifactPath
    # 删除本次可重新生成 artifact。
    Remove-OwnedArtifact -Path $artifactPath
    # 清除已清理路径。
    $artifactPath = $null
    # 通过生产 launcher 再执行同一 Rust capability。
    $launcherResult = & powershell `
        -NoProfile `
        -ExecutionPolicy Bypass `
        -File $launcher `
        run app create `
        --input $requestPath `
        --confirm |
        ConvertFrom-Json
    # 保存 launcher 退出码。
    $launcherExitCode = $LASTEXITCODE
    # 成功后立即登记第二个测试 artifact。
    if ($launcherResult.ok) {
        # 保存 launcher artifact 供统一 finally 清理。
        $artifactPath = [string]$launcherResult.data.path
    }
    # 核对 launcher 结果与直接 Rust 结果的安全形状。
    if ($launcherExitCode -ne 0 -or
        -not $launcherResult.ok -or
        $launcherResult.data.text -cne $text -or
        -not $launcherResult.meta.foreground.unchanged -or
        -not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
        # 停止生产路由差异。
        throw 'Production launcher did not execute the Rust text document capability.'
    }
    # 只终止打开第二个唯一 artifact 的测试 Notepad。
    Stop-OwnedNotepad -Path $artifactPath
    # 删除第二个可重新生成 artifact。
    Remove-OwnedArtifact -Path $artifactPath
    # 清除已清理路径。
    $artifactPath = $null
    # 输出机器可读门禁摘要。
    [ordered]@{
        # 标记整体成功。
        ok = $true
        # 输出精确目标。
        sessionId = $sessionId
        # 输出 assessment 决策。
        decision = $assessment.decision
        # 确认门禁通过。
        confirmationFirst = $true
        # stale 门禁通过。
        staleRefused = $true
        # 既有实例写前拒绝。
        existingApplicationRefusedBeforeArtifact = $true
        # 原子 UTF-8 回读通过。
        atomicUtf8Readback = $true
        # 前台保持不变。
        foregroundUnchanged = $true
        # 公共字段已净化。
        facadeSanitized = $true
        # 生产 launcher 已执行 Rust 成功路径。
        launcherRustRoute = $true
        # 未修改用户文档。
        userDocumentsModified = 0
    } | ConvertTo-Json -Compress
# 无论断言结果如何都只清理本测试所有物。
} finally {
    # 若 fixture 仍运行则只终止其精确 PID。
    if ($null -ne $existingProcess -and
        -not $existingProcess.HasExited) {
        # 终止自有命名 fixture。
        Stop-Process -Id $existingProcess.Id -Force
        # 有界等待退出。
        [void]$existingProcess.WaitForExit(5000)
    }
    # 回收 fixture wrapper。
    if ($null -ne $existingProcess) {
        # 释放 PowerShell 对象。
        $existingProcess.Dispose()
    }
    # 若成功 artifact 尚未清理则先终止其精确 Notepad。
    if (-not [string]::IsNullOrWhiteSpace($artifactPath)) {
        # 只终止打开该唯一文件的进程。
        Stop-OwnedNotepad -Path $artifactPath
        # 删除受控 artifact。
        Remove-OwnedArtifact -Path $artifactPath
    }
    # 逐个删除夹具目录中的已知文件。
    foreach ($file in @(
        $requestPath,
        $stalePath,
        $stdoutPath,
        $stderrPath,
        $namedFixture
    )) {
        # 只允许删除规范化夹具子路径。
        $resolvedFile = [IO.Path]::GetFullPath($file)
        # 拒绝任何逃逸路径。
        if (-not $resolvedFile.StartsWith(
            $resolvedFixture + [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            # 停止不安全清理。
            throw 'Rust text document cleanup escaped the fixture directory.'
        }
        # 删除存在的已知夹具文件。
        if (Test-Path -LiteralPath $resolvedFile -PathType Leaf) {
            # 文件均为本测试生成的可恢复夹具。
            Remove-Item -LiteralPath $resolvedFile -Force
        }
    }
    # 空目录存在时仅删除该精确目录，不递归。
    if (Test-Path -LiteralPath $resolvedFixture -PathType Container) {
        # 删除已验证的空夹具目录。
        Remove-Item -LiteralPath $resolvedFixture -Force
    }
}
