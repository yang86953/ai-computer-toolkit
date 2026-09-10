[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cpp = Join-Path `
    $projectRoot 'build\cpp-main\ai-computer-toolkit-cpp.exe'
$null = Get-Content -LiteralPath (
    Join-Path `
        $projectRoot 'contracts\v1\window-observation.schema.json'
) -Raw | ConvertFrom-Json

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$status = & $cpp status window | ConvertFrom-Json
$sessions = & $cpp sessions window --max-items 4096 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $status.ok -or
    $status.data.capability -ne 'window.discover@1' -or
    -not $status.data.foregroundUnchanged -or
    -not $sessions.ok -or
    $sessions.data.capability -ne 'window.discover@1' -or
    -not $sessions.data.foregroundUnchanged -or
    $sessions.data.count -ne $sessions.data.sessions.Count -or
    $sessions.data.total -lt $sessions.data.count -or
    $sessions.data.truncated -ne
        ($sessions.data.total -gt $sessions.data.count)) {
    throw 'C++ direct window observation contract failed.'
}

$rust = cargo run --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
) -- sessions window --target visible=true --max-items 4096 |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $rust.ok -or
    -not $rust.foreground.unchanged) {
    throw 'Rust window compatibility read failed.'
}

# 通过统一 app facade 读取本批迁移后的 Rust 窗口目标.
$rustApp = cargo run --quiet --manifest-path (
    # 复用当前仓库清单，避免依赖调用目录.
    Join-Path $projectRoot 'Cargo.toml'
# 请求有界的统一 session 快照.
) -- sessions app --max-items 4096 |
    # 解析统一 facade 的 JSON 输出.
    ConvertFrom-Json
# 拒绝命令失败或错误形状.
if ($LASTEXITCODE -ne 0 -or
    # 统一 facade 必须报告成功.
    -not $rustApp.ok -or
    # session 集合必须存在.
    $null -eq $rustApp.sessions) {
    # 报告迁移面读取失败.
    throw 'Rust app window migration read failed.'
# 结束统一读取门禁.
}
# 只保留发布窗口截图 capability 的 session.
$rustAppWindows = @(
    # 遍历统一 facade 的全部 provider session.
    $rustApp.sessions |
        # 通过稳定 capability ID 识别窗口 provider.
        Where-Object {
            # 要求能力数组包含窗口截图能力.
            @($_.capabilities | Where-Object { $_.id -eq 'window.screenshot@1' }).Count -gt 0
        # 结束窗口 provider 过滤.
        }
# 结束窗口 session 快照.
)
# 验证每个 Rust 窗口目标的 canonical s2 表示.
foreach ($window in $rustAppWindows) {
    # 固定版本、kind 和小写十六进制摘要.
    if ($window.sessionId -cnotmatch '^s2:w:[0-9a-f]{16}$') {
        # 拒绝旧 s1、原生句柄或非 canonical 表示.
        throw "Rust app window returned non-canonical session: $($window.sessionId)"
    # 结束 canonical s2 门禁.
    }
# 结束全部窗口身份检查.
}
# 记录跨实现精确身份命中数.
$exactIdentityMatches = 0
# 逐个检查 C++ 窗口的安全事实和 Rust s2 身份.
foreach ($cppWindow in $sessions.data.sessions) {
    # 找出 C++ 快照中具有相同安全事实的窗口.
    $cppMatches = @(
        # 遍历 C++ 当前快照.
        $sessions.data.sessions |
            # 按不区分大小写的进程名和精确标题匹配.
            Where-Object {
                # 要求两个公开事实同时相等.
                $_.applicationName -ieq $cppWindow.applicationName -and
                    # 标题保持逐字匹配.
                    $_.title -ceq $cppWindow.title
            # 结束 C++ 安全事实匹配.
            }
    # 结束 C++ 同事实集合.
    )
    # 找出 Rust 统一快照中具有相同安全事实的窗口.
    $rustMatches = @(
        # 遍历已过滤的 Rust 窗口 provider session.
        $rustAppWindows |
            # 对齐两边公开且稳定的事实.
            Where-Object {
                # 进程名不区分大小写，标题逐字匹配.
                $_.processName -ieq $cppWindow.applicationName -and
                    # 标题保持逐字匹配.
                    $_.title -ceq $cppWindow.title
            # 结束 Rust 安全事实匹配.
            }
    # 结束 Rust 同事实集合.
    )
    # 只对两边都唯一的安全事实执行强身份断言.
    if ($cppMatches.Count -eq 1 -and $rustMatches.Count -eq 1) {
        # 相同原生身份算法必须生成完全相等的 s2 ID.
        if ($cppWindow.sessionId -cne $rustMatches[0].sessionId) {
            # 报告跨实现身份漂移.
            throw "Rust/C++ exact window identity mismatch: $($cppWindow.sessionId) != $($rustMatches[0].sessionId)"
        # 结束跨实现身份相等门禁.
        }
        # 累加已验证的唯一身份.
        $exactIdentityMatches += 1
    # 结束唯一安全事实分支.
    }
# 结束全部 C++ 窗口遍历.
}
# 两边都有窗口时必须至少形成一个可验证的唯一对应.
if ($sessions.data.sessions.Count -gt 0 -and
    # Rust 统一窗口 provider 同样存在目标.
    $rustAppWindows.Count -gt 0 -and
    # 禁止在没有任何精确身份证据时误报通过.
    $exactIdentityMatches -eq 0) {
    # 报告缺少跨实现唯一身份样本.
    throw 'Rust/C++ window snapshots had no unique exact identity match.'
# 结束身份样本充分性门禁.
}
# 记录 Rust 统一 facade 的真实 inspect 门禁结果.
$rustExactInspect = $false
# 有窗口时验证发现后的 opaque ID 可通过重新枚举解析.
if ($rustAppWindows.Count -gt 0) {
    # 选择当前快照中的一个精确窗口目标.
    $rustTarget = $rustAppWindows[0].sessionId
    # 通过统一 app facade 执行只读 inspect.
    $rustInspect = cargo run --quiet --manifest-path (
        # 复用当前仓库清单.
        Join-Path $projectRoot 'Cargo.toml'
    # 传入刚发现的 canonical s2 窗口目标.
    ) -- inspect app --target "sessionId=$rustTarget" |
        # 解析 inspect JSON 结果.
        ConvertFrom-Json
    # 要求命令成功并回显同一个公共 session.
    if ($LASTEXITCODE -ne 0 -or
        # 统一 inspect 必须报告成功.
        -not $rustInspect.ok -or
        # 回显身份必须与发现结果完全一致.
        $rustInspect.sessionId -cne $rustTarget) {
        # 报告 Rust 重新发现 inspect 失败.
        throw 'Rust app exact window inspection failed.'
    # 结束 Rust inspect 门禁.
    }
    # 标记已有真实窗口通过重新发现.
    $rustExactInspect = $true
# 结束有窗口 inspect 分支.
}
# 使用不存在的 canonical s2 目标验证 stale 拒绝.
$rustStale = cargo run --quiet --manifest-path (
    # 复用当前仓库清单.
    Join-Path $projectRoot 'Cargo.toml'
# 请求固定不存在的窗口身份.
) -- inspect app --target sessionId=s2:w:0000000000000000 |
    # 解析结构化错误 JSON.
    ConvertFrom-Json
