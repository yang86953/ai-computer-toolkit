# 声明真实只读诊断隐私门禁的有界参数。
param(
    # 限制每个 sessions surface 的公开条数并使用足够覆盖当前桌面的默认条数。
    [ValidateRange(1, 4096)] [int]$MaximumItems = 64,
    # 限制每次 UIA worker 调用并使用项目公开默认 deadline。
    [ValidateRange(1, 30000)] [int]$TimeoutMs = 5000
)

# 启用严格变量与属性访问。
Set-StrictMode -Version Latest
# 让任何脚本错误立即终止门禁。
$ErrorActionPreference = 'Stop'

# 从脚本目录解析仓库根，禁止依赖调用方 cwd。
$repository = Split-Path -Parent $PSScriptRoot
# 固定统一迁移 launcher 路径。
$launcher = Join-Path $repository 'tools\Invoke-ComputerControl.ps1'
# launcher 缺失时禁止绕过到 executable。
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    # 返回可定位的环境错误。
    throw 'Computer-control launcher is missing.'
}

# 通过唯一 launcher 调用一个公开 capability 并解析 JSON。
function Invoke-ToolkitJson {
    # 声明固定参数。
    param(
        # 接收已经分词的 CLI 参数并禁止调用方传入 shell 字符串。
        [Parameter(Mandatory)] [string[]]$Arguments,
        # 只允许调用点显式声明的一种结构化失败码。
        [string]$AllowedFailureCode = '',
        # 同时固定该失败码对应的进程退出码。
        [ValidateRange(-1, 255)] [int]$AllowedFailureExitCode = -1,
        # 区分直接错误信封与 doctor 的单结果聚合信封。
        [ValidateSet('Direct', 'DoctorResult')] [string]$AllowedFailureShape = 'Direct'
    )
    # 在仓库根调用统一 launcher。
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $launcher @Arguments
    # 保存结构化命令退出码。
    $exitCode = $LASTEXITCODE
    # 先解析唯一 JSON，确保允许失败也必须使用公开 envelope。
    $value = $output | ConvertFrom-Json
    # 非零退出只允许调用点显式声明的精确错误与退出码组合。
    if ($exitCode -ne 0) {
        # 检查公开结果确实包含错误对象。
        $hasError = $null -ne $value.PSObject.Properties['error'] -and
            # 错误对象不得为空。
            $null -ne $value.error -and
            # 错误对象必须包含稳定 code。
            $null -ne $value.error.PSObject.Properties['code']
        # 直接失败必须携带唯一公开错误对象。
        $directFailureMatches = $hasError -and
            # 错误码必须逐字匹配调用点声明。
            ([string]$value.error.code) -ceq $AllowedFailureCode
        # 默认没有可接受的 doctor 聚合结果。
        $doctorFailureMatches = $false
        # doctor 只允许精确一个失败结果，避免隐藏部分成功或额外 provider。
        if ($AllowedFailureShape -ceq 'DoctorResult' -and
            # 聚合信封必须公开 results。
            $null -ne $value.PSObject.Properties['results']) {
            # 固定数组语义以处理单结果 JSON。
            $doctorResults = @($value.results)
            # 唯一结果必须是相同的结构化不可用错误。
            $doctorFailureMatches = $doctorResults.Count -eq 1 -and
                # 结果必须明确失败。
                $doctorResults[0].ok -eq $false -and
                # 结果必须包含错误对象。
                $null -ne $doctorResults[0].PSObject.Properties['error'] -and
                # 错误对象必须包含稳定 code。
                $null -ne $doctorResults[0].error.PSObject.Properties['code'] -and
                # 嵌套错误码必须逐字匹配调用点声明。
                ([string]$doctorResults[0].error.code) -ceq $AllowedFailureCode
        }
        # 只有完整匹配的预期失败可以继续进入隐私扫描。
        $isAllowedFailure = $AllowedFailureCode.Length -gt 0 -and
            # 退出码必须与公开错误映射一致。
            $exitCode -eq $AllowedFailureExitCode -and
            # 信封必须明确报告失败。
            $value.ok -eq $false -and
            # 按调用点声明匹配直接或 doctor 聚合形状。
            (($AllowedFailureShape -ceq 'Direct' -and $directFailureMatches) -or
                # doctor 不接受顶层伪造的直接错误。
                ($AllowedFailureShape -ceq 'DoctorResult' -and $doctorFailureMatches))
        # 任何未声明失败仍立即终止门禁。
        if (-not $isAllowedFailure) {
            # 不尝试第二控制面或静默回退。
            throw "Toolkit command failed with exit code $exitCode."
        }
    }
    # 返回成功或精确允许失败的公开信封供同一隐私扫描器处理。
    return $value
}

