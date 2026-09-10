# 声明动态应用启动只读与未确认门禁。
param(
    # 限制动态应用发现数量。
    [ValidateRange(1, 4096)] [int]$MaximumApplications = 512,
    # 限制进程发现数量。
    [ValidateRange(1, 4096)] [int]$MaximumProcesses = 512,
    # 限制窗口发现数量。
    [ValidateRange(1, 4096)] [int]$MaximumWindows = 256
)

# 启用严格变量与属性访问。
Set-StrictMode -Version Latest
# 让任何门禁错误立即终止。
$ErrorActionPreference = 'Stop'
# 禁止 PowerShell 进度污染 JSON 输出。
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

# 通过唯一 launcher 执行成功只读命令。
function Invoke-LauncherRead {
    # 声明分词后的 CLI 参数。
    param(
        # 禁止调用方传入 shell 字符串。
        [Parameter(Mandatory)] [string[]]$Arguments
    )
    # 调用正式 launcher 而不是 Rust executable。
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $launcher @Arguments
    # 保存结构化退出码。
    $exitCode = $LASTEXITCODE
    # 只读命令必须成功。
    if ($exitCode -ne 0) {
        # 不重试或降级到其他运行时。
        throw "Launcher read failed with exit code $exitCode."
    }
    # 解析唯一 JSON 输出。
    return $output | ConvertFrom-Json
}

# 读取完整有界动态应用目录。
$inventory = Invoke-LauncherRead -Arguments @(
    # 调用动态关系图发现。
    'discover',
    # 指定 app surface。
    'app',
    # 指定应用上限。
    '--max-applications',
    # 转换为稳定十进制文本。
    [string]$MaximumApplications,
    # 指定进程上限。
    '--max-processes',
    # 转换为稳定十进制文本。
    [string]$MaximumProcesses,
    # 指定窗口上限。
    '--max-windows',
    # 转换为稳定十进制文本。
    [string]$MaximumWindows
)
# 只选择单独认证的动态应用目标。
$launchable = @(
    # 遍历当前公开应用记录。
    $inventory.data.applications | Where-Object {
        # 要求确认型启动状态。
        $_.launchCapability -eq 'available-confirmed'
    }
)
# 缺少目标时不能把门禁误报为通过。
if ($launchable.Count -eq 0) {
    # 返回可定位环境错误。
    throw 'No certified dynamic application target was discovered.'
}
# 只选择公开标记固定 Start Menu 来源的认证目标。
$startMenuLaunchable = @(
    # 遍历全部确认型目标。
    $launchable | Where-Object {
        # 要求公开来源集合包含固定标签而不检查任何路径。
        @($_.discoverySources) -contains 'shell-start-menu'
    }
)
# 缺少固定 Start Menu 目标时不能宣称新增来源接入生产 launcher。
if ($startMenuLaunchable.Count -eq 0) {
    # 返回不含应用名称或私有 identity 的环境错误。
    throw 'No certified Start Menu application target was discovered.'
}
# 只从公开固定来源集合取得首个 opaque 目标。
$target = [string]$startMenuLaunchable[0].sessionId
# 目标必须是 canonical s2:a。
if ($target -cnotmatch '^s2:a:[0-9a-f]{16}$') {
    # 禁止重建 AUMID、路径或其他 identity。
    throw 'Dynamic application target is not canonical.'
}

# 精确检查当前目标的公开认证状态。
$inspection = Invoke-LauncherRead -Arguments @(
    # 调用 inspect verb。
    'inspect',
    # 指定统一 app surface。
    'app',
    # 指定精确目标选项。
    '--target',
    # 只传公开 opaque ID。
    "sessionId=$target"
)
# inspect 必须保持确认型能力状态。
if ($inspection.data.application.launchCapability -ne 'available-confirmed') {
    # 认证状态漂移使门禁失败。
    throw 'Dynamic application inspection is not launchable.'
}

