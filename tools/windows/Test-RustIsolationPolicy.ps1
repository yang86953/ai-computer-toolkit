# 声明 Rust 严格隔离状态机的统一 launcher 门禁。
param(
    # 限制读取的进程数量。
    [ValidateRange(1, 4096)] [int]$MaximumProcesses = 4096,
    # 限制读取的窗口数量。
    [ValidateRange(1, 4096)] [int]$MaximumWindows = 256
)

# 启用严格变量与属性访问。
Set-StrictMode -Version Latest
# 让任何门禁错误立即终止。
$ErrorActionPreference = 'Stop'
# 禁止进度记录污染 JSON 输出。
$ProgressPreference = 'SilentlyContinue'

# 从脚本目录解析仓库根。
$repository = Split-Path -Parent $PSScriptRoot
# 固定唯一 computer-control launcher。
$launcher = Join-Path $repository 'tools\Invoke-ComputerControl.ps1'
# launcher 缺失时禁止绕过到 executable。
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    # 返回稳定环境错误。
    throw 'Computer-control launcher is missing.'
}

# 通过唯一 launcher 执行一次分词后的 CLI 请求。
function Invoke-Launcher {
    # 声明函数参数。
    param(
        # 禁止调用方传入 shell 命令字符串。
        [Parameter(Mandatory)] [string[]]$Arguments
    )
    # 调用正式 launcher。
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $launcher @Arguments
    # 保存结构化退出码。
    $exitCode = $LASTEXITCODE
    # 合并唯一 JSON 输出文本。
    $raw = $output -join "`n"
    # 空输出违反进程间协议。
    if ([string]::IsNullOrWhiteSpace($raw)) {
        # 禁止把空结果当作错误门禁通过。
        throw 'Launcher returned no JSON output.'
    }
    # 解析唯一 JSON value。
    $value = $raw | ConvertFrom-Json
    # 返回不含内部路径的测试外壳。
    return [pscustomobject]@{
        # 保存退出码。
        ExitCode = $exitCode
        # 保存解析结果。
        Value = $value
        # 保存用于字段名扫描的原始 JSON。
        Raw = $raw
    }
}

# 验证一个严格请求以预期错误失败。
function Assert-StrictFailure {
    # 声明函数参数。
    param(
        # 接收分词参数。
        [Parameter(Mandatory)] [string[]]$Arguments,
        # 接收稳定预期错误码。
        [Parameter(Mandatory)] [string]$ExpectedCode
    )
    # 执行不会到达 provider 的严格请求。
    $result = Invoke-Launcher -Arguments $Arguments
    # 退出码必须为结构化失败。
    if ($result.ExitCode -eq 0 -or
        # 顶层错误必须存在。
        $null -eq $result.Value.error -or
        # 错误码必须精确匹配。
        $result.Value.error.code -ne $ExpectedCode) {
        # 禁止错误顺序或执行路径漂移。
        throw "Strict isolation did not return $ExpectedCode before provider dispatch."
    }
    # 返回供隐私扫描使用的失败结果。
    return $result
}

# 以严格模式读取有界进程目录，证明 host-headless 成功路径。
$headless = Invoke-Launcher -Arguments @(
    # 调用只读 session 发现。
    'sessions',
    # 指定进程 surface。
    'process',
    # 设置硬数量边界。
    '--max-items',
    # 转换为十进制文本。
    [string]$MaximumProcesses,
    # 要求严格零打扰。
    '--strict-isolation'
)
# 主机无头读取必须成功。
if ($headless.ExitCode -ne 0 -or
    # 必须返回至少一个当前进程事实。
    [int]$headless.Value.count -le 0 -or
    # 实际 realm 必须为主机无头。
    $headless.Value.executionRealm -ne 'host-headless' -or
    # 要求 realm 必须一致。
    $headless.Value.requiredExecutionRealm -ne 'host-headless' -or
    # System 必须认证 realm。
    -not $headless.Value.executionRealmCertified -or
    # 请求必须保持严格。
    $headless.Value.isolationRequirement -ne 'strict' -or
    # 主机策略必须为零打扰。
    $headless.Value.hostImpactPolicy -ne 'strict-no-interference') {
    # 任一字段漂移使门禁失败。
    throw 'Strict host-headless execution evidence is invalid.'
}

