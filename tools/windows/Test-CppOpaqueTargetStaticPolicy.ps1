# 声明无参数静态策略门禁。
[CmdletBinding()]
# 保持统一脚本参数入口。
param()

# 让任意策略检查失败立即终止。
$ErrorActionPreference = 'Stop'
# 定位项目根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位 C++ 源码根目录。
$sourceRoot = Join-Path $projectRoot 'cpp\src'

# 匹配单个 std::find_if 调用块。
$findIfPattern = [regex]::new('std::find_if\([\s\S]*?\}\);', [Text.RegularExpressions.RegexOptions]::Singleline)
# 收集仍以 find-first 解析 sessionId 的源码位置。
$violations = [Collections.Generic.List[string]]::new()
# 枚举受迁移约束的全部 C++ 源文件。
$cppFiles = Get-ChildItem -LiteralPath $sourceRoot -Recurse -Filter '*.cpp' -File
# 检查每个 find_if 调用是否比较 session_id。
foreach ($file in $cppFiles) {
    # 读取完整源码以跨行匹配调用块。
    $text = Get-Content -LiteralPath $file.FullName -Raw
    # 枚举当前文件的所有 find_if 调用。
    foreach ($match in $findIfPattern.Matches($text)) {
        # sessionId 解析不得保留 find-first 语义。
        if ($match.Value -match '(?:\.|\b)session_id\s*==') {
            # 记录相对项目路径供审计定位。
            $violations.Add([IO.Path]::GetRelativePath($projectRoot, $file.FullName))
        }
    }
}
# 任一违规调用都会使静态门禁失败。
if ($violations.Count -ne 0) {
    # 输出去重后的违规清单。
    throw "Opaque target find-first resolvers remain: $((($violations | Sort-Object -Unique) -join ', '))."
}

# 列出必须复用共享唯一匹配组件的源码。
$matcherFiles = @(
    'cpp\src\modules\application_discovery_module.cpp',
    'cpp\src\modules\application_launch_module.cpp',
    'cpp\src\modules\discovery_module.cpp',
    'cpp\src\modules\foreground_input_module.cpp',
    'cpp\src\modules\recording_module.cpp',
    'cpp\src\modules\screenshot_module.cpp',
    'cpp\src\modules\standard_edit_module.cpp',
    'cpp\src\modules\structured_image_module.cpp',
    'cpp\src\modules\window_close_module.cpp',
    'cpp\src\platform\windows\structured_image_backend.cpp',
    'cpp\src\worker\capture_worker_main.cpp',
    'cpp\src\worker\observation_worker_main.cpp',
    'cpp\src\worker\recording_worker_main.cpp'
)
# 验证每个 resolver 同时保留共享匹配和歧义错误。
foreach ($relativePath in $matcherFiles) {
    # 解析待检查源码的绝对路径。
    $path = Join-Path $projectRoot $relativePath
    # 读取待检查源码。
    $text = Get-Content -LiteralPath $path -Raw
    # 缺任一策略标记都表示实现可能回退。
    if ($text -notmatch 'match_opaque_target' -or $text -notmatch 'AMBIGUOUS_TARGET') {
        # 报告缺少策略标记的文件。
        throw "Opaque target policy markers are incomplete: $relativePath."
    }
}

# 读取媒体只读 worker 的显式计数实现。
$mediaObservation = Get-Content -LiteralPath (Join-Path $projectRoot 'cpp\src\worker\media_observation_worker_main.cpp') -Raw
# 验证媒体观察保留零命中与多命中分支。
if ($mediaObservation -notmatch 'std::count_if' -or $mediaObservation -notmatch 'matching\s*==\s*0' -or $mediaObservation -notmatch 'matching\s*>\s*1' -or $mediaObservation -notmatch 'AMBIGUOUS_TARGET') {
    # 阻止媒体观察退回任取一个目标。
    throw 'Media observation no longer proves zero/one/many target resolution.'
}

# 读取媒体控制 worker 的显式候选清单实现。
$mediaControl = Get-Content -LiteralPath (Join-Path $projectRoot 'cpp\src\worker\media_control_worker_main.cpp') -Raw
# 验证媒体控制保留零命中与非唯一分支。
if ($mediaControl -notmatch 'matches\.empty\(\)' -or $mediaControl -notmatch 'matches\.size\(\)\s*!=\s*1U' -or $mediaControl -notmatch 'AMBIGUOUS_TARGET') {
    # 阻止媒体控制退回任取一个目标。
    throw 'Media control no longer proves zero/one/many target resolution.'
}

# 读取纯碰撞测试与 CMake 注册信息。
$pureTest = Get-Content -LiteralPath (Join-Path $projectRoot 'cpp\tests\opaque_target_match_test.cpp') -Raw
# 读取 C++ 构建清单。
$cmake = Get-Content -LiteralPath (Join-Path $projectRoot 'cpp\CMakeLists.txt') -Raw
# 验证三态测试源码及 CTest 注册没有被移除。
if ($pureTest -notmatch 'OpaqueTargetMatchState::missing' -or $pureTest -notmatch 'OpaqueTargetMatchState::unique' -or $pureTest -notmatch 'OpaqueTargetMatchState::ambiguous' -or $cmake -notmatch 'add_executable\(act-opaque-target-match-test' -or $cmake -notmatch 'add_test\(NAME cpp-opaque-target-match') {
    # 阻止只保留实现而删除碰撞门禁。
    throw 'Opaque target pure collision test or CTest registration is incomplete.'
}

# 输出可由自动化读取的静态门禁证据。
[PSCustomObject]@{
    # 标记门禁成功。
    ok = $true
    # 记录共享 matcher 覆盖文件数。
    sharedMatcherFiles = $matcherFiles.Count
    # 记录媒体显式计数边界已验证。
    mediaExplicitCounting = $true
    # 记录纯三态测试已注册。
    pureCollisionTestRegistered = $true
    # 记录 find-first session resolver 数量为零。
    findFirstSessionResolvers = 0
} | ConvertTo-Json
