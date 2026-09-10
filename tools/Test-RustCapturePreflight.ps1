# 声明脚本参数。
[CmdletBinding()]
# 接收可跳过既有构建的开关。
param([switch] $SkipBuild)

# 启用严格变量与属性访问。
Set-StrictMode -Version Latest
# 任何 PowerShell 错误立即终止门禁。
$ErrorActionPreference = 'Stop'
# 定位仓库根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位统一 computer-control launcher。
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 定位 Rust 主程序。
$rustExecutable = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
# 定位预检策略。
$policyPath = Join-Path $projectRoot 'tests\contracts\capture-preflight-policy.json'
# 定位 Rust 零帧 Windows Component。
$componentPath = Join-Path $projectRoot `
    'src\adapters\window_capture_preflight_windows.rs'

# 默认构建最新 Rust 主程序。
if (-not $SkipBuild) {
    # 优先使用 PATH 中的 cargo。
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    # PATH 缺失时使用用户 cargo 默认安装位置。
    if ($null -eq $cargo) {
        # 构造固定用户级 cargo 路径。
        $cargoPath = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
        # 固定路径也缺失时禁止继续使用旧二进制。
        if (-not (Test-Path -LiteralPath $cargoPath -PathType Leaf)) {
            # 返回明确环境缺口。
            throw 'cargo is unavailable for the Rust capture preflight gate.'
        }
        # 使用已验证的绝对路径。
        $cargo = $cargoPath
    }
    # 构建 Rust 主程序。
    & $cargo build --quiet --manifest-path (
        # 使用仓库权威 Cargo manifest。
        Join-Path $projectRoot 'Cargo.toml'
    )
    # 构建失败立即终止。
    if ($LASTEXITCODE -ne 0) {
        # 返回稳定构建错误。
        throw "Rust build failed with exit code $LASTEXITCODE."
    }
}

# Rust 主程序必须存在。
if (-not (Test-Path -LiteralPath $rustExecutable -PathType Leaf)) {
    # 禁止 launcher 对缺失 runtime 静默回退。
    throw 'The Rust main executable is not built.'
}
# launcher 必须存在。
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    # 禁止绕过统一 facade。
    throw 'Computer-control launcher is missing.'
}
# 读取权威预检策略。
$policy = Get-Content -LiteralPath $policyPath -Raw | ConvertFrom-Json

# 通过唯一 launcher 调用公开 capability 并解析 JSON。
function Invoke-ToolkitJson {
    # 声明固定参数。
    param(
        # 接收已经分词的 CLI 参数。
        [Parameter(Mandatory)] [string[]] $Arguments,
        # 接收预期退出码。
        [int] $ExpectedExitCode = 0
    )
    # 从独立 PowerShell 进程执行统一 launcher。
    $output = & powershell -NoProfile -ExecutionPolicy Bypass `
        -File $launcher @Arguments
    # 保存结构化命令退出码。
    $exitCode = $LASTEXITCODE
    # 退出码必须与调用场景一致。
    if ($exitCode -ne $ExpectedExitCode) {
        # 不回退到第二控制面。
        throw "Unexpected launcher exit code $exitCode."
    }
    # 唯一 stdout 必须是 JSON。
    return $output | ConvertFrom-Json
}

# 递归结果通过序列化键扫描验证无原生字段。
function Assert-NoNativeField {
    # 声明待扫描值。
    param([Parameter(Mandatory)] [object] $Value)
    # 序列化完整结果。
    $json = $Value | ConvertTo-Json -Depth 20 -Compress
    # 逐项扫描策略禁止字段。
    foreach ($field in $policy.forbiddenPublicFields) {
        # 构造精确 JSON key 模式。
        $pattern = '"' + [Regex]::Escape($field) + '"\s*:'
        # 任一命中立即失败。
        if ($json -match $pattern) {
            # 不回显可能包含敏感值的原始 JSON。
            throw "Capture preflight leaked forbidden field '$field'."
        }
    }
}