# 读取当前窗口以取得公开 canonical 目标。
$windows = Invoke-Launcher -Arguments @(
    # 调用只读窗口 session 发现。
    'sessions',
    # 指定窗口 surface。
    'window',
    # 设置窗口数量边界。
    '--max-items',
    # 转换为十进制文本。
    [string]$MaximumWindows
)
# 窗口读取必须成功且非空。
if ($windows.ExitCode -ne 0 -or @($windows.Value.sessions).Count -eq 0) {
    # 当前环境无法完成隔离 worker 实证。
    throw 'No visible titled window is available for strict worker verification.'
}
# 只从公开目录选择首个 opaque 目标。
$target = [string]@($windows.Value.sessions)[0].sessionId
# 目标必须是 canonical s2:w。
if ($target -cnotmatch '^s2:w:[0-9a-f]{16}$') {
    # 禁止重建 HWND、PID 或其他身份。
    throw 'Window target is not canonical.'
}

# 严格执行只读 UIA root inspect，证明真实 companion worker 成功路径。
$worker = Invoke-Launcher -Arguments @(
    # 调用只读精确检查。
    'inspect',
    # 指定 UIA surface。
    'uia',
    # 指定公开目标选项。
    '--target',
    # 只传 canonical opaque ID。
    "sessionId=$target",
    # 要求严格零打扰。
    '--strict-isolation'
)
# 隔离 worker 读取必须成功。
if ($worker.ExitCode -ne 0 -or
    # 结果必须为只读。
    -not $worker.Value.readOnly -or
    # 结果必须证明前景不变。
    -not $worker.Value.foregroundUnchanged -or
    # 实际 realm 必须为隔离 worker。
    $worker.Value.executionRealm -ne 'isolated-worker' -or
    # 要求 realm 必须一致。
    $worker.Value.requiredExecutionRealm -ne 'isolated-worker' -or
    # System 必须认证 realm。
    -not $worker.Value.executionRealmCertified -or
    # 请求必须保持严格。
    $worker.Value.isolationRequirement -ne 'strict' -or
    # 主机策略必须为零打扰。
    $worker.Value.hostImpactPolicy -ne 'strict-no-interference') {
    # 任一证据漂移使门禁失败。
    throw 'Strict isolated-worker execution evidence is invalid.'
}

# 严格执行有界 accessibility tree，证明专用入口也进入同一 Policy Module。
$tree = Invoke-Launcher -Arguments @(
    # 调用有界树读取。
    'inspect-tree',
    # 指定 UIA surface。
    'uia',
    # 指定公开目标选项。
    '--target',
    # 只传 canonical opaque ID。
    "sessionId=$target",
    # 限制树深度。
    '--max-depth',
    # 只读取根与一层子节点。
    '1',
    # 限制节点数量。
    '--max-items',
    # 保持门禁输出有界。
    '32',
    # 使用只读 control view。
    '--view',
    # 指定固定 view。
    'control',
    # 设置 worker deadline。
    '--timeout-ms',
    # 使用五秒硬边界。
    '5000',
    # 要求严格零打扰。
    '--strict-isolation'
)
# 树读取必须成功并携带相同策略证明。
if ($tree.ExitCode -ne 0 -or
    # 结果必须保持只读。
    -not $tree.Value.readOnly -or
    # 结果必须证明前景不变。
    -not $tree.Value.foregroundUnchanged -or
    # 实际域必须为隔离 worker。
    $tree.Value.executionRealm -ne 'isolated-worker' -or
    # 要求域必须一致。
    $tree.Value.requiredExecutionRealm -ne 'isolated-worker' -or
    # System 必须认证 realm。
    -not $tree.Value.executionRealmCertified -or
    # 请求必须保持严格。
    $tree.Value.isolationRequirement -ne 'strict' -or
    # 主机策略必须为零打扰。
    $tree.Value.hostImpactPolicy -ne 'strict-no-interference') {
    # 任一证据漂移使门禁失败。
    throw 'Strict accessibility-tree execution evidence is invalid.'
}

