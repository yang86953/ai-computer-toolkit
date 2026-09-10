# 声明该脚本支持 PowerShell 通用参数.
[CmdletBinding()]
# 声明该回归测试不接受业务参数.
param()

# 让任何未处理错误立即终止测试.
$ErrorActionPreference = 'Stop'
# 解析当前仓库根目录.
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位待验证的 launcher 脚本.
$launcher = Join-Path $PSScriptRoot 'Invoke-ComputerControl.ps1'
# 生成唯一且可恢复清理的临时夹具目录.
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("act-launcher-runtime-unavailable-$([guid]::NewGuid().ToString('N'))")
# 规范化系统临时目录边界.
$resolvedTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
# 规范化本次夹具目录边界.
$resolvedFixtureRoot = [IO.Path]::GetFullPath($fixtureRoot)
# 拒绝任何逃逸出系统临时目录的递归清理目标.
if (-not $resolvedFixtureRoot.StartsWith($resolvedTempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Launcher runtime fixture escaped the temporary directory.' }
# 拒绝缺少专用前缀的递归清理目标.
if ([IO.Path]::GetFileName($resolvedFixtureRoot) -notlike 'act-launcher-runtime-unavailable-*') { throw 'Launcher runtime fixture has an unsafe directory name.' }
# 定位模拟项目中的 tools 目录.
$fixtureTools = Join-Path $resolvedFixtureRoot 'tools'
# 创建不包含 Rust runtime 的模拟项目结构。
New-Item -ItemType Directory -Path $fixtureTools -Force | Out-Null
# 将 launcher 复制到模拟项目中以触发运行时缺失分支.
Copy-Item -LiteralPath $launcher -Destination $fixtureTools

# 确保成功或失败后都清理唯一临时夹具.
try {
    # 定位模拟项目中的 launcher 副本.
    $fixtureLauncher = Join-Path $fixtureTools 'Invoke-ComputerControl.ps1'
    # 在独立 PowerShell 进程中调用默认 Rust 路由并解析结构化失败结果.
    $rustFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher help | ConvertFrom-Json
    # 保存 Rust 路由的进程退出码.
    $rustExitCode = $LASTEXITCODE
    # 验证 Rust 路由使用兼容运行时错误码和固定退出码.
    if ($rustExitCode -ne 2 -or $rustFailure.ok -or $rustFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Rust runtime-unavailable launcher result is invalid.' }
    # 调用 capability descriptor 以验证阶段 5 静态状态固定选择 Rust。
    $capabilitiesFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher capabilities descriptor desktop screenshot | ConvertFrom-Json
    # 保存 capability descriptor 路由的退出码。
    $capabilitiesExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时不得回退旧 C++ descriptor。
    if ($capabilitiesExitCode -ne 2 -or $capabilitiesFailure.ok -or $capabilitiesFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Capabilities did not select the Rust runtime.' }
    # 调用 build-info 以验证构建元数据固定选择 Rust。
    $buildInfoFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher build-info | ConvertFrom-Json
    # 保存 build-info 路由的退出码。
    $buildInfoExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时不得回退旧 C++ 构建信息。
    if ($buildInfoExitCode -ne 2 -or $buildInfoFailure.ok -or $buildInfoFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Build info did not select the Rust runtime.' }
    # 调用完整 status 以验证全部诊断 surface 固定选择 Rust。
    $statusFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher status | ConvertFrom-Json
    # 保存完整 status 路由的退出码。
    $statusExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时不得回退旧 C++ 诊断。
    if ($statusExitCode -ne 2 -or $statusFailure.ok -or $statusFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Status did not select the Rust runtime.' }
    # 调用完整 doctor 以验证聚合诊断固定选择 Rust。
    $doctorFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher doctor | ConvertFrom-Json
    # 保存完整 doctor 路由的退出码。
    $doctorExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时不得回退旧 C++ 诊断。
    if ($doctorExitCode -ne 2 -or $doctorFailure.ok -or $doctorFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Doctor did not select the Rust runtime.' }
    # 调用 sessions app 以验证只读会话面不再选择 C++.
    $sessionsFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher sessions app | ConvertFrom-Json
    # 保存 sessions app 路由的进程退出码.
    $sessionsExitCode = $LASTEXITCODE
    # 缺失双 runtime 时必须报告 Rust 运行时不可用.
    if ($sessionsExitCode -ne 2 -or $sessionsFailure.ok -or $sessionsFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Sessions did not select the Rust runtime.' }
    # 调用 inspect app 以验证精确检查面不再选择 C++.
    $inspectFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher inspect app --target sessionId=s2:a:0000000000000000 | ConvertFrom-Json
    # 保存 inspect app 路由的进程退出码.
    $inspectExitCode = $LASTEXITCODE
    # 缺失双 runtime 时必须报告 Rust 运行时不可用.
    if ($inspectExitCode -ne 2 -or $inspectFailure.ok -or $inspectFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Inspect did not select the Rust runtime.' }
    # 调用完整应用发现以验证阶段 3C 不再选择 C++.
    $discoverFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher discover app | ConvertFrom-Json
    # 保存 discover app 路由的进程退出码.
    $discoverExitCode = $LASTEXITCODE
    # 缺失双 runtime 时必须报告 Rust 运行时不可用.
    if ($discoverExitCode -ne 2 -or $discoverFailure.ok -or $discoverFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Discover did not select the Rust runtime.' }
    # 调用 assess app 以验证阶段 3E 不再选择 C++。
    $assessFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher assess app --capability process.metadata.read@1 --target sessionId=s2:p:0000000000000000 | ConvertFrom-Json
    # 保存 assess app 路由的进程退出码。
    $assessExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时必须结构化失败且不回退 C++。
    if ($assessExitCode -ne 2 -or $assessFailure.ok -or $assessFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Assess did not select the Rust runtime.' }
    # 保存稳定字符串，避免后续 PowerShell 错误流复用对象属性。
    $assessCode = [string] $assessFailure.error.code
    # 调用 provider-neutral 文本新建以验证阶段 4A 固定选择 Rust。
    $textFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run app create --input missing-fixture.json --confirm | ConvertFrom-Json
    # 保存文本新建路由的退出码。
    $textExitCode = $LASTEXITCODE
    # runtime 缺失必须先由 launcher 返回 Rust 结构化错误。
    if ($textExitCode -ne 2 -or $textFailure.ok -or $textFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Text document create did not select the Rust runtime.' }
    # 调用 legacy 文本新建以验证旧入口同步切回 Rust。
    $legacyTextFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run notepad open-and-write-text --arg text=fixture --confirm | ConvertFrom-Json
    # 保存 legacy 路由退出码。
    $legacyTextExitCode = $LASTEXITCODE
    # legacy 入口也不得回退 C++。
    if ($legacyTextExitCode -ne 2 -or $legacyTextFailure.ok -or $legacyTextFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Legacy text document create did not select the Rust runtime.' }
    # 调用 provider-neutral Standard Edit 以验证阶段 4B 固定选择 Rust。
    $editFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run app apply --input missing-edit-fixture.json --confirm | ConvertFrom-Json
    # 保存 provider-neutral Standard Edit 路由退出码。
    $editExitCode = $LASTEXITCODE
    # runtime 缺失必须先由 launcher 返回 Rust 结构化错误。
    if ($editExitCode -ne 2 -or $editFailure.ok -or $editFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Standard Edit app apply did not select the Rust runtime.' }
    # 调用 legacy Standard Edit 以验证兼容 mapper 同步切回 Rust。
    $legacyEditFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run win32-control set-text --target sessionId=s2:c:0000000000000000 | ConvertFrom-Json
    # 保存 legacy Standard Edit 路由退出码。
    $legacyEditExitCode = $LASTEXITCODE
    # legacy Standard Edit 不得回退 C++。
    if ($legacyEditExitCode -ne 2 -or $legacyEditFailure.ok -or $legacyEditFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Legacy Standard Edit did not select the Rust runtime.' }
    # 调用 desktop Standard Edit 兼容入口以验证 s2:c 固定选择 Rust。
    $desktopEditFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run desktop type-text --target sessionId=s2:c:0000000000000000 --arg text=fixture --confirm | ConvertFrom-Json
    # 保存 desktop Standard Edit 路由退出码。
    $desktopEditExitCode = $LASTEXITCODE
    # desktop opaque 控件入口不得回退 C++。
    if ($desktopEditExitCode -ne 2 -or $desktopEditFailure.ok -or $desktopEditFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Desktop Standard Edit did not select the Rust runtime.' }
    # 调用 provider-neutral Window Close 以验证阶段 4C 固定选择 Rust。
    $closeFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run app close --input missing-close-fixture.json --confirm | ConvertFrom-Json
    # 保存 provider-neutral Window Close 路由退出码。
    $closeExitCode = $LASTEXITCODE
    # runtime 缺失必须先由 launcher 返回 Rust 结构化错误。
    if ($closeExitCode -ne 2 -or $closeFailure.ok -or $closeFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Window close app route did not select the Rust runtime.' }
    # 调用 desktop 精确关闭兼容入口以验证 s2:w 固定选择 Rust。
    $desktopCloseFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run desktop close --target sessionId=s2:w:0000000000000000 --confirm | ConvertFrom-Json
    # 保存 desktop Window Close 路由退出码。
    $desktopCloseExitCode = $LASTEXITCODE
    # desktop 精确窗口入口不得回退 C++。
    if ($desktopCloseExitCode -ne 2 -or $desktopCloseFailure.ok -or $desktopCloseFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Desktop window close did not select the Rust runtime.' }
    # 调用 Standard Edit status 以验证只读汇总同步切回 Rust。
    $editStatusFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher status win32-control | ConvertFrom-Json
    # 保存 Standard Edit status 路由退出码。
    $editStatusExitCode = $LASTEXITCODE
    # status 不得继续依赖 C++ runtime。
    if ($editStatusExitCode -ne 2 -or $editStatusFailure.ok -or $editStatusFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Standard Edit status did not select the Rust runtime.' }
    # 调用有界可访问性树以验证阶段 3D 不再选择 C++.
    $treeFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher inspect-tree uia --target sessionId=s2:w:0000000000000000 | ConvertFrom-Json
    # 保存 inspect-tree 路由的进程退出码.
    $treeExitCode = $LASTEXITCODE
    # 缺失双 runtime 时必须报告 Rust 运行时不可用.
    if ($treeExitCode -ne 2 -or $treeFailure.ok -or $treeFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Inspect-tree did not select the Rust runtime.' }
    # 调用 desktop screenshot 以验证阶段 5B 不再选择 C++。
    $screenshotFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run desktop screenshot --target sessionId=s2:w:0000000000000000 --arg path=fixture.png --confirm | ConvertFrom-Json
    # 保存 desktop screenshot 路由的进程退出码。
    $screenshotExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时必须结构化失败且不回退 C++。
    if ($screenshotExitCode -ne 2 -or $screenshotFailure.ok -or $screenshotFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Window screenshot did not select the Rust runtime.' }
    # 调用 app screenshot 以验证 provider-neutral 路由同样固定选择 Rust。
    $appScreenshotFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run app screenshot --input missing.json --confirm | ConvertFrom-Json
    # 保存 app screenshot 路由的进程退出码。
    $appScreenshotExitCode = $LASTEXITCODE
    # launcher 必须在缺失请求文件解析前报告 Rust runtime 缺失。
    if ($appScreenshotExitCode -ne 2 -or $appScreenshotFailure.ok -or $appScreenshotFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'App screenshot did not select the Rust runtime.' }
    # 调用 browser screenshot 以验证阶段 5C 固定选择 Rust。
    $browserScreenshotFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run browser screenshot --target url=https://example.invalid/ --arg path=fixture.png --confirm | ConvertFrom-Json
    # 保存 browser screenshot 路由的进程退出码。
    $browserScreenshotExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时必须结构化失败且不回退 C++。
    if ($browserScreenshotExitCode -ne 2 -or $browserScreenshotFailure.ok -or $browserScreenshotFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Browser screenshot did not select the Rust runtime.' }
    # 调用首帧探针以验证阶段 5B 不再选择 C++。
    $captureProbeFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher probe-capture-frame app --target sessionId=s2:w:0000000000000000 --confirm | ConvertFrom-Json
    # 保存首帧探针路由的进程退出码。
    $captureProbeExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时必须结构化失败且不回退 C++。
    if ($captureProbeExitCode -ne 2 -or $captureProbeFailure.ok -or $captureProbeFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Capture frame probe did not select the Rust runtime.' }
    # 调用 desktop record 以验证阶段 5D 固定选择 Rust。
    $desktopRecordFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run desktop record --target sessionId=s2:w:0000000000000000 --arg path=fixture.mp4 --confirm | ConvertFrom-Json
    # 保存 desktop record 路由的进程退出码。
    $desktopRecordExitCode = $LASTEXITCODE
    # 缺少 Rust runtime 时必须结构化失败且不回退 C++。
    if ($desktopRecordExitCode -ne 2 -or $desktopRecordFailure.ok -or $desktopRecordFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Desktop recording did not select the Rust runtime.' }
    # 调用 app record 以验证 provider-neutral 路由不再读取请求内容选择 C++。
    $appRecordFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run app record --input missing-recording.json --confirm | ConvertFrom-Json
    # 保存 app record 路由的进程退出码。
    $appRecordExitCode = $LASTEXITCODE
    # launcher 必须在缺失请求文件解析前报告 Rust runtime 缺失。
    if ($appRecordExitCode -ne 2 -or $appRecordFailure.ok -or $appRecordFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'App recording did not select the Rust runtime.' }
    # 调用未认证媒体入口并验证 launcher 仍只选择 Rust runtime。
    $mediaFailure = & powershell -NoProfile -ExecutionPolicy Bypass -File $fixtureLauncher run media-session pause --target sessionId=s2:m:0000000000000000 --confirm | ConvertFrom-Json
    # 保存媒体路由的进程退出码。
    $mediaExitCode = $LASTEXITCODE
    # Rust runtime 缺失必须先于媒体 provider 解析稳定失败。
    if ($mediaExitCode -ne 2 -or $mediaFailure.ok -or $mediaFailure.error.code -ne 'COMPATIBILITY_RUNTIME_UNAVAILABLE') { throw 'Media route did not select the Rust runtime.' }
    # 输出便于自动化消费的测试摘要.
    # 输出两个只读路由的 Rust 运行时选择证据.
    # 输出全部受检生产路由的稳定错误码。
    [ordered]@{ ok = $true; rustCode = $rustFailure.error.code; capabilitiesCode = $capabilitiesFailure.error.code; buildInfoCode = $buildInfoFailure.error.code; statusCode = $statusFailure.error.code; doctorCode = $doctorFailure.error.code; sessionsCode = $sessionsFailure.error.code; inspectCode = $inspectFailure.error.code; discoverCode = $discoverFailure.error.code; assessCode = $assessCode; textCreateCode = $textFailure.error.code; legacyTextCode = $legacyTextFailure.error.code; editApplyCode = $editFailure.error.code; legacyEditCode = $legacyEditFailure.error.code; desktopEditCode = $desktopEditFailure.error.code; closeCode = $closeFailure.error.code; desktopCloseCode = $desktopCloseFailure.error.code; editStatusCode = $editStatusFailure.error.code; inspectTreeCode = $treeFailure.error.code; screenshotCode = $screenshotFailure.error.code; appScreenshotCode = $appScreenshotFailure.error.code; browserScreenshotCode = $browserScreenshotFailure.error.code; captureProbeCode = $captureProbeFailure.error.code; desktopRecordCode = $desktopRecordFailure.error.code; appRecordCode = $appRecordFailure.error.code; mediaCode = $mediaFailure.error.code } | ConvertTo-Json -Compress
# 无论断言结果如何都进入受控清理.
} finally {
    # 仅在已创建夹具目录时执行递归清理.
    if (Test-Path -LiteralPath $resolvedFixtureRoot) { Remove-Item -LiteralPath $resolvedFixtureRoot -Recurse -Force }
# 结束受控清理区域.
}