# 通过统一 app facade 读取当前 sessions。
$sessions = Invoke-ToolkitJson -Arguments @(
    # 使用只读 session 发现命令。
    'sessions',
    # 使用统一 app surface。
    'app',
    # 使用公开硬边界。
    '--max-items',
    # 覆盖当前桌面但保持有界。
    '100'
)
# 只保留 canonical 窗口目标，拒绝 application/document session 混入。
$windows = @(
    # 遍历统一 session 数组。
    $sessions.data.sessions |
        # 依据 opaque kind 前缀选择窗口。
        Where-Object { $_.sessionId -like 's2:w:*' }
)
# 实机门禁至少需要一个当前窗口。
if ($windows.Count -eq 0) {
    # 不以零样本误报通过。
    throw 'No canonical window session is available for capture preflight.'
}

# 初始化 eligibility 统计。
$eligibility = [ordered]@{}
# 初始化 WGC runtime 统计。
$wgcRuntime = [ordered]@{}
# 初始化 WGC item interop 统计。
$wgcItemInterop = [ordered]@{}
# 逐个验证当前 canonical 窗口。
foreach ($window in $windows) {
    # 通过 launcher 执行 Rust 预检。
    $result = Invoke-ToolkitJson -Arguments @(
        # 使用预检命令。
        'preflight-capture',
        # 使用唯一公开 surface。
        'app',
        # 提供 opaque 目标选项。
        '--target',
        # 只传 canonical sessionId。
        "sessionId=$($window.sessionId)"
    )
    # 核对 Rust control envelope 与只读常量。
    if (-not $result.ok -or
        # launcher 必须固定选择 Rust。
        $result.implementation -ne 'rust' -or
        # capability 必须匹配策略。
        $result.data.capability -ne $policy.capability -or
        # 预检必须只读。
        -not $result.data.readOnly -or
        # 成功路径必须通过前景门禁。
        -not $result.data.foregroundUnchanged -or
        # 预检不得要求前景。
        $result.data.foregroundRequired -or
        # 预检必须报告真实截图已经迁入 Rust worker。
        -not $result.data.captureExecutionMigrated -or
        # 禁止像素读取。
        $result.data.safety.pixelsRead -or
        # 禁止启动捕获 session。
        $result.data.safety.captureSessionStarted -or
        # 禁止创建帧池。
        $result.data.safety.framePoolCreated -or
        # 禁止写文件。
        $result.data.safety.fileWritten -or
        # 禁止激活窗口。
        $result.data.safety.windowActivated -or
        # 禁止发送输入。
        $result.data.safety.inputSent -or
        # 禁止公开原生目标。
        $result.data.safety.nativeTargetExposed) {
        # 任一常量漂移立即失败。
        throw 'Rust capture preflight violated its read-only safety envelope.'
    }
    # 内容保护必须属于封闭枚举。
    if ($policy.allowedContentProtection -notcontains `
            $result.data.evidence.contentProtection) {
        # 拒绝未知保护值。
        throw 'Rust capture preflight returned an unknown protection state.'
    }
    # WGC runtime 必须属于封闭枚举。
    if ($policy.allowedWgcRuntime -notcontains `
            $result.data.evidence.wgcRuntime) {
        # 拒绝未知 runtime 值。
        throw 'Rust capture preflight returned an unknown WGC runtime state.'
    }
    # WGC item interop 必须属于封闭枚举。
    if ($policy.allowedWgcItemInterop -notcontains `
            $result.data.evidence.wgcItemInterop) {
        # 拒绝未知 interop 值。
        throw 'Rust capture preflight returned an unknown WGC item state.'
    }
    # 扫描全部公开字段。
    Assert-NoNativeField -Value $result
    # 读取 eligibility 分类。
    $eligibilityName = [string] $result.data.evidence.eligibility
    # 初始化未出现分类。
    if (-not $eligibility.Contains($eligibilityName)) {
        # 写入零计数。
        $eligibility[$eligibilityName] = 0
    }
    # 增加 eligibility 计数。
    $eligibility[$eligibilityName]++
    # 读取 runtime 分类。
    $runtimeName = [string] $result.data.evidence.wgcRuntime
    # 初始化未出现 runtime。
    if (-not $wgcRuntime.Contains($runtimeName)) {
        # 写入零计数。
        $wgcRuntime[$runtimeName] = 0
    }
    # 增加 runtime 计数。
    $wgcRuntime[$runtimeName]++
    # 读取 item interop 分类。
    $itemName = [string] $result.data.evidence.wgcItemInterop
    # 初始化未出现 item 状态。
    if (-not $wgcItemInterop.Contains($itemName)) {
        # 写入零计数。
        $wgcItemInterop[$itemName] = 0
    }
    # 增加 item interop 计数。
    $wgcItemInterop[$itemName]++
}

# 验证 stale exact target 保持失败闭合。
$stale = Invoke-ToolkitJson -Arguments @(
    # 使用预检命令。
    'preflight-capture',
    # 使用 app surface。
    'app',
    # 提供 opaque 目标。
    '--target',
    # 使用确定不存在的 canonical 指纹。
    'sessionId=s2:w:0000000000000000'
# stale 必须返回通用失败退出码。
) -ExpectedExitCode 2
# 核对 stale 结构化错误。
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    # 禁止把 stale 误报为可用。
    throw 'Rust capture preflight did not preserve stale semantics.'
}

# 验证 assessment 与已迁移预检 realm 一致。
$assessment = Invoke-ToolkitJson -Arguments @(
    # 使用只读 assessment。
    'assess',
    # 使用 app surface。
    'app',
    # 指定预检 capability。
    '--capability',
    # 使用策略权威 ID。
    $policy.capability,
    # 提供 opaque 目标。
    '--target',
    # 使用首个当前窗口。
    "sessionId=$($windows[0].sessionId)"
)
# 核对后台可执行决策与 host-headless realm。
if (-not $assessment.ok -or
    # 只读预检必须可后台执行。
    $assessment.decision -ne 'executable-background' -or
    # 预检不得进入捕获 worker。
    $assessment.executionRealm -ne 'host-headless') {
    # assessment 漂移必须失败。
    throw 'Rust capture preflight assessment is not host-headless executable.'
}

# 静态保证 Rust Component 不调用会创建捕获资源或前景影响的 API。
$component = Get-Content -LiteralPath $componentPath -Raw
# 逐项扫描预检专属禁止 token。
foreach ($api in $policy.forbiddenPreflightTokens) {
    # 任一调用名出现即失败。
    if ($component.Contains($api)) {
        # 输出 API 名帮助定位，不回显源码。
        throw "Rust capture preflight uses forbidden API '$api'."
    }
}

# 输出不含窗口内容的机器可读门禁摘要。
[ordered]@{
    # 标记全部检查通过。
    ok = $true
    # 说明正式调用边界。
    boundary = 'tools/Invoke-ComputerControl.ps1'
    # 声明直接实现。
    implementation = 'rust'
    # 输出已检查窗口数量。
    inspectedWindows = $windows.Count
    # 输出 eligibility 聚合。
    eligibility = $eligibility
    # 输出 WGC runtime 聚合。
    wgcRuntime = $wgcRuntime
    # 输出 WGC item interop 聚合。
    wgcItemInterop = $wgcItemInterop
    # 标记前景门禁通过。
    foregroundUnchanged = $true
    # 标记全部像素操作为零。
    pixelsRead = 0
    # 标记全部文件写入为零。
    filesWritten = 0
    # 标记禁止字段命中为零。
    nativeFieldLeaks = 0
    # 输出 stale 门禁错误码。
    staleCode = $stale.error.code
    # 输出 assessment realm。
    assessmentRealm = $assessment.executionRealm
} | ConvertTo-Json -Depth 8
