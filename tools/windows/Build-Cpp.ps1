[CmdletBinding()]
param(
    [switch] $Clean
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'cpp'
$outputRoot = Join-Path $projectRoot 'build\cpp-main'
$executable = Join-Path $outputRoot 'ai-computer-toolkit-cpp.exe'
$workerExecutable = Join-Path $outputRoot 'ai-computer-toolkit-observation-worker.exe'
$captureWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-capture-worker.exe'
$mediaWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-media-worker.exe'
$mediaControlWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-media-control-worker.exe'
$structuredImageWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-structured-image-worker.exe'
$browserWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-browser-worker.exe'
$recordingWorkerExecutable =
    Join-Path $outputRoot 'ai-computer-toolkit-recording-worker.exe'

if ($Clean -and (Test-Path -LiteralPath $outputRoot)) {
    $resolvedOutput = [System.IO.Path]::GetFullPath($outputRoot)
    $resolvedBuild = [System.IO.Path]::GetFullPath((Join-Path $projectRoot 'build'))
    if (-not $resolvedOutput.StartsWith(
        $resolvedBuild + [System.IO.Path]::DirectorySeparatorChar,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Refusing to clean output outside the project build directory."
    }
    Remove-Item -LiteralPath $resolvedOutput -Recurse -Force
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null

$compiler = (Get-Command clang++ -ErrorAction Stop).Source
$sources = @(
    (Join-Path $sourceRoot 'src\app\main.cpp')
    (Join-Path $sourceRoot 'src\components\cancellation.cpp')
    (Join-Path $sourceRoot 'src\components\companion_file.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\json_input.cpp')
    (Join-Path $sourceRoot 'src\components\key_chord.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\output_path_policy.cpp')
    (Join-Path $sourceRoot 'src\components\recording_config.cpp')
    (Join-Path $sourceRoot 'src\components\recording_commit.cpp')
    (Join-Path $sourceRoot 'src\components\static_permission_assessment.cpp')
    (Join-Path $sourceRoot 'src\components\utf8.cpp')
    (Join-Path $sourceRoot 'src\components\worker_process.cpp')
    (Join-Path $sourceRoot 'src\control_system.cpp')
    (Join-Path $sourceRoot 'src\modules\application_discovery_module.cpp')
    (Join-Path $sourceRoot 'src\modules\application_launch_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\application_launch_module.cpp')
    (Join-Path $sourceRoot 'src\modules\browser_screenshot_module.cpp')
    (Join-Path $sourceRoot 'src\modules\capability_assessment_module.cpp')
    (Join-Path $sourceRoot 'src\modules\capability_directory_module.cpp')
    (Join-Path $sourceRoot 'src\modules\discovery_module.cpp')
    (Join-Path $sourceRoot 'src\modules\environment_capability_module.cpp')
    (Join-Path $sourceRoot 'src\modules\foreground_input_module.cpp')
    (Join-Path $sourceRoot 'src\modules\media_control_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\media_session_module.cpp')
    (Join-Path $sourceRoot 'src\modules\recording_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\recording_module.cpp')
    (Join-Path $sourceRoot 'src\modules\screenshot_module.cpp')
    (Join-Path $sourceRoot 'src\modules\screenshot_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\standard_edit_module.cpp')
    (Join-Path $sourceRoot 'src\modules\standard_edit_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\structured_image_module.cpp')
    (Join-Path $sourceRoot 'src\modules\text_document_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\text_document_module.cpp')
    (Join-Path $sourceRoot 'src\modules\window_close_compatibility_module.cpp')
    (Join-Path $sourceRoot 'src\modules\window_close_module.cpp')
    (Join-Path $sourceRoot 'src\systems\application_facade_system.cpp')
    (Join-Path $sourceRoot 'src\systems\application_launch_command_system.cpp')
    (Join-Path $sourceRoot 'src\systems\browser_command_system.cpp')
    (Join-Path $sourceRoot 'src\systems\media_command_system.cpp')
    (Join-Path $sourceRoot 'src\systems\readonly_diagnostic_system.cpp')
    (Join-Path $sourceRoot 'src\systems\recording_command_system.cpp')
    (Join-Path $sourceRoot 'src\systems\screenshot_command_system.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\browser_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\application_launch_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\capture_preflight_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\capture_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\environment_capability_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\foreground_input_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\installed_application_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\media_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\observation_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\png_file_output.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\recording_worker_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\shell_application_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\standard_edit_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\structured_image_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\structured_image_observation_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_document_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\window_close_backend.cpp')
)
$commonArguments = @(
    '-std=c++23'
    '-Wall'
    '-Wextra'
    '-Wpedantic'
    '-Werror'
    '-DUNICODE'
    '-D_UNICODE'
    '-DWIN32_LEAN_AND_MEAN'
    '-DNOMINMAX'
    '-D_WIN32_WINNT=0x0A00'
    "-I$(Join-Path $sourceRoot 'include')"
    "-I$(Join-Path $sourceRoot 'src')"
)
$linkArguments = @(
    '-lole32'
    '-loleaut32'
    '-luuid'
    '-luser32'
    '-ladvapi32'
    '-ldwmapi'
    '-lshell32'
    '-lpropsys'
    '-lruntimeobject'
)
$arguments = $commonArguments + $sources + $linkArguments + @(
    '-o'
    $executable
)

& $compiler @arguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ build failed with exit code $LASTEXITCODE."
}
$legacyCatalog = Join-Path `
    $projectRoot 'contracts\compat\legacy-public-catalog-v1.json'
Copy-Item -LiteralPath $legacyCatalog -Destination (
    Join-Path $outputRoot 'legacy-public-catalog-v1.json'
) -Force

$workerSources = @(
    (Join-Path $sourceRoot 'src\worker\observation_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
)
$workerArguments =
    $commonArguments + $workerSources + $linkArguments + @(
        '-o'
        $workerExecutable
    )

& $compiler @workerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ observation worker build failed with exit code $LASTEXITCODE."
}

$sdkIncludeRoot = 'C:\Program Files (x86)\Windows Kits\10\Include'
$cppWinRt = Get-ChildItem -LiteralPath $sdkIncludeRoot -Directory |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'cppwinrt' } |
    Where-Object {
        Test-Path -LiteralPath (Join-Path $_ 'winrt\base.h')
    } |
    Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($cppWinRt)) {
    throw 'Windows SDK C++/WinRT headers are unavailable.'
}
$captureWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\capture_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\pixel_buffer.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\capture_preflight_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\png_encoder.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\png_file_output.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\recording_capture_probe.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\wgc_capture.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\wgc_recording_entrypoints.cpp')
)
$captureWorkerArguments =
    $commonArguments +
    @('-Wno-nonportable-include-path', "-I$cppWinRt") +
    $captureWorkerSources +
    $linkArguments +
    @(
        '-ld3d11'
        '-ldxgi'
        '-lgdi32'
        '-lwindowscodecs'
        '-o'
        $captureWorkerExecutable
    )

& $compiler @captureWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ capture worker build failed with exit code $LASTEXITCODE."
}

$mediaWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\media_observation_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
)
$mediaWorkerArguments =
    $commonArguments +
    @('-Wno-nonportable-include-path', "-I$cppWinRt") +
    $mediaWorkerSources +
    $linkArguments +
    @(
        '-o'
        $mediaWorkerExecutable
    )

& $compiler @mediaWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ media observation worker build failed with exit code $LASTEXITCODE."
}

$mediaControlWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\media_control_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
)
$mediaControlWorkerArguments =
    $commonArguments +
    @('-Wno-nonportable-include-path', "-I$cppWinRt") +
    $mediaControlWorkerSources +
    $linkArguments +
    @(
        '-o'
        $mediaControlWorkerExecutable
    )

& $compiler @mediaControlWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ media control worker build failed with exit code $LASTEXITCODE."
}

$structuredImageWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\structured_image_observation_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\output_path_policy.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\process_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\structured_image_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
)
$structuredImageWorkerArguments =
    $commonArguments + $structuredImageWorkerSources + $linkArguments + @(
        '-o'
        $structuredImageWorkerExecutable
    )

& $compiler @structuredImageWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ structured image worker build failed with exit code $LASTEXITCODE."
}

$browserWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\browser_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
)
$browserWorkerArguments =
    $commonArguments + $browserWorkerSources + $linkArguments + @(
        '-o'
        $browserWorkerExecutable
    )

& $compiler @browserWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ browser worker build failed with exit code $LASTEXITCODE."
}

$recordingWorkerSources = @(
    (Join-Path $sourceRoot 'src\worker\recording_worker_main.cpp')
    (Join-Path $sourceRoot 'src\components\json.cpp')
    (Join-Path $sourceRoot 'src\components\opaque_id.cpp')
    (Join-Path $sourceRoot 'src\components\pixel_buffer.cpp')
    (Join-Path $sourceRoot 'src\components\recording_analysis.cpp')
    (Join-Path $sourceRoot 'src\components\recording_config.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\ffmpeg_encoder.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\ffmpeg_stream_encoder.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\capture_preflight_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\discovery_backend.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\png_encoder.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\png_file_output.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\recording_stream_pipeline.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\text_codec.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\wgc_capture.cpp')
    (Join-Path $sourceRoot 'src\platform\windows\wgc_recording_entrypoints.cpp')
)
$recordingWorkerArguments =
    $commonArguments +
    @('-Wno-nonportable-include-path', "-I$cppWinRt") +
    $recordingWorkerSources +
    $linkArguments +
    @(
        '-ld3d11'
        '-ldxgi'
        '-lgdi32'
        '-lwindowscodecs'
        '-o'
        $recordingWorkerExecutable
    )

& $compiler @recordingWorkerArguments
if ($LASTEXITCODE -ne 0) {
    throw "C++ recording worker build failed with exit code $LASTEXITCODE."
}

[PSCustomObject]@{
    compiler = $compiler
    executable = $executable
    workerExecutable = $workerExecutable
    captureWorkerExecutable = $captureWorkerExecutable
    mediaWorkerExecutable = $mediaWorkerExecutable
    mediaControlWorkerExecutable = $mediaControlWorkerExecutable
    browserWorkerExecutable = $browserWorkerExecutable
    recordingWorkerExecutable = $recordingWorkerExecutable
    sourceCount = $sources.Count
    workerSourceCount = $workerSources.Count
    captureWorkerSourceCount = $captureWorkerSources.Count
    mediaWorkerSourceCount = $mediaWorkerSources.Count
    mediaControlWorkerSourceCount = $mediaControlWorkerSources.Count
    browserWorkerSourceCount = $browserWorkerSources.Count
    recordingWorkerSourceCount = $recordingWorkerSources.Count
    cppWinRt = $cppWinRt
    cmakeProject = Join-Path $sourceRoot 'CMakeLists.txt'
} | ConvertTo-Json