# 已有前台同意也不得放宽同会话写路径。
$sameSession = Assert-StrictFailure -ExpectedCode 'ISOLATION_REQUIRED' -Arguments @(
    # 调用 run。
    'run',
    # 指定固定消息 surface。
    'win32-control',
    # 指定同会话无焦点 operation。
    'set-text',
    # 提供 mutation 确认。
    '--confirm',
    # 故意提供前台同意以证明它被忽略。
    '--allow-foreground',
    # 要求严格零打扰。
    '--strict-isolation'
)

# 尚未迁入 Rust companion 的隔离候选必须显式不可用。
$unavailable = Assert-StrictFailure -ExpectedCode 'ISOLATED_WORKER_UNAVAILABLE' -Arguments @(
    # 调用 run。
    'run',
    # 指定通用桌面 surface。
    'desktop',
    # 指定正式要求隔离 worker 的截图 operation。
    'screenshot',
    # 提供 mutation 确认。
    '--confirm',
    # 要求严格零打扰。
    '--strict-isolation'
)

# 在 worker 返回后再次读取完整进程目录。
$after = Invoke-Launcher -Arguments @(
    # 调用只读进程 session 发现。
    'sessions',
    # 指定进程 surface。
    'process',
    # 使用完整硬边界。
    '--max-items',
    # 转换为十进制文本。
    [string]$MaximumProcesses,
    # 保持严格零打扰。
    '--strict-isolation'
)
# 后置读取必须成功。
if ($after.ExitCode -ne 0) {
    # 禁止跳过 worker 清理证明。
    throw 'Post-worker process discovery failed.'
}
# worker 文件名不得残留在当前公开进程目录。
$workerResidual = [regex]::Matches(
    # 使用结构化后置目录 JSON。
    $after.Raw,
    # 匹配固定 companion 名称。
    '(?i)ai-computer-toolkit-observation-worker'
).Count
# 任一残留使门禁失败。
if ($workerResidual -ne 0) {
    # Job worker 必须在返回前回收。
    throw 'Observation worker remained after strict execution.'
}

# 合并所有公开结果执行原生身份字段扫描。
$serialized = @(
    # 加入主机无头成功结果。
    $headless.Value,
    # 加入 worker 成功结果。
    $worker.Value,
    # 加入 accessibility tree 成功结果。
    $tree.Value,
    # 加入同会话拒绝结果。
    $sameSession.Value,
    # 加入 worker 不可用结果。
    $unavailable.Value
) | ConvertTo-Json -Depth 50 -Compress
# 固定禁止的原生身份字段名。
$nativeIdentityLeaks = [regex]::Matches(
    # 扫描完整公开 JSON。
    $serialized,
    # 只匹配 JSON key，避免误判普通消息文本。
    '(?i)"(?:pid|processId|hwnd|handle|nativeId|runtimeId|aumid|executablePath|providerId)"\s*:'
).Count
# 任一泄漏使门禁失败。
if ($nativeIdentityLeaks -ne 0) {
    # 公共严格策略证据不得包含原生身份。
    throw 'Strict isolation evidence leaked a native identity field.'
}

# 输出单一有界门禁摘要。
[ordered]@{
    # 标记门禁成功。
    ok = $true
    # 声明唯一运行边界。
    boundary = 'tools/Invoke-ComputerControl.ps1'
    # 输出主机无头域。
    hostHeadlessRealm = $headless.Value.executionRealm
    # 输出隔离 worker 域。
    isolatedWorkerRealm = $worker.Value.executionRealm
    # 输出三个成功入口都已认证。
    certifiedSuccesses = @(
        # 主机无头成功认证。
        $headless.Value.executionRealmCertified,
        # 隔离 worker 成功认证。
        $worker.Value.executionRealmCertified,
        # accessibility tree 成功认证。
        $tree.Value.executionRealmCertified
    )
    # 输出同会话拒绝错误。
    sameSessionError = $sameSession.Value.error.code
    # 输出未认证 worker 拒绝错误。
    unavailableWorkerError = $unavailable.Value.error.code
    # 输出前景不变证据。
    foregroundUnchanged = $worker.Value.foregroundUnchanged
    # 输出 worker 残留数量。
    workerResiduals = $workerResidual
    # 输出原生身份泄漏数量。
    nativeIdentityLeaks = $nativeIdentityLeaks
} | ConvertTo-Json -Depth 5