# 从迁移期顶层或 data 外壳读取同一 sessions 数组。
function Get-PublicSessions {
    # 声明固定参数。
    param(
        # 接收 launcher 返回对象并保持动态 JSON 形状。
        [Parameter(Mandatory)] [object]$Value
    )
    # 优先读取迁移期顶层数组。
    if ($null -ne $Value.PSObject.Properties['sessions']) {
        # 固定为数组以统一零一多形状。
        return @($Value.sessions)
    }
    # 读取 provider-neutral data 数组。
    if ($null -ne $Value.PSObject.Properties['data'] -and
        # data 必须实际存在。
        $null -ne $Value.data -and
        # data 必须包含 sessions。
        $null -ne $Value.data.PSObject.Properties['sessions']) {
        # 固定为数组以统一零一多形状。
        return @($Value.data.sessions)
    }
    # 缺失数组表示公开契约漂移。
    throw 'Diagnostic sessions response omitted sessions.'
}

# 递归结果通过序列化键扫描验证无原生字段。
function Assert-DiagnosticPrivacy {
    # 声明固定参数。
    param(
        # 提供失败时的命令标签并使用不含目标内容的安全名称。
        [Parameter(Mandatory)] [string]$Name,
        # 接收待扫描 JSON 对象并保持动态 JSON 形状。
        [Parameter(Mandatory)] [object]$Value,
        # UIA 节点允许语义 className。
        [switch]$AllowSemanticClassName
    )
    # 用足够深度序列化完整公开结果。
    $json = $Value | ConvertTo-Json -Depth 100 -Compress
    # 固定所有 surface 都禁止的原生字段。
    $forbiddenFields = @(
        # 禁止 HWND。
        'hwnd',
        # 禁止 PID。
        'processId',
        # 禁止矩形边界。
        'bounds',
        # 禁止 provider 私有身份。
        'providerId',
        # 禁止 handle 别名。
        'nativeHandle',
        # 禁止原生窗口别名。
        'nativeWindow',
        # 禁止原生进程别名。
        'nativeProcessId',
        # 禁止前景 HWND 起点。
        'before',
        # 禁止前景 HWND 终点。
        'after',
        # 禁止启动结果 PID。
        'launcherProcessId',
        # 禁止 C++ 顶层策略。
        'cppPolicy',
        # 禁止 C++ capability 状态。
        'cppStatus',
        # 禁止 C++ 执行开关。
        'cppExecutionEnabled',
        # 禁止聚合 C++ 执行可用性。
        'allCppExecutionAvailable',
        # 禁止构建实现语言。
        'implementationLanguage',
        # 禁止迁移版本。
        'migrationVersion',
        # 禁止构建版本。
        'buildVersion',
        # 禁止固定 Notepad runtime 路径。
        'notepadPath',
        # 禁止浏览器 runtime 路径。
        'browserPath',
        # 禁止通用 runtime 路径。
        'runtimePath',
        # 禁止 executable 路径。
        'executablePath'
    )
    # 非 UIA 节点结果还要禁止 Win32 class 字段。
    if (-not $AllowSemanticClassName) {
        # 把 className 加入本次禁止集合。
        $forbiddenFields += 'className'
    }
    # 逐项扫描 JSON 字段名而非普通文本值。
    foreach ($field in $forbiddenFields) {
        # 构造固定 JSON key 模式。
        $pattern = '"' + [Regex]::Escape($field) + '"\s*:'
        # 任一命中立即失败。
        if ($json -match $pattern) {
            # 不回显可能包含敏感值的原始 JSON。
            throw "$Name leaked native field '$field'."
        }
    }
    # 禁止旧 window:<HWND> 字符串目标。
    if ($json -match '"window:[0-9-]+"' -or
        # 禁止旧 UIA 目标。
        $json -match 'uia:window:' -or
        # 禁止旧 Win32 control 目标。
        $json -match 'win32-control:window:') {
        # 只报告安全命令标签。
        throw "$Name leaked a legacy native target."
    }
}

