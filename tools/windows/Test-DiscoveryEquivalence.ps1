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
    Join-Path $projectRoot 'tests\contracts\discovery-equivalence-policy.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1')
if ($LASTEXITCODE -ne 0) {
    throw "C++ build failed with exit code $LASTEXITCODE."
}
& cargo build --manifest-path (Join-Path $projectRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    throw "Rust main build failed with exit code $LASTEXITCODE."
}

function Invoke-JsonCommand {
    param(
        [Parameter(Mandatory)]
        [string] $Executable,
        [Parameter(Mandatory)]
        [string[]] $CommandArguments,
        [int] $ExpectedExitCode = 0,
        [switch] $AllowHostInterference
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

    if (-not [string]::IsNullOrWhiteSpace($stderr)) {
        throw "Command wrote unexpected stderr: $stderr"
    }
    try {
        $json = $stdout | ConvertFrom-Json
    } catch {
        throw "Command did not return one JSON value: $stdout"
    }
    if ($process.ExitCode -ne $ExpectedExitCode) {
        if (
            -not $AllowHostInterference -or
            $json.error.code -ne 'HOST_INTERFERENCE_DETECTED'
        ) {
            throw "Unexpected exit $($process.ExitCode): $stdout $stderr"
        }
    }
    return $json
}

function Get-NormalizedSet {
    param([object[]] $Values)
    $set = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($value in $Values) {
        if (-not [string]::IsNullOrWhiteSpace([string] $value)) {
            [void] $set.Add(([string] $value).Trim())
        }
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

$cpp = Invoke-JsonCommand -Executable $cppExecutable -CommandArguments @(
    'discover',
    'app',
    '--max-applications',
    '4096',
    '--max-processes',
    '4096',
    '--max-windows',
    '4096'
)
# 直接执行 Rust 主实现的完整应用关系图。
$rust = Invoke-JsonCommand -Executable $rustExecutable -CommandArguments @(
    # 选择完整发现命令。
    'discover',
    # 选择应用关系图 surface。
    'app',
    # 设置应用硬边界。
    '--max-applications',
    # 覆盖全部现实应用来源。
    '4096',
    # 设置进程硬边界。
    '--max-processes',
    # 覆盖全部现实运行进程。
    '4096',
    # 设置窗口硬边界。
    '--max-windows',
    # 覆盖全部现实可见窗口。
    '4096'
# 结束 Rust 完整发现参数。
)
# 读取 Rust 统一 app facade 的当前 session 快照.
$rustApp = Invoke-JsonCommand -Executable $rustExecutable `
    # 使用有界但足以覆盖全部 provider 的数量.
    -CommandArguments @('sessions', 'app', '--max-items', '4096')
# 读取 C++ 兼容 facade 的同界限 session 快照。
$cppApp = Invoke-JsonCommand -Executable $cppExecutable `
    # 使用与 Rust 完全相同的公开命令参数。
    -CommandArguments @('sessions', 'app', '--max-items', '4096')

# 两种实现都必须满足同一只读顶层契约。
foreach ($inventory in @($cpp, $rust)) {
    # 验证版本、只读与前景不变门禁。
    if (
        # 命令必须成功。
        -not $inventory.ok -or
        # 协议版本必须一致。
        $inventory.contractVersion -ne $policy.contractVersion -or
        # 全流程必须只读。
        -not $inventory.data.readOnly -or
        # 成功时前景必须保持不变。
        -not $inventory.data.foregroundUnchanged
    ) {
        # 任一实现漂移都中止等价门禁。
        throw 'An inventory violated the top-level read-only contract.'
    }
    # 验证产品所需的非空与无窗口覆盖。
    if (
        # 必须发现已安装应用。
        @($inventory.data.applications).Count -eq 0 -or
        # 必须发现运行进程。
        @($inventory.data.processes).Count -eq 0 -or
        # 必须保留无窗口进程。
        $inventory.data.counts.noWindowProcesses -eq 0 -or
        # 当前验收机必须提供 Shell AppsFolder。
        $inventory.data.coverage.shellApplications -ne
            'available-shell-apps-folder'
    ) {
        # 报告不完整产品覆盖。
        throw 'An inventory did not include installed apps and no-window processes.'
    }
# 结束双实现顶层门禁。
}
# 同一登录会话必须生成完全相同的 canonical host 目标。
if ($rust.data.hostTargetId -cne $cpp.data.hostTargetId) {
    # 报告跨实现主机 identity 漂移。
    throw "Rust/C++ inventory host identity mismatch: $($rust.data.hostTargetId) != $($cpp.data.hostTargetId)"
# 结束 host identity 门禁。
}

# 两种统一 app facade 都必须满足同一 session envelope 不变量。
foreach ($appInventory in @($cppApp, $rustApp)) {
    # 验证固定 capability、只读、前景与身份声明。
    if (-not $appInventory.ok -or
        # 公开 surface 必须为 provider-neutral app。
        $appInventory.surface -cne 'app' -or
        # capability 必须保持版本化稳定 ID。
        $appInventory.capability -cne 'application.session.discover@1' -or
        # 发现不能执行任何写操作。
        -not $appInventory.readOnly -or
        # 同一快照必须保持前景不变。
        -not $appInventory.foregroundUnchanged -or
        # 目标身份只能是 opaque 版本化 session。
        $appInventory.targetIdentity -cne 'opaque-versioned-session-id' -or
        # session 数组必须存在。
        $null -eq $appInventory.sessions -or
        # warning 数组必须存在。
        $null -eq $appInventory.warnings) {
        # 任一实现漂移都中止门禁。
        throw 'An app session inventory violated the public discovery contract.'
    # 结束顶层字段门禁。
    }
    # 当前数量必须等于实际 session 数组长度。
    if ($appInventory.count -ne @($appInventory.sessions).Count -or
        # 完整数量不能小于返回数量。
        $appInventory.total -lt $appInventory.count -or
        # 截断标记必须与计数关系一致。
        $appInventory.truncated -ne
            [bool]($appInventory.total -gt $appInventory.count)) {
        # 报告计数或截断语义漂移。
        throw 'An app session inventory violated count and truncation invariants.'
    # 结束数量门禁。
    }
    # 顶层与 data 必须来自同一聚合事实。
    foreach ($field in @(
        # 固定 surface。
        'surface'
        # 固定 capability。
        'capability'
        # 固定只读分类。
        'readOnly'
        # 传播真实前景事实。
        'foregroundUnchanged'
        # 固定目标身份声明。
        'targetIdentity'
        # 返回数量。
        'count'
        # 完整数量。
        'total'
        # 截断标志。
        'truncated'
        # session 投影。
        'sessions'
        # provider 警告。
        'warnings'
    # 结束同源字段列表。
    )) {
        # 以稳定 JSON 投影比较标量、数组和对象。
        $topValue = $appInventory.$field |
            # 保留嵌套 capability descriptor。
            ConvertTo-Json -Depth 30 -Compress
        # 投影兼容 data 的同名字段。
        $dataValue = $appInventory.data.$field |
            # 使用相同序列化深度。
            ConvertTo-Json -Depth 30 -Compress
        # 字节级比较同源投影。
        if ($topValue -cne $dataValue) {
            # 报告发生漂移的字段。
            throw "App session compatibility field drifted: $field"
        # 结束同源字段比较。
        }
    # 结束全部同源字段门禁。
    }
    # 扫描完整 app session 结果的私有身份泄漏。
    $appSerialized = $appInventory | ConvertTo-Json -Depth 30 -Compress
    # 原生、路径与 provider 标识不得越过公开 JSON 边界。
    if ($appSerialized -match
        # 使用与完整 inventory 相同的禁止字段集合。
        '"(hwnd|pid|processId|nativeHandle|executablePath|registryPath|providerId|appUserModelId|aumid)":') {
        # 任一实现泄漏都使 session 等价门禁失败。
        throw 'An app session inventory leaked a native, path, or provider identifier.'
    # 结束 app session 隐私门禁。
    }
# 结束双实现 app session envelope 门禁。
}
# 只识别当前 Rust 主机观察 session。
$rustHosts = @(
    # 遍历 Rust 统一 app session.
    $rustApp.sessions |
        # 只按 provider-neutral host kind 识别。
        Where-Object {
            # application.open 已改绑精确 s2:a，host 不再承载静态白名单。
            $_.kind -eq 'host'
        # 结束 Rust host 过滤.
        }
# 结束 Rust host session 集合.
)
# 统一 app 必须只发布一个当前主机目标.
if ($rustHosts.Count -ne 1) {
    # 拒绝缺失或歧义主机身份.
    throw "Rust app returned $($rustHosts.Count) host sessions."
# 结束唯一主机门禁.
}
# 保存唯一 Rust 主机目标.
$rustHost = $rustHosts[0]
# 主机目标必须 canonical 且与 C++ hostTargetId 完全相等.
if ($rustHost.sessionId -cnotmatch '^s2:h:[0-9a-f]{16}$' -or
    # 同一 Windows 登录会话必须生成相同字节级指纹.
    $rustHost.sessionId -cne $cpp.data.hostTargetId) {
    # 报告跨实现主机身份漂移.
    throw "Rust/C++ host identity mismatch: $($rustHost.sessionId) != $($cpp.data.hostTargetId)"
# 结束跨实现主机身份门禁.
}
# 序列化 Rust 主机结果以检查私有身份泄漏.
$rustHostSerialized = $rustHost | ConvertTo-Json -Depth 20 -Compress
# 检查 Windows session 与用户名字段都未公开.
foreach ($term in @(
    # 禁止 Windows session ID 字段.
    'windowsSessionId'
    # 禁止当前用户名字段.
    'userName'
    # 禁止原生进程标识字段.
    'processId'
    # 禁止安全 token 字段.
    'token'
    # 禁止安全标识符字段.
    'sid'
# 结束主机禁止字段列表.
)) {
    # 使用 ordinal ignore-case 搜索字段名.
    if ($rustHostSerialized.IndexOf(
        # 搜索当前禁止字段.
        $term,
        # 固定不区分大小写的比较规则.
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        # 报告 Rust 主机私有身份泄漏.
        throw "Rust host session leaked private identity field: $term"
    # 结束私有字段泄漏分支.
    }
# 结束 Rust 主机结果泄漏检查.
}
# 使用刚发现的 opaque 主机目标验证实时重新解析.
$rustHostInspection = Invoke-JsonCommand -Executable $rustExecutable `
    # 通过统一 app facade 执行只读 inspect.
    -CommandArguments @(
        # 选择 inspect verb.
        'inspect',
        # 选择统一 app surface.
        'app',
        # 传入精确目标参数名.
        '--target',
        # 使用本次发现的 canonical s2:h.
        "sessionId=$($rustHost.sessionId)"
    # 结束 Rust 主机 inspect 参数.
    )
# inspect 必须成功并回显同一个公共目标.
if (-not $rustHostInspection.ok -or
    # 回显 session 必须逐字节相等.
    $rustHostInspection.sessionId -cne $rustHost.sessionId) {
    # 报告 Rust 主机重新发现失败.
    throw 'Rust app host inspection failed exact re-resolution.'
# 结束 Rust 主机 inspect 门禁.
}
# 固定不存在的 canonical 主机目标必须 fail closed.
$rustHostStale = Invoke-JsonCommand -Executable $rustExecutable `
    # 请求不存在主机目标的统一 inspect.
    -CommandArguments @(
        # 选择 inspect verb.
        'inspect',
        # 选择统一 app surface.
        'app',
        # 传入精确目标参数名.
        '--target',
        # 使用固定 stale s2:h 夹具.
        'sessionId=s2:h:0000000000000000'
    # 结束 stale 主机 inspect 参数.
    ) `
    # 结构化目标缺失必须使用失败退出码.
    -ExpectedExitCode 2
# 统一 app facade 对 stale 主机报告 provider-neutral 目标缺失.
if ($rustHostStale.ok -or
    # 固定对外错误码为 TARGET_NOT_FOUND.
    $rustHostStale.error.code -ne 'TARGET_NOT_FOUND') {
    # 报告 stale 主机被错误接受.
    throw 'Rust app accepted a stale host session.'
# 结束 Rust stale 主机门禁.
}

# 两种实现都不得泄漏原生、路径或 provider identity。
foreach ($inventory in @($cpp, $rust)) {
    # 序列化完整关系图供字段名扫描。
    $serialized = $inventory | ConvertTo-Json -Depth 30 -Compress
    # 使用固定禁止字段集合。
    if (
        # 匹配任何不允许跨 JSON 边界的字段。
        $serialized -match
        '"(hwnd|pid|processId|nativeHandle|executablePath|registryPath|providerId|appUserModelId|aumid)":'
    ) {
        # 任一实现泄漏都使门禁失败。
        throw 'An inventory leaked a native, path, or provider identifier.'
    }
# 结束双实现隐私门禁。
}

# 验证一份完整关系图的引用、分类和 Shell 覆盖。
function Assert-InventoryRelations {
    # 接收待验证关系图。
    param([Parameter(Mandatory)] [object] $Inventory)
    # 收集应用 opaque 目标。
    $apps = Get-NormalizedSet @(
        # 投影应用 ID。
        $Inventory.data.applications | ForEach-Object { $_.sessionId }
    # 结束应用 ID 集合。
    )
    # 收集进程 opaque 目标。
    $processes = Get-NormalizedSet @(
        # 投影进程 ID。
        $Inventory.data.processes | ForEach-Object { $_.sessionId }
    # 结束进程 ID 集合。
    )
    # 收集窗口 opaque 目标。
    $windows = Get-NormalizedSet @(
        # 投影窗口 ID。
        $Inventory.data.windows | ForEach-Object { $_.sessionId }
    # 结束窗口 ID 集合。
    )
    # 禁止应用 identity 重复。
    if ($apps.Count -ne @($Inventory.data.applications).Count) {
        # 报告应用 identity 碰撞。
        throw 'Inventory contains duplicate application identities.'
    }
    # Shell 可用时必须至少返回一个 Shell 应用。
    if ($Inventory.data.coverage.shellApplications -eq 'available-shell-apps-folder' -and
        # 检查来源标记。
        @($Inventory.data.applications | Where-Object { $_.discoverySources -contains 'shell-apps-folder' }).Count -eq 0) {
        # 报告来源与事实冲突。
        throw 'Inventory reported Shell AppsFolder without any Shell application.'
    }
    # 验证应用到进程关系。
    foreach ($application in @($Inventory.data.applications)) {
        # 遍历全部进程关系。
        foreach ($processId in @($application.runningProcessSessionIds)) {
            # 关系必须指向当前进程集合。
            if (-not $processes.Contains($processId)) { throw "Application relation references unknown process $processId." }
        }
    }
    # 验证进程分类与双向关系。
    foreach ($process in @($Inventory.data.processes)) {
        # 分类必须属于封闭集合且不要求前景。
        if ($process.metadataAccess -notin @('available', 'permission-blocked', 'unavailable') -or
            # 相对完整性必须属于封闭集合。
            $process.integrityRelation -notin @('lower', 'same', 'higher', 'unknown') -or
            # 只读观察不得要求前景。
            $process.foregroundRequiredForObservation) { throw 'Process permission/visibility classification is invalid.' }
        # 验证进程到窗口关系。
        foreach ($windowId in @($process.windowSessionIds)) {
            # 关系必须指向当前窗口集合。
            if (-not $windows.Contains($windowId)) { throw "Process relation references unknown window $windowId." }
        }
        # 验证进程到应用关系。
        foreach ($applicationId in @($process.relatedApplicationIds)) {
            # 关系必须指向当前应用集合。
            if (-not $apps.Contains($applicationId)) { throw "Process relation references unknown app $applicationId." }
        }
    }
    # 验证窗口到进程关系。
    foreach ($window in @($Inventory.data.windows)) {
        # 非空关系必须指向当前进程集合。
        if ($null -ne $window.processSessionId -and -not $processes.Contains($window.processSessionId)) { throw "Window relation references unknown process $($window.processSessionId)." }
    }
# 结束单份关系图门禁。
}
# Rust 主实现必须独立通过完整关系门禁。
Assert-InventoryRelations -Inventory $rust

$applicationIds = Get-NormalizedSet @(
    $cpp.data.applications | ForEach-Object { $_.sessionId }
)
$processIds = Get-NormalizedSet @(
    $cpp.data.processes | ForEach-Object { $_.sessionId }
)
$windowIds = Get-NormalizedSet @(
    $cpp.data.windows | ForEach-Object { $_.sessionId }
)
if ($applicationIds.Count -ne @($cpp.data.applications).Count) {
    throw 'Application inventory contains duplicate opaque identities.'
}
$shellApplications = @(
    $cpp.data.applications | Where-Object {
        $_.discoverySources -contains 'shell-apps-folder'
    }
)
if ($shellApplications.Count -eq 0) {
    throw 'Shell AppsFolder source reported available but returned no apps.'
}
foreach ($application in $cpp.data.applications) {
    foreach ($processId in $application.runningProcessSessionIds) {
        if (-not $processIds.Contains($processId)) {
            throw "Application relation references unknown process $processId."
        }
    }
}
foreach ($process in $cpp.data.processes) {
    if (
        $process.metadataAccess -notin @(
            'available', 'permission-blocked', 'unavailable'
        ) -or
        $process.integrityRelation -notin @(
            'lower', 'same', 'higher', 'unknown'
        ) -or
        $process.foregroundRequiredForObservation
    ) {
        throw 'Process permission/visibility classification is invalid.'
    }
    foreach ($windowId in $process.windowSessionIds) {
        if (-not $windowIds.Contains($windowId)) {
            throw "Process relation references unknown window $windowId."
        }
    }
    foreach ($applicationId in $process.relatedApplicationIds) {
        if (-not $applicationIds.Contains($applicationId)) {
            throw "Process relation references unknown app $applicationId."
        }
    }
}
foreach ($window in $cpp.data.windows) {
    if (
        $null -ne $window.processSessionId -and
        -not $processIds.Contains($window.processSessionId)
    ) {
        throw "Window relation references unknown process $($window.processSessionId)."
    }
}

$cppProcessNames = Get-NormalizedSet @(
    $cpp.data.processes | ForEach-Object { $_.processName }
)
$rustProcessNames = Get-NormalizedSet @(
    $rust.data.processes | ForEach-Object { $_.processName }
)
$processJaccard = Get-Jaccard $cppProcessNames $rustProcessNames
if ($processJaccard -lt $policy.minimumProcessNameJaccard) {
    throw "Process equivalence Jaccard $processJaccard is below policy."
}

$cppWindowFacts = Get-NormalizedSet @(
    $cpp.data.windows |
        ForEach-Object { "$($_.applicationName)|$($_.title)" }
)
$rustWindowFacts = Get-NormalizedSet @(
    $rust.data.windows |
        ForEach-Object { "$($_.applicationName)|$($_.title)" }
)
$windowJaccard = Get-Jaccard $cppWindowFacts $rustWindowFacts
if ($windowJaccard -lt $policy.minimumVisibleWindowJaccard) {
    throw "Window equivalence Jaccard $windowJaccard is below policy."
}

# 比较已安装应用显示名集合，覆盖 Registry 与 Shell 合并结果。
$cppApplicationNames = Get-NormalizedSet @(
    # 投影 C++ 应用显示名。
    $cpp.data.applications | ForEach-Object { $_.displayName }
# 结束 C++ 应用集合。
)
# 构造 Rust 应用显示名集合。
$rustApplicationNames = Get-NormalizedSet @(
    # 投影 Rust 应用显示名。
    $rust.data.applications | ForEach-Object { $_.displayName }
# 结束 Rust 应用集合。
)
# 计算应用集合 Jaccard。
$applicationJaccard = Get-Jaccard $cppApplicationNames $rustApplicationNames
# 执行应用集合阈值。
if ($applicationJaccard -lt $policy.minimumApplicationNameJaccard) {
    # 报告具体差异率。
    throw "Application equivalence Jaccard $applicationJaccard is below policy."
# 结束应用集合阈值门禁。
}

function Assert-Assessment {
    param(
        [string] $Capability,
        [string] $TargetId,
        [string] $ExpectedDecision
    )
    $assessment = $null
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        $assessment = Invoke-JsonCommand -Executable $cppExecutable `
            -CommandArguments @(
                'assess',
                'app',
                '--capability',
                $Capability,
                '--target',
                "sessionId=$TargetId"
            ) -AllowHostInterference
        if ($assessment.error.code -ne 'HOST_INTERFERENCE_DETECTED') {
            break
        }
    }
    if (
        -not $assessment.ok -or
        $assessment.contractVersion -ne 'act/control/v1' -or
        $assessment.capability -ne $Capability -or
        $assessment.targetId -ne $TargetId -or
        $assessment.decision -ne $ExpectedDecision -or
        $assessment.evidence.foregroundActivationAllowed -or
        $assessment.evidence.inputAllowed
    ) {
        throw "Assessment contract failed for $Capability."
    }
    # 只对策略声明的 Stage 3 共享 assessment 执行 Rust/C++ 等价比较。
    if ($Capability -in @($policy.rustCppEquivalentAssessments)) {
        # 初始化 Rust assessment 结果。
        $rustAssessment = $null
        # 最多重试三次瞬时前景干扰。
        for ($attempt = 0; $attempt -lt 3; $attempt++) {
            # 调用 Rust 主入口的同一 opaque 目标。
            $rustAssessment = Invoke-JsonCommand -Executable $rustExecutable `
                -CommandArguments @(
                    # 指定 generic assessment 命令。
                    'assess',
                    # 指定 app surface。
                    'app',
                    # 指定同一版本化 capability。
                    '--capability',
                    # 传入 capability 值。
                    $Capability,
                    # 指定精确目标选项。
                    '--target',
                    # 传入同一 canonical session。
                    "sessionId=$TargetId"
                ) -AllowHostInterference
            # 非前景干扰结果结束重试。
            if ($rustAssessment.error.code -ne 'HOST_INTERFERENCE_DETECTED') {
                # 退出重试循环。
                break
            }
        }
        # Rust 与 C++ 必须共享决策、realm、原因与安全约束。
        if (-not $rustAssessment.ok -or
            # 核对封闭 decision。
            $rustAssessment.decision -ne $assessment.decision -or
            # 核对 execution realm。
            $rustAssessment.executionRealm -ne $assessment.executionRealm -or
            # 核对确认要求。
            $rustAssessment.requiresConfirmation -ne $assessment.requiresConfirmation -or
            # 核对前台同意要求。
            $rustAssessment.requiresForegroundConsent -ne $assessment.requiresForegroundConsent -or
            # 核对稳定原因顺序与内容。
            (($rustAssessment.reasons -join '|') -ne ($assessment.reasons -join '|')) -or
            # 核对范围摘要。
            $rustAssessment.constraints.scope -ne $assessment.constraints.scope -or
            # 核对只读分类。
            $rustAssessment.constraints.readOnly -ne $assessment.constraints.readOnly -or
            # 核对禁止回退。
            $rustAssessment.constraints.noFallback -ne $assessment.constraints.noFallback -or
            # 核对目标类别。
            $rustAssessment.evidence.targetKind -ne $assessment.evidence.targetKind -or
            # 两边都不得授权前景激活。
            $rustAssessment.evidence.foregroundActivationAllowed -or
            # 两边都不得授权输入。
            $rustAssessment.evidence.inputAllowed) {
            # 报告具体 capability 的跨语言漂移。
            throw "Rust/C++ assessment equivalence failed for $Capability."
        }
    }
    return $assessment
}

$hostTarget = $cpp.data.hostTargetId
[void] (Assert-Assessment 'application.discover@1' $hostTarget `
    $policy.requiredAssessmentDecisions.'application.discover@1')
[void] (Assert-Assessment 'process.discover@1' $hostTarget `
    $policy.requiredAssessmentDecisions.'process.discover@1')
[void] (Assert-Assessment 'window.discover@1' $hostTarget `
    $policy.requiredAssessmentDecisions.'window.discover@1')
[void] (Assert-Assessment 'uncataloged.read@1' $hostTarget `
    $policy.requiredAssessmentDecisions.'uncataloged.read@1')

$availableProcess = @(
    $cpp.data.processes | Where-Object {
        $_.metadataAccess -eq 'available'
    } | Select-Object -First 1
)
if ($availableProcess.Count -eq 0) {
    throw 'No process exposes readable public metadata.'
}
[void] (Assert-Assessment 'process.metadata.read@1' `
    $availableProcess[0].sessionId `
    $policy.requiredAssessmentDecisions.'process.metadata.read@1')

$blockedProcess = @(
    $cpp.data.processes | Where-Object {
        $_.metadataAccess -eq 'permission-blocked'
    } | Select-Object -First 1
)
$permissionAssessment = 'safe-skip-no-blocked-process'
if ($blockedProcess.Count -gt 0) {
    $blockedAssessment = Assert-Assessment 'process.metadata.read@1' `
        $blockedProcess[0].sessionId 'permission-blocked'
    if (
        $blockedAssessment.executionRealm -ne 'none' -or
        $blockedAssessment.evidence.inputAllowed
    ) {
        throw 'Permission-blocked process assessment did not fail closed.'
    }
    $permissionAssessment = 'permission-blocked-passed'
}

$firstWindow = @($cpp.data.windows)[0]
if ($null -ne $firstWindow) {
    [void] (Assert-Assessment 'accessibility.tree.read@1' `
        $firstWindow.sessionId `
        $policy.requiredAssessmentDecisions.'accessibility.tree.read@1')
    $inputAssessment = Assert-Assessment 'ui.input.key@1' `
        $firstWindow.sessionId `
        $policy.requiredAssessmentDecisions.'ui.input.key@1'
    if (
        -not $inputAssessment.requiresConfirmation -or
        -not $inputAssessment.requiresForegroundConsent -or
        $inputAssessment.executionRealm -ne 'none'
    ) {
        throw 'Unmigrated input assessment did not fail closed.'
    }
}

$firstApplication = @($cpp.data.applications)[0]
if ($null -ne $firstApplication) {
    $openAssessment = Assert-Assessment 'application.open@1' `
        $firstApplication.sessionId `
        $policy.requiredAssessmentDecisions.'application.open@1'
    if (
        -not $openAssessment.requiresConfirmation -or
        $openAssessment.executionRealm -ne 'none'
    ) {
        throw 'Unmigrated application open assessment did not fail closed.'
    }
}

$stale = Invoke-JsonCommand -Executable $cppExecutable `
    -CommandArguments @(
        'assess',
        'app',
        '--capability',
        'application.discover@1',
        '--target',
        'sessionId=s2:h:0000000000000000'
    ) -ExpectedExitCode 2
if ($stale.ok -or $stale.error.code -ne 'STALE_SESSION') {
    throw 'Assessment did not reject a stale exact target.'
}

$forbidden = [ordered]@{
    SetForegroundWindow = '\bSetForegroundWindow\s*\('
    SendInput = '\bSendInput\s*\('
    SetFocus = '\bSetFocus\s*\('
    SetClipboardData = '\bSetClipboardData\s*\('
}
$certifiedWriteLocations = @{
    SetForegroundWindow = @(
        'cpp\src\platform\windows\foreground_input_backend.cpp'
    )
    SendInput = @(
        'cpp\src\platform\windows\foreground_input_backend.cpp'
    )
    SetFocus = @()
    SetClipboardData = @()
}
$sourceFiles = Get-ChildItem -LiteralPath (
    Join-Path $projectRoot 'cpp\src'
) -Recurse -File -Filter '*.cpp'
foreach ($sourceFile in $sourceFiles) {
    $source = Get-Content -LiteralPath $sourceFile.FullName -Raw
    $relative = $sourceFile.FullName.Substring(
        $projectRoot.Length + 1
    )
    foreach ($entry in $forbidden.GetEnumerator()) {
        if ($source -cmatch $entry.Value -and
            $relative -notin
                $certifiedWriteLocations[$entry.Key]) {
            throw "$($sourceFile.FullName) contains forbidden write API $($entry.Key)."
        }
    }
}

[PSCustomObject]@{
    ok = $true
    installedApplications = @($cpp.data.applications).Count
    shellApplications = $shellApplications.Count
    runningProcesses = @($cpp.data.processes).Count
    noWindowProcesses = $cpp.data.counts.noWindowProcesses
    visibleWindows = @($cpp.data.windows).Count
    # 报告应用集合等价率。
    applicationNameJaccard = [Math]::Round($applicationJaccard, 4)
    processNameJaccard = [Math]::Round($processJaccard, 4)
    visibleWindowJaccard = [Math]::Round($windowJaccard, 4)
    nativeIdentifierLeak = $false
    foregroundUnchanged = $true
    assessmentFailClosed = $true
    permissionBlockedProcesses = $cpp.data.counts.permissionBlockedProcesses
    higherIntegrityProcesses = $cpp.data.counts.higherIntegrityProcesses
    permissionAssessment = $permissionAssessment
    staleTargetError = $stale.error.code
    # 报告 Rust 与 C++ 主机身份完全一致.
    hostIdentityExact = $true
    # 报告两种 app session envelope 通过同源与隐私门禁。
    appSessionEnvelopeEquivalent = $true
    # 报告 Rust 主机 inspect 重新解析成功.
    rustHostInspect = $true
    # 报告 Rust 主机 stale 拒绝错误码.
    rustHostStaleTargetError = $rustHostStale.error.code
} | ConvertTo-Json
