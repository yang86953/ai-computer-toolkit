# 声明无参数纯 C++ 碰撞测试入口。
[CmdletBinding()]
# 保持统一脚本参数入口。
param()

# 让编译或执行失败立即终止。
$ErrorActionPreference = 'Stop'
# 定位项目根目录。
$projectRoot = Split-Path -Parent $PSScriptRoot
# 定位 C++ 源码根目录。
$sourceRoot = Join-Path $projectRoot 'cpp'
# 定位测试输出目录。
$outputRoot = Join-Path $projectRoot 'build\cpp-main'

# 先执行无需编译器的静态策略门禁。
& (Join-Path $PSScriptRoot 'Test-CppOpaqueTargetStaticPolicy.ps1') | Out-Null
# 严格要求项目指定的 clang++ 编译器。
$compiler = (Get-Command clang++ -ErrorAction Stop).Source
# 确保测试输出目录存在。
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
# 定位纯唯一匹配测试可执行文件。
$test = Join-Path $outputRoot 'act-opaque-target-match-test.exe'

# 使用项目严格警告策略编译无平台依赖测试。
& $compiler `
    -std=c++23 `
    -Wall `
    -Wextra `
    -Wpedantic `
    -Werror `
    "-I$(Join-Path $sourceRoot 'src')" `
    (Join-Path $sourceRoot 'tests\opaque_target_match_test.cpp') `
    -o $test
# 编译失败时返回清晰门禁错误。
if ($LASTEXITCODE -ne 0) {
    # 阻止未编译的测试被误报通过。
    throw 'Opaque target unique-match test compilation failed.'
}

# 执行 Missing、Unique 与 Ambiguous 三态测试。
& $test
# 任一三态断言失败都会阻止门禁通过。
if ($LASTEXITCODE -ne 0) {
    # 报告纯碰撞测试失败。
    throw 'Opaque target unique-match test failed.'
}

# 输出可由自动化读取的编译门禁证据。
[PSCustomObject]@{
    # 标记门禁成功。
    ok = $true
    # 记录三个已验证状态。
    states = @('missing', 'unique', 'ambiguous')
    # 记录多命中不暴露任意位置。
    ambiguousPositionExposed = $false
} | ConvertTo-Json