# 保存每个 surface 的公开 session 数量。
$sessionCounts = [ordered]@{}
# 保存已验证的 status/doctor/sessions 标签。
$verifiedReads = [System.Collections.Generic.List[string]]::new()
# 对其余公开 surface 执行完整 Rust status 与 doctor 隐私门禁。
foreach ($surface in @('app', 'process', 'browser', 'win32-control', 'notepad')) {
    # status 只读取 provider-neutral 运行事实。
    $status = Invoke-ToolkitJson -Arguments @('status', $surface)
    # status 不得泄漏路径或原生字段。
    Assert-DiagnosticPrivacy -Name "status $surface" -Value $status
    # 记录已验证 status 标签。
    $verifiedReads.Add("status:$surface")
    # doctor 复用同一安全 status 结果。
    $doctor = Invoke-ToolkitJson -Arguments @('doctor', $surface)
    # doctor 外壳不得恢复路径或原生字段。
    Assert-DiagnosticPrivacy -Name "doctor $surface" -Value $doctor
    # 记录已验证 doctor 标签。
    $verifiedReads.Add("doctor:$surface")
}
# 未认证媒体 status 必须以直接结构化不可用参与隐私门禁。
# 构造无续行歧义的 status 调用参数。
$mediaStatusInvocation = @{
    # 传入固定 status 命令。
    Arguments = @('status', 'media-session')
    # 只接受已发布能力缺少认证后台实现。
    AllowedFailureCode = 'BACKGROUND_OPERATION_UNAVAILABLE'
    # 直接错误由主入口映射为退出码三。
    AllowedFailureExitCode = 3
}
# 调用媒体 status 并只允许直接不可用信封。
$mediaStatus = Invoke-ToolkitJson @mediaStatusInvocation
# 直接失败 envelope 仍不得泄漏路径、原生字段或迁移元数据。
Assert-DiagnosticPrivacy -Name 'status media-session' -Value $mediaStatus
# 再次固定直接错误码，防止 helper 未来放宽后误报通过。
if ($mediaStatus.ok -or $mediaStatus.error.code -cne 'BACKGROUND_OPERATION_UNAVAILABLE') {
    # 不可用媒体不得伪装成成功状态。
    throw 'status media-session did not preserve structured unavailability.'
}
# 记录已验证的是直接结构化不可用而非 provider 成功。
$verifiedReads.Add('status-unavailable:media-session')
# doctor 保留聚合契约并把同一错误放入唯一 results 项。
# 构造无续行歧义的 doctor 调用参数。
$mediaDoctorInvocation = @{
    # 传入固定 doctor 命令。
    Arguments = @('doctor', 'media-session')
    # 只接受已发布能力缺少认证后台实现。
    AllowedFailureCode = 'BACKGROUND_OPERATION_UNAVAILABLE'
    # doctor 聚合失败由 CLI 固定映射为退出码二。
    AllowedFailureExitCode = 2
    # 要求唯一嵌套结果，不接受直接错误信封。
    AllowedFailureShape = 'DoctorResult'
}
# 调用媒体 doctor 并只允许单结果聚合不可用信封。
$mediaDoctor = Invoke-ToolkitJson @mediaDoctorInvocation
# 聚合失败 envelope 仍不得泄漏路径、原生字段或迁移元数据。
Assert-DiagnosticPrivacy -Name 'doctor media-session' -Value $mediaDoctor
# 固定唯一嵌套失败结果，防止 doctor 混入成功 provider。
$mediaDoctorResults = @($mediaDoctor.results)
# 公开 doctor 必须保持失败且只含同一后台不可用错误。
if ($mediaDoctor.ok -or $mediaDoctorResults.Count -ne 1 -or
    # 嵌套结果不得伪装成成功。
    $mediaDoctorResults[0].ok -or
    # 嵌套错误码必须保持稳定。
    $mediaDoctorResults[0].error.code -cne 'BACKGROUND_OPERATION_UNAVAILABLE') {
    # 不可用媒体不得伪装成成功诊断。
    throw 'doctor media-session did not preserve structured unavailability.'
}
# 记录已验证的是 doctor 聚合不可用而非 provider 成功。
$verifiedReads.Add('doctor-unavailable:media-session')
# 依次读取三个 catalog 公开诊断 surface。
foreach ($surface in @('window', 'uia', 'desktop')) {
    # status 不触发 provider 写入。
    $status = Invoke-ToolkitJson -Arguments @('status', $surface)
    # status 不得泄漏 native 字段。
    Assert-DiagnosticPrivacy -Name "status $surface" -Value $status
    # 记录已验证标签。
    $verifiedReads.Add("status:$surface")
    # doctor 复用同一安全 status 结果。
    $doctor = Invoke-ToolkitJson -Arguments @('doctor', $surface)
    # doctor 外壳不得恢复 native 字段。
    Assert-DiagnosticPrivacy -Name "doctor $surface" -Value $doctor
    # 记录已验证标签。
    $verifiedReads.Add("doctor:$surface")
    # sessions 使用有界只读发现。
    $sessionsResult = Invoke-ToolkitJson -Arguments @(
        # 调用 sessions verb。
        'sessions',
        # 指定当前 surface。
        $surface,
        # 固定数量选项。
        '--max-items',
        # 转成稳定十进制文本。
        [string]$MaximumItems
    )
    # sessions 不得泄漏 native 字段。
    Assert-DiagnosticPrivacy -Name "sessions $surface" -Value $sessionsResult
    # 读取统一 sessions 数组。
    $sessions = Get-PublicSessions -Value $sessionsResult
    # 保存安全计数。
    $sessionCounts[$surface] = $sessions.Count
    # 记录已验证标签。
    $verifiedReads.Add("sessions:$surface")
    # window 结果供后续精确检查选择目标。
    if ($surface -eq 'window') {
        # 保存当前窗口数组。
        $windowSessions = $sessions
    }
}

