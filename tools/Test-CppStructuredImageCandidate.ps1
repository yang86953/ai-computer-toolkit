[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$fixture = Join-Path $outputRoot 'structured-image-candidate-fixture'
$test = Join-Path $outputRoot 'act-structured-image-candidate-test.exe'
$compiler = (Get-Command clang++ -ErrorAction Stop).Source

& (Join-Path $PSScriptRoot 'Build-Cpp.ps1') | Out-Null
$sources = @(
    (Join-Path $sourceRoot 'tests\structured_image_candidate_test.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\output_path_policy.cpp')
    (Join-Path $sourceRoot 'src\components\cancellation.cpp')
    (Join-Path $sourceRoot 'src\components\worker_process.cpp')
    (Join-Path $sourceRoot 'src\modules\structured_image_module.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\structured_image_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\structured_image_observation_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
)
& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    -DUNICODE `
    -D_UNICODE `
    -DWIN32_LEAN_AND_MEAN `
    -DNOMINMAX `
    -D_WIN32_WINNT=0x0A00 `
    "-I$(Join-Path $sourceRoot 'src')" `
    @sources `
    -lole32 `
    -loleaut32 `
    -luuid `
    -luser32 `
    -ladvapi32 `
    -ldwmapi `
    -o $test
if ($LASTEXITCODE -ne 0) {
    throw 'Structured image candidate compilation failed.'
}
& $test $fixture
if ($LASTEXITCODE -ne 0) {
    throw 'Structured image candidate test failed.'
}
if (Test-Path -LiteralPath $fixture) {
    throw 'Structured image candidate fixture was not cleaned.'
}
# 定位刚刚构建的 C++ 公开主入口。
$cpp = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
# 要求当前环境提供正式 Rust 工具链。
$cargo = (Get-Command cargo -ErrorAction Stop).Source
# 构建当前 Rust 主实现以供会话集合对照。
& $cargo build --quiet --manifest-path (Join-Path $projectRoot 'Cargo.toml')
# 构建失败时不得继续使用旧二进制。
if ($LASTEXITCODE -ne 0) {
    # 返回可定位的 Rust 构建门禁错误。
    throw 'Rust structured image comparison build failed.'
# 结束 Rust 构建门禁。
}
# 定位与当前源码对应的 Rust 公开主入口。
$rust = Join-Path $projectRoot 'target\debug\ai-computer-toolkit.exe'
# 从 C++ 主入口取得同一时点附近的应用会话。
$cppSessions = & $cpp sessions app --max-items 4096 |
    # 将公开 JSON 转换为可检查对象。
    ConvertFrom-Json
# 拒绝任何 C++ 会话枚举失败。
if ($LASTEXITCODE -ne 0) {
    # 返回结构化图像 C++ 观测门禁错误。
    throw 'C++ structured image session discovery failed.'
# 结束 C++ 会话枚举门禁。
}
# 从 Rust 主入口取得同一时点附近的应用会话。
$rustSessions = & $rust sessions app --max-items 4096 |
    # 将公开 JSON 转换为可检查对象。
    ConvertFrom-Json
# 拒绝任何 Rust 会话枚举失败。
if ($LASTEXITCODE -ne 0) {
    # 返回结构化图像 Rust 观测门禁错误。
    throw 'Rust structured image session discovery failed.'
# 结束 Rust 会话枚举门禁。
}
# 选取 C++ 的结构化图像应用与文档会话。
$cppStructured = @(
    # 遍历 C++ 公开会话。
    $cppSessions.sessions |
        # 应用标题或文档保存 capability 必须命中。
        Where-Object {
            # 固定 application kind 与标题共同标识 C++ 结构化图像应用。
            ($_.kind -eq 'application' -and $_.title -eq 'Structured image editor') -or
            # 固定 document kind 与保存 capability 共同标识文档。
            ($_.kind -eq 'document' -and $_.capabilities.id -contains 'artifact.save@1')
        # 结束 C++ 结构化图像筛选。
        }
# 结束 C++ 会话数组。
)
# 选取 Rust 的结构化图像应用与文档会话。
$rustStructured = @(
    # 遍历 Rust 公开会话。
    $rustSessions.sessions |
        # 画布创建或文档保存 capability 必须命中。
        Where-Object {
            # 固定 application kind 与创建 capability 共同标识 Rust 应用。
            ($_.kind -eq 'application' -and $_.capabilities.id -contains 'image.canvas.create@1') -or
            # 固定 document kind 与保存 capability 共同标识 Rust 文档。
            ($_.kind -eq 'document' -and $_.capabilities.id -contains 'artifact.save@1')
        # 结束 Rust 结构化图像筛选。
        }
# 结束 Rust 会话数组。
)
# 检查两种实现的所有结构化图像公开会话。
foreach ($session in @($cppStructured) + @($rustStructured)) {
    # 应用与文档必须分别使用 canonical s2 kind。
    if (($session.kind -eq 'application' -and $session.sessionId -notlike 's2:a:*') -or
        # 文档会话必须使用 canonical s2:d。
        ($session.kind -eq 'document' -and $session.sessionId -notlike 's2:d:*')) {
        # 拒绝旧 s1、错误 kind 或非 canonical 会话。
        throw 'Structured image session kind is not canonical s2.'
    # 结束 canonical kind 检查。
    }
    # 禁止公开会话泄漏原生进程 ID。
    if ($session.PSObject.Properties.Name -contains 'processId') {
        # 返回原生进程身份泄漏错误。
        throw 'Structured image session exposed a native process ID.'
    # 结束进程隐私检查。
    }
    # 禁止文档详情泄漏 provider 内部源路径。
    if ($null -ne $session.document -and
        # 只在实际存在 path 属性时判定泄漏。
        $session.document.PSObject.Properties.Name -contains 'path') {
        # 返回原生文档源路径泄漏错误。
        throw 'Structured image session exposed a native document path.'
    # 结束文档路径隐私检查。
    }
# 结束两种实现的公开会话检查。
}
# 取得排序后的 C++ 结构化图像会话 ID 集合。
$cppIds = @($cppStructured.sessionId | Sort-Object)
# 取得排序后的 Rust 结构化图像会话 ID 集合。
$rustIds = @($rustStructured.sessionId | Sort-Object)
# 将 C++ ID 数组序列化为稳定比较形状。
$cppIdJson = ConvertTo-Json -Compress -InputObject $cppIds
# 将 Rust ID 数组序列化为稳定比较形状。
$rustIdJson = ConvertTo-Json -Compress -InputObject $rustIds
# 要求两种实现在同一观测窗口中发布逐字节相同的会话集合。
if ($cppIdJson -cne $rustIdJson) {
    # 返回可定位的跨实现会话集合分歧错误。
    throw 'Rust/C++ structured image session sets diverged.'
# 结束会话集合等价检查。
}
[PSCustomObject]@{
    ok = $true
    confirmationFirst = $true
    exactOpaqueDocumentTargetRequired = $true
    attachOnlyCom = $true
    applicationLaunchAllowed = $false
    arbitraryScriptAccepted = $false
    overwriteProtection = $true
    outcomeUnknownIsNotRetrySafe = $true
    nativeProviderIdentityExposed = $false
    # 记录 Rust 与 C++ 结构化图像会话集合已逐字节对齐。
    rustCppSessionSetEquivalent = $true
    # 记录 C++ 纯身份夹具已命中与 Rust 相同的 golden。
    rustCppIdentityGoldenEquivalent = $true
    # 记录公开会话不暴露 provider 内部源路径。
    sourceDocumentPathExposed = $false
    userDocumentsWritten = 0
    publicRouteEnabled = $false
} | ConvertTo-Json
