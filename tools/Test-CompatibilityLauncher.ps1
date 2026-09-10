[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$launcher =
    Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 固定 Rust 主程序供 launcher 路由对照。
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
$cpp = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'ai-computer-toolkit-cpp.exe'
$fixturePath = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'launcher-must-not-write.png'
$textRequestPath = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'launcher-text-stale.json'
$editRequestPath = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'launcher-edit-stale.json'
$closeRequestPath = Join-Path (
    Join-Path $projectRoot 'build\cpp-main'
) 'launcher-close-stale.json'

# 在构建真实运行时前验证两类缺失运行时的结构化失败外壳.
& (Join-Path $PSScriptRoot 'Test-LauncherRuntimeUnavailable.ps1') | Out-Null

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
cargo build --quiet --manifest-path (
    Join-Path $projectRoot 'Cargo.toml'
)
if ($LASTEXITCODE -ne 0) {
    throw 'Rust compatibility build failed.'
}
if (Test-Path -LiteralPath $fixturePath) {
    Remove-Item -LiteralPath $fixturePath -Force
}
[IO.File]::Delete($textRequestPath)
[IO.File]::Delete($editRequestPath)
[IO.File]::Delete($closeRequestPath)

$buildInfo = & $launcher build-info |
    ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or
    -not $buildInfo.ok -or
    $buildInfo.implementation -ne 'cpp') {
    throw 'Compatibility launcher did not route C++ build-info.'
}

$opaqueStale = & $launcher run desktop screenshot `
    --target sessionId=s2:w:0000000000000000 `
    --arg "path=$fixturePath" `
    --confirm |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $opaqueStale.error.code -ne 'STALE_SESSION' -or
    (Test-Path -LiteralPath $fixturePath)) {
    throw 'Opaque screenshot target did not route to fail-closed Rust.'
}

$legacyStale = & $launcher run desktop screenshot `
    --target sessionId=window:1 `
    --arg "path=$fixturePath" `
    --confirm |
    ConvertFrom-Json
if ($LASTEXITCODE -eq 0 -or
    $legacyStale.error.code -ne 'TARGET_NOT_FOUND' -or
    (Test-Path -LiteralPath $fixturePath)) {
    throw 'Legacy screenshot target did not route to Rust compatibility.'
}

$launcherBrowser = & $launcher run browser screenshot `
    --target url=https://example.invalid/ `
    --arg "path=$fixturePath" |
    ConvertFrom-Json
# 浏览器截图已废弃 C++ 路由，直接与 Rust 主程序对照。
$directBrowser = & $rust run browser screenshot `
    --target url=https://example.invalid/ `
    --arg "path=$fixturePath" |
    ConvertFrom-Json
if ($launcherBrowser.ok -or
    $launcherBrowser.error.code -ne
        'CONFIRMATION_REQUIRED' -or
    $directBrowser.error.message -cne
        $launcherBrowser.error.message -or
    (Test-Path -LiteralPath $fixturePath)) {
    throw 'Browser screenshot did not route to the Rust confirmation gate.'
}

$sessions = & $launcher sessions app --max-items 4096 |
    ConvertFrom-Json