# 读取同一目标的 capability assessment。
$assessment = Invoke-LauncherRead -Arguments @(
    # 调用无副作用 assessment。
    'assess',
    # 指定统一 app surface。
    'app',
    # 指定稳定 capability。
    '--capability',
    # 使用 application.open 版本一。
    'application.open@1',
    # 指定精确目标选项。
    '--target',
    # 只传公开 opaque ID。
    "sessionId=$target"
)
# assessment 必须要求逐操作确认。
if ($assessment.decision -ne 'confirmation-required' -or
    # 启动应用由其自行呈现前景。
    $assessment.executionRealm -ne 'host-foreground' -or
    # 明确核对确认标志。
    -not $assessment.requiresConfirmation) {
    # 决策或 realm 漂移使门禁失败。
    throw 'Dynamic application launch assessment drifted.'
}

# 构造故意省略确认的分词参数。
$failureArguments = @(
    # 调用 generic run verb。
    'run',
    # 指定统一 app surface。
    'app',
    # application.open 使用 create 动作。
    'create',
    # 指定精确目标选项。
    '--target',
    # 传入当前公开应用目标。
    "sessionId=$target",
    # 指定 capability 参数选项。
    '--arg',
    # 不传 path、argv 或 identity。
    'capability=application.open@1'
)
# 故意省略确认执行写命令。
$failureText = & powershell -NoProfile -ExecutionPolicy Bypass -File $launcher @failureArguments
# 保存未确认调用退出码。
$failureCode = $LASTEXITCODE
# 解析结构化拒绝结果。
$failure = $failureText | ConvertFrom-Json
# 未确认必须在 provider 解析前失败。
if ($failureCode -eq 0 -or
    # 成功标志必须为假。
    $failure.ok -or
    # 使用稳定确认错误码。
    $failure.error.code -ne 'CONFIRMATION_REQUIRED') {
    # 禁止未确认启动或错误顺序漂移。
    throw 'Unconfirmed dynamic application launch did not fail first.'
}

# 合并三个公开结果执行隐私扫描。
$serialized = @($inspection, $assessment, $failure) |
    # 使用足够深度序列化全部字段。
    ConvertTo-Json -Depth 50 -Compress
# 固定禁止的私有身份字段名。
foreach ($field in @('aumid', 'path', 'executablePath', 'runtimePath', 'pid', 'processId', 'providerId')) {
    # 只匹配 JSON key 而不是普通文本。
    if ($serialized -match ('(?i)"' + [Regex]::Escape($field) + '"\s*:')) {
        # 不输出可能包含私有值的原始 JSON。
        throw "Dynamic application gate leaked field '$field'."
    }
}

# 输出不含目标内容的机器可读摘要。
[ordered]@{
    # 标记门禁通过。
    ok = $true
    # 声明唯一运行边界。
    boundary = 'tools/Invoke-ComputerControl.ps1'
    # 声明本门禁只读或拒绝。
    readOnlyAndDeniedOnly = $true
    # 输出动态应用总数。
    discoveredApplications = $inventory.data.counts.installedApplications
    # 输出单独认证目标数。
    launchableApplications = $launchable.Count
    # 输出固定 Start Menu 认证目标数。
    startMenuLaunchableApplications = $startMenuLaunchable.Count
    # 声明精确检查使用的公开固定来源标签。
    testedSource = 'shell-start-menu'
    # 标记目标格式通过。
    canonicalApplicationTarget = $true
    # 输出 assessment 决策。
    assessmentDecision = $assessment.decision
    # 输出 execution realm。
    assessmentRealm = $assessment.executionRealm
    # 输出未确认错误码。
    unconfirmedError = $failure.error.code
    # 明确本门禁未调度启动。
    launchesDispatched = 0
    # 明确私有字段泄漏为零。
    nativeIdentityLeaks = 0
# 使用稳定 JSON 输出。
} | ConvertTo-Json -Depth 4