# 真实精确检查至少需要一个可见有标题窗口。
if (@($windowSessions).Count -eq 0) {
    # 不把空桌面误报为真实门禁通过。
    throw 'No visible titled window is available for diagnostic privacy verification.'
}
# 只从公开结果取得 opaque 目标。
$sessionId = [string]@($windowSessions)[0].sessionId
# 目标必须是 canonical s2:w。
if ($sessionId -notmatch '^s2:w:[0-9a-f]{16}$') {
    # 禁止猜测或重建原生目标。
    throw 'Window sessions did not publish a canonical opaque target.'
}

# window 与 desktop inspect 都只应返回安全窗口元数据。
foreach ($surface in @('window', 'desktop')) {
    # 使用当前公开目标执行精确检查。
    $inspect = Invoke-ToolkitJson -Arguments @(
        # 调用 inspect verb。
        'inspect',
        # 指定当前 surface。
        $surface,
        # 传入唯一公开目标字段。
        '--target',
        # 不重建或暴露目标私有输入。
        "sessionId=$sessionId"
    )
    # 窗口诊断不得公开 className 或其他 native 字段。
    Assert-DiagnosticPrivacy -Name "inspect $surface" -Value $inspect
    # 记录已验证标签。
    $verifiedReads.Add("inspect:$surface")
}

# uia root 检查允许语义 className，但不得泄漏原生目标。
$uiaInspect = Invoke-ToolkitJson -Arguments @(
    # 调用 inspect verb。
    'inspect',
    # 指定 UIA 只读 surface。
    'uia',
    # 传入唯一公开目标字段。
    '--target',
    # 使用当前 canonical 窗口。
    "sessionId=$sessionId",
    # 设置 worker deadline。
    '--timeout-ms',
    # 转为稳定十进制文本。
    [string]$TimeoutMs
)
# 保留允许语义 className 的扫描模式。
Assert-DiagnosticPrivacy -Name 'inspect uia' -Value $uiaInspect -AllowSemanticClassName
# 记录已验证标签。
$verifiedReads.Add('inspect:uia')

# 三个 inspect-tree 入口必须共享同一安全 Module 结果。
foreach ($surface in @('app', 'uia', 'accessibility')) {
    # 执行 root-only 有界 ControlView 检查。
    $tree = Invoke-ToolkitJson -Arguments @(
        # 调用独立只读 tree verb。
        'inspect-tree',
        # 指定当前兼容别名。
        $surface,
        # 传入唯一公开目标字段。
        '--target',
        # 使用当前 canonical 窗口。
        "sessionId=$sessionId",
        # 使用 root-only 深度。
        '--max-depth',
        # 固定零深度文本。
        '0',
        # 只读取一个节点。
        '--max-items',
        # 固定单节点文本。
        '1',
        # 使用 ControlView。
        '--view',
        # 固定公开枚举。
        'control',
        # 设置 worker deadline。
        '--timeout-ms',
        # 转为稳定十进制文本。
        [string]$TimeoutMs
    )
    # UIA 节点允许语义 className，其余 native 字段禁止。
    Assert-DiagnosticPrivacy -Name "inspect-tree $surface" -Value $tree -AllowSemanticClassName
    # 记录已验证标签。
    $verifiedReads.Add("inspect-tree:$surface")
}

# 输出不含目标内容的机器可读门禁摘要。
[ordered]@{
    # 标记全部检查通过。
    ok = $true
    # 说明正式调用边界。
    boundary = 'tools/Invoke-ComputerControl.ps1'
    # 标记全部调用无副作用。
    readOnly = $true
    # 输出安全公开计数。
    sessionCounts = $sessionCounts
    # 输出已验证调用标签。
    verifiedReads = @($verifiedReads)
    # 明确禁止字段命中为零。
    nativeFieldLeaks = 0
    # 明确 legacy 目标命中为零。
    legacyTargetLeaks = 0
    # 明确构建与迁移元数据命中为零。
    migrationMetadataLeaks = 0
} | ConvertTo-Json -Depth 6