# 统一路由必须拒绝 stale 目标并返回稳定错误码.
if ($LASTEXITCODE -eq 0 -or
    # app facade 对外使用 provider-neutral 目标缺失错误.
    $rustStale.error.code -ne 'TARGET_NOT_FOUND') {
    # 报告 stale 目标被错误接受.
    throw 'Rust app window inspect accepted a stale target.'
# 结束 Rust stale 门禁.
}
$cppFacts = @(
    $sessions.data.sessions |
        ForEach-Object {
            "$($_.applicationName.ToLowerInvariant())`n$($_.title)"
        } |
        Sort-Object -Unique
)
$rustFacts = @(
    $rust.sessions |
        Where-Object {
            $_.visible -and
            -not [string]::IsNullOrWhiteSpace($_.title)
        } |
        ForEach-Object {
            "$($_.processName.ToLowerInvariant())`n$($_.title)"
        } |
        Sort-Object -Unique
)
$intersection = @(
    $cppFacts | Where-Object { $rustFacts -contains $_ }
)
$union = @($cppFacts + $rustFacts | Sort-Object -Unique)
$jaccard = if ($union.Count -eq 0) {
    1.0
} else {
    $intersection.Count / $union.Count
}
if ($jaccard -lt 0.9) {
    throw "Rust/C++ safe window fact Jaccard too low: $jaccard"
}

$exactInspect = $false
if ($sessions.data.sessions.Count -gt 0) {
    $target = $sessions.data.sessions[0].sessionId
    $inspect = & $cpp inspect window `
        --target "sessionId=$target" |
        ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or
        -not $inspect.ok -or
        $inspect.data.capability -ne 'window.metadata.read@1' -or
        $inspect.data.window.sessionId -ne $target) {
        throw 'C++ exact window metadata inspection failed.'
    }
    $exactInspect = $true
}
$stale = & $cpp inspect window `
    --target sessionId=s2:w:0000000000000000 |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $stale.error.code -ne 'STALE_SESSION') {
    throw 'C++ window inspect accepted a stale target.'
}

$serialized = $sessions | ConvertTo-Json -Depth 20 -Compress
foreach ($term in @(
    'hwnd'
    'processId'
    'nativeWindow'
    'nativeProcessId'
    'className'
)) {
    if ($serialized.IndexOf(
        $term,
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "C++ window result leaked native field: $term"
    }
}

# 序列化 Rust 统一窗口结果以检查私有字段泄漏.
$rustAppSerialized = $rustAppWindows | ConvertTo-Json -Depth 20 -Compress
# 检查原生标识及进程创建时间都未越过统一边界.
foreach ($term in @(
    # 禁止 Win32 窗口句柄字段.
    'hwnd'
    # 禁止原生进程标识字段.
    'processId'
    # 禁止旧窗口类名字段.
    'className'
    # 禁止 Rust 私有创建时间字段.
    'processCreationTime'
    # 禁止 C++ 私有创建时间别名.
    'creationTime'
# 结束禁止字段列表.
)) {
    # 对字段名执行不区分大小写的泄漏搜索.
    if ($rustAppSerialized.IndexOf(
        # 搜索当前禁止字段.
        $term,
        # 固定 ordinal ignore-case 规则.
        [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        # 报告统一 Rust 结果泄漏.
        throw "Rust app window result leaked native field: $term"
    # 结束字段泄漏分支.
    }
# 结束 Rust 统一窗口泄漏检查.
}

[PSCustomObject]@{
    ok = $true
    cppCount = $sessions.data.total
    rustVisibleTitledCount = $rustFacts.Count
    safeFactJaccard = [math]::Round($jaccard, 4)
    exactInspect = $exactInspect
    # 报告 Rust/C++ 完全相等的 s2 窗口目标数量.
    exactIdentityMatches = $exactIdentityMatches
    # 报告 Rust 统一 facade 是否完成真实 inspect.
    rustExactInspect = $rustExactInspect
    # 报告 Rust 统一 facade 拒绝 stale 目标.
    rustStaleTargetRefused = $true
    staleTargetRefused = $true
    foregroundUnchanged = $true
    nativeIdentifierLeak = $false
} | ConvertTo-Json