# launcher 必须返回 Rust 阶段 3F 的统一 session 发现契约。
if ($LASTEXITCODE -ne 0 -or
    # 固定 capability ID。
    $sessions.capability -cne 'application.session.discover@1' -or
    # 聚合必须只读。
    -not $sessions.readOnly -or
    # 实机只读聚合必须保持前景不变。
    -not $sessions.foregroundUnchanged -or
    # 公开目标只能是 opaque 版本化 session。
    $sessions.targetIdentity -cne 'opaque-versioned-session-id' -or
    # 返回数量必须匹配数组。
    $sessions.count -ne @($sessions.sessions).Count -or
    # 完整数量不能小于返回数量。
    $sessions.total -lt $sessions.count -or
    # 截断标志必须与数量关系一致。
    $sessions.truncated -ne [bool]($sessions.total -gt $sessions.count)) {
    # 报告 launcher 路由或 Rust envelope 漂移。
    throw 'Launcher did not return the Rust application session discovery contract.'
# 结束 launcher session 契约门禁。
}
# 顶层与兼容 data 必须从同一聚合事实生成。
foreach ($field in @(
    # 固定 capability。
    'capability'
    # 固定只读分类。
    'readOnly'
    # 真实前景事实。
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
    # 序列化顶层字段。
    $topValue = $sessions.$field | ConvertTo-Json -Depth 30 -Compress
    # 序列化 data 同名字段。
    $dataValue = $sessions.data.$field | ConvertTo-Json -Depth 30 -Compress
    # 拒绝迁移期外壳漂移。
    if ($topValue -cne $dataValue) {
        # 报告具体漂移字段。
        throw "Launcher application session field drifted: $field"
    # 结束同源字段比较。
    }
# 结束 launcher 同源数据门禁。
}
$textSessions = @(
    $sessions.sessions |
        Where-Object {
            $_.capabilities.id -contains 'text.document.create@1'
        }
)
if ($LASTEXITCODE -ne 0 -or
    $textSessions.Count -ne 1 -or
    $textSessions[0].sessionId -notlike 's2:a:*') {
    # 反向迁移后 launcher 必须从 Rust 发布文本创建器会话.
    throw 'Launcher did not publish the Rust text document session.'
}
$textRequest = [ordered]@{
    target = [ordered]@{
        sessionId = 's2:a:0000000000000000'
    }
    args = [ordered]@{
        capability = 'text.document.create@1'
        input = [ordered]@{ text = 'must-not-write' }
    }
    confirmed = $true
}
[IO.File]::WriteAllText(
    $textRequestPath,
    ($textRequest | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)
$launcherText = & $launcher run app create `
    --input $textRequestPath |
    ConvertFrom-Json
$directText = & $rust run app create `
    --input $textRequestPath |
    ConvertFrom-Json
[IO.File]::Delete($textRequestPath)
if ($launcherText.ok -or
    $launcherText.error.code -ne 'TARGET_NOT_FOUND' -or
    $directText.error.code -ne $launcherText.error.code -or
    (Test-Path -LiteralPath $textRequestPath)) {
    throw 'Opaque text create did not route to fail-closed Rust.'
}
$launcherLegacyText = & $launcher run notepad `
    open-and-write-text --arg text=must-not-write |
    ConvertFrom-Json
$directLegacyText = & $rust run notepad `
    open-and-write-text --arg text=must-not-write |
    ConvertFrom-Json
if ($launcherLegacyText.ok -or
    $launcherLegacyText.error.code -ne
        'CONFIRMATION_REQUIRED' -or
    $directLegacyText.error.message -cne
        $launcherLegacyText.error.message) {
    throw 'Legacy text route did not use the Rust confirmation gate.'
}

$editRequest = [ordered]@{
    target = [ordered]@{
        sessionId = 's2:c:0000000000000000'
    }
    args = [ordered]@{
        capability = 'ui.text.input@1'
        input = [ordered]@{ text = 'must-not-write' }
    }
    confirmed = $true
}
[IO.File]::WriteAllText(
    $editRequestPath,
    ($editRequest | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)
$launcherEdit = & $launcher run app apply `
    --input $editRequestPath |
    ConvertFrom-Json
$directEdit = & $rust run app apply `
    --input $editRequestPath |
    ConvertFrom-Json
[IO.File]::Delete($editRequestPath)
if ($launcherEdit.ok -or
    $launcherEdit.error.code -ne 'STALE_SESSION' -or
    $directEdit.error.code -ne $launcherEdit.error.code -or
    (Test-Path -LiteralPath $editRequestPath)) {
    throw 'Opaque Standard Edit did not route to fail-closed Rust.'
}
$launcherLegacyEdit = & $launcher run win32-control set-text `
    --target sessionId=s2:c:0000000000000000 |
    ConvertFrom-Json
$directLegacyEdit = & $rust run win32-control set-text `
    --target sessionId=s2:c:0000000000000000 |
    ConvertFrom-Json
if ($launcherLegacyEdit.ok -or
    $launcherLegacyEdit.error.code -ne
        'CONFIRMATION_REQUIRED' -or
    $directLegacyEdit.error.message -cne
        $launcherLegacyEdit.error.message) {
    throw 'Legacy Standard Edit did not use the Rust confirmation gate.'
}

$closeRequest = [ordered]@{
    target = [ordered]@{
        sessionId = 's2:w:0000000000000000'
    }
    args = [ordered]@{
        capability = 'window.close@1'
        input = [ordered]@{ timeoutMs = 2000 }
    }
    confirmed = $true
}
[IO.File]::WriteAllText(
    $closeRequestPath,
    ($closeRequest | ConvertTo-Json -Depth 8),
    [Text.UTF8Encoding]::new($false)
)
$launcherClose = & $launcher run app close `
    --input $closeRequestPath |
    ConvertFrom-Json
$directClose = & $rust run app close `
    --input $closeRequestPath |
    ConvertFrom-Json
[IO.File]::Delete($closeRequestPath)
if ($launcherClose.ok -or
    $launcherClose.error.code -ne 'STALE_SESSION' -or
    $directClose.error.code -ne $launcherClose.error.code -or
    (Test-Path -LiteralPath $closeRequestPath)) {
    throw 'Opaque window close did not route to fail-closed Rust.'
}

[PSCustomObject]@{
    ok = $true
    cppReadOnlyRoute = $true
    rustOpaqueScreenshotRoute = $true
    rustLegacyScreenshotFallback = $true
    cppIsolatedBrowserScreenshotRoute = $true
    cppScreenshotGateRequired = $false
    cppTextSessionRoute = $true
    cppOpaqueTextCreateRoute = $true
    cppLegacyTextRoute = $true
    rustOpaqueStandardEditRoute = $true
    rustLegacyStandardEditRoute = $true
    rustOpaqueWindowCloseRoute = $true
    filesWritten = 0
    userApplicationsCaptured = 0
} | ConvertTo-Json
$global:LASTEXITCODE = 0
