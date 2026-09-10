cmake_minimum_required(VERSION 3.24)

> **版本化参考**：从原项目资料迁入，保留协议/实验的历史定义，不代表默认构建当前启用或通过实机验收。当前接入以[文档中心](../docs/README.md)、运行时 capability 与同版本 schema 为准；旧 UIX 控制、候选 provider 和 feature 专属路线不自动恢复。

project(ai_computer_toolkit LANGUAGES CXX)

add_executable(ai-computer-toolkit-cpp
    src/app/main.cpp
    src/components/cancellation.cpp
    src/components/companion_file.cpp
    src/components/json.cpp
    src/components/json_input.cpp
    src/components/key_chord.cpp
    src/components/opaque_id.cpp
    src/components/output_path_policy.cpp
    src/components/recording_config.cpp
    src/components/recording_commit.cpp
    src/components/static_permission_assessment.cpp
    src/components/utf8.cpp
    src/components/worker_process.cpp
    src/control_system.cpp
    src/modules/application_discovery_module.cpp
    src/modules/application_launch_compatibility_module.cpp
    src/modules/application_launch_module.cpp
    src/modules/browser_screenshot_module.cpp
    src/modules/capability_assessment_module.cpp
    src/modules/capability_directory_module.cpp
    src/modules/discovery_module.cpp
    src/modules/environment_capability_module.cpp
    src/modules/foreground_input_module.cpp
    src/modules/media_control_compatibility_module.cpp
    src/modules/media_session_module.cpp
    src/modules/recording_compatibility_module.cpp
    src/modules/recording_module.cpp
    src/modules/screenshot_module.cpp
    src/modules/screenshot_compatibility_module.cpp
    src/modules/standard_edit_module.cpp
    src/modules/standard_edit_compatibility_module.cpp
    src/modules/structured_image_module.cpp
    src/modules/text_document_compatibility_module.cpp
    src/modules/text_document_module.cpp
    src/modules/window_close_compatibility_module.cpp
    src/modules/window_close_module.cpp
    src/systems/application_facade_system.cpp
    src/systems/application_launch_command_system.cpp
    src/systems/browser_command_system.cpp
    src/systems/media_command_system.cpp
    src/systems/readonly_diagnostic_system.cpp
    src/systems/recording_command_system.cpp
    src/systems/screenshot_command_system.cpp
    src/platform/windows/browser_worker_backend.cpp
    src/platform/windows/application_launch_backend.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/capture_worker_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/environment_capability_backend.cpp
    src/platform/windows/foreground_input_backend.cpp
    src/platform/windows/installed_application_backend.cpp
    src/platform/windows/media_worker_backend.cpp
    src/platform/windows/observation_worker_backend.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/recording_worker_backend.cpp
    src/platform/windows/shell_application_backend.cpp
    src/platform/windows/standard_edit_backend.cpp
    src/platform/windows/structured_image_backend.cpp
    src/platform/windows/structured_image_observation_backend.cpp
    src/platform/windows/text_document_backend.cpp
    src/platform/windows/text_codec.cpp
    src/platform/windows/window_close_backend.cpp
)
add_custom_command(TARGET ai-computer-toolkit-cpp POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/../contracts/compat/legacy-public-catalog-v1.json"
        "$<TARGET_FILE_DIR:ai-computer-toolkit-cpp>/legacy-public-catalog-v1.json"
)

add_executable(ai-computer-toolkit-observation-worker
    src/worker/observation_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/text_codec.cpp
)

file(GLOB ACT_CPPWINRT_CANDIDATES
    "C:/Program Files (x86)/Windows Kits/10/Include/*/cppwinrt"
)
list(SORT ACT_CPPWINRT_CANDIDATES
    COMPARE NATURAL
    ORDER DESCENDING
)
foreach(candidate IN LISTS ACT_CPPWINRT_CANDIDATES)
    if(EXISTS "${candidate}/winrt/base.h")
        set(ACT_CPPWINRT_INCLUDE "${candidate}")
        break()
    endif()
endforeach()
if(NOT ACT_CPPWINRT_INCLUDE)
    message(FATAL_ERROR
        "Windows SDK C++/WinRT headers are required for capture worker."
    )
endif()

add_executable(ai-computer-toolkit-capture-worker
    src/worker/capture_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/pixel_buffer.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/png_encoder.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/recording_capture_probe.cpp
    src/platform/windows/text_codec.cpp
    src/platform/windows/wgc_capture.cpp
    src/platform/windows/wgc_recording_entrypoints.cpp
)
add_executable(ai-computer-toolkit-media-worker
    src/worker/media_observation_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
)
add_executable(ai-computer-toolkit-media-control-worker
    src/worker/media_control_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
)
add_executable(ai-computer-toolkit-browser-worker
    src/worker/browser_worker_main.cpp
    src/components/json.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(ai-computer-toolkit-structured-image-worker
    src/worker/structured_image_observation_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/output_path_policy.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/structured_image_backend.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(ai-computer-toolkit-recording-worker
    src/worker/recording_worker_main.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/pixel_buffer.cpp
    src/components/recording_analysis.cpp
    src/components/recording_config.cpp
    src/platform/windows/ffmpeg_encoder.cpp
    src/platform/windows/ffmpeg_stream_encoder.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/png_encoder.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/recording_stream_pipeline.cpp
    src/platform/windows/text_codec.cpp
    src/platform/windows/wgc_capture.cpp
    src/platform/windows/wgc_recording_entrypoints.cpp
)
target_include_directories(ai-computer-toolkit-capture-worker PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)
target_include_directories(ai-computer-toolkit-media-worker PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)
target_include_directories(
    ai-computer-toolkit-media-control-worker PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)
target_include_directories(ai-computer-toolkit-recording-worker PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)

foreach(target
    ai-computer-toolkit-cpp
    ai-computer-toolkit-observation-worker
    ai-computer-toolkit-capture-worker
    ai-computer-toolkit-media-worker
    ai-computer-toolkit-media-control-worker
    ai-computer-toolkit-browser-worker
    ai-computer-toolkit-structured-image-worker
    ai-computer-toolkit-recording-worker
)
    target_compile_features(${target} PRIVATE cxx_std_23)
    target_include_directories(${target} PRIVATE include src)
    target_compile_definitions(${target} PRIVATE
        UNICODE
        _UNICODE
        WIN32_LEAN_AND_MEAN
        NOMINMAX
        _WIN32_WINNT=0x0A00
    )

    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /WX /permissive-)
    else()
        target_compile_options(${target} PRIVATE
            -Wall
            -Wextra
            -Wpedantic
            -Werror
        )
    endif()

    target_link_libraries(${target} PRIVATE
        ole32
        oleaut32
        uuid
        user32
        advapi32
        dwmapi
        shell32
        propsys
        runtimeobject
    )
endforeach()
target_link_libraries(ai-computer-toolkit-capture-worker PRIVATE
    d3d11
    dxgi
    gdi32
    windowscodecs
)
target_link_libraries(ai-computer-toolkit-recording-worker PRIVATE
    d3d11
    dxgi
    gdi32
    windowscodecs
)
if(NOT MSVC)
    target_compile_options(ai-computer-toolkit-capture-worker PRIVATE
        -Wno-nonportable-include-path
    )
    target_compile_options(ai-computer-toolkit-media-worker PRIVATE
        -Wno-nonportable-include-path
    )
    target_compile_options(
        ai-computer-toolkit-media-control-worker PRIVATE
        -Wno-nonportable-include-path
    )
    target_compile_options(ai-computer-toolkit-recording-worker PRIVATE
        -Wno-nonportable-include-path
    )
endif()

enable_testing()
add_test(NAME cpp-cli-version COMMAND ai-computer-toolkit-cpp version)
add_test(NAME cpp-cli-catalog COMMAND ai-computer-toolkit-cpp catalog app)
add_test(NAME cpp-cli-discover COMMAND ai-computer-toolkit-cpp discover app)

add_executable(act-worker-fixture
    tests/worker_fixture_main.cpp
)
set_target_properties(act-worker-fixture PROPERTIES
    OUTPUT_NAME act-worker-fixture
)
add_executable(act-worker-process-test
    tests/worker_process_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
)
add_executable(act-capture-worker-process-test
    tests/capture_worker_process_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
)
add_executable(act-pixel-buffer-test
    tests/pixel_buffer_test.cpp
    src/components/pixel_buffer.cpp
)
# 编译无平台依赖的 opaque 目标唯一匹配纯测试。
add_executable(act-opaque-target-match-test
    tests/opaque_target_match_test.cpp
)
add_executable(act-png-encoder-test
    tests/png_encoder_test.cpp
    src/platform/windows/png_encoder.cpp
)
add_executable(act-png-file-output-test
    tests/png_file_output_test.cpp
    src/platform/windows/png_encoder.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-screenshot-module-policy-test
    tests/screenshot_module_policy_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/worker_process.cpp
    src/modules/screenshot_module.cpp
    src/modules/screenshot_compatibility_module.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/capture_worker_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-text-document-compatibility-test
    tests/text_document_compatibility_test.cpp
    src/components/json.cpp
    src/components/utf8.cpp
    src/modules/text_document_compatibility_module.cpp
)
add_executable(act-media-worker-process-test
    tests/media_worker_process_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
)
add_executable(act-media-control-candidate-test
    tests/media_control_candidate_test.cpp
    src/components/cancellation.cpp
    src/components/companion_file.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/worker_process.cpp
    src/modules/media_control_compatibility_module.cpp
    src/modules/media_session_module.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/media_worker_backend.cpp
    src/platform/windows/text_codec.cpp
    src/systems/media_command_system.cpp
)
add_executable(act-foreground-input-candidate-test
    tests/foreground_input_candidate_test.cpp
    src/components/json.cpp
    src/components/key_chord.cpp
    src/components/opaque_id.cpp
    src/modules/foreground_input_module.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/foreground_input_backend.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-application-launch-fixture WIN32
    tests/application_launch_fixture_main.cpp
)
add_executable(act-application-launch-candidate-test
    tests/application_launch_candidate_test.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/modules/application_discovery_module.cpp
    src/modules/application_launch_compatibility_module.cpp
    src/modules/application_launch_module.cpp
    src/platform/windows/application_launch_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/installed_application_backend.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/shell_application_backend.cpp
    src/platform/windows/text_codec.cpp
    src/systems/application_launch_command_system.cpp
)
add_executable(act-structured-image-candidate-test
    tests/structured_image_candidate_test.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/output_path_policy.cpp
    src/components/cancellation.cpp
    src/components/worker_process.cpp
    src/modules/structured_image_module.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/structured_image_backend.cpp
    src/platform/windows/structured_image_observation_backend.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-standard-edit-module-test
    tests/standard_edit_module_test.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/static_permission_assessment.cpp
    src/modules/standard_edit_module.cpp
    src/modules/standard_edit_compatibility_module.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/process_backend.cpp
    src/platform/windows/standard_edit_backend.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-window-close-module-test
    tests/window_close_module_test.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/modules/window_close_compatibility_module.cpp
    src/modules/window_close_module.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/text_codec.cpp
    src/platform/windows/window_close_backend.cpp
)
add_executable(act-browser-screenshot-module-test
    tests/browser_screenshot_module_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
    src/modules/browser_screenshot_module.cpp
    src/platform/windows/browser_worker_backend.cpp
    src/platform/windows/png_file_output.cpp
    src/platform/windows/text_codec.cpp
    # 编译 CLI System 以覆盖未知 Chromium 参数拒绝门禁。
    src/systems/browser_command_system.cpp
)
add_executable(act-json-number-test
    tests/json_number_test.cpp
    src/components/json.cpp
)
add_executable(act-recording-config-test
    tests/recording_config_test.cpp
    src/components/json.cpp
    src/components/recording_config.cpp
)
add_executable(act-recording-capture-probe-test
    tests/recording_capture_probe_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
)
add_executable(act-recording-worker-process-test
    tests/recording_worker_process_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/worker_process.cpp
    src/platform/windows/recording_worker_backend.cpp
)
add_executable(act-recording-analysis-test
    tests/recording_analysis_test.cpp
    src/components/recording_analysis.cpp
)
add_executable(act-recording-commit-test
    tests/recording_commit_test.cpp
    src/components/recording_commit.cpp
)
add_executable(act-recording-module-test
    tests/recording_module_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/opaque_id.cpp
    src/components/recording_commit.cpp
    src/components/recording_config.cpp
    src/components/worker_process.cpp
    src/modules/recording_module.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/recording_worker_backend.cpp
    src/platform/windows/text_codec.cpp
)
add_executable(act-recording-compatibility-test
    tests/recording_compatibility_test.cpp
    src/components/json.cpp
    src/modules/recording_compatibility_module.cpp
)
add_executable(act-recording-command-system-policy-test
    tests/recording_command_system_policy_test.cpp
    src/components/cancellation.cpp
    src/components/json.cpp
    src/components/json_input.cpp
    src/components/opaque_id.cpp
    src/components/recording_commit.cpp
    src/components/recording_config.cpp
    src/components/worker_process.cpp
    src/modules/recording_compatibility_module.cpp
    src/modules/recording_module.cpp
    src/platform/windows/capture_preflight_backend.cpp
    src/platform/windows/discovery_backend.cpp
    src/platform/windows/recording_worker_backend.cpp
    src/platform/windows/text_codec.cpp
    src/systems/recording_command_system.cpp
)
target_include_directories(act-png-encoder-test PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)
target_include_directories(act-png-file-output-test PRIVATE
    "${ACT_CPPWINRT_INCLUDE}"
)
if(NOT MSVC)
    target_compile_options(act-png-encoder-test PRIVATE
        -Wno-nonportable-include-path
    )
    target_compile_options(act-png-file-output-test PRIVATE
        -Wno-nonportable-include-path
    )
endif()
foreach(target
    act-worker-fixture
    act-worker-process-test
    act-capture-worker-process-test
    act-pixel-buffer-test
    act-opaque-target-match-test
    act-png-encoder-test
    act-png-file-output-test
    act-screenshot-module-policy-test
    act-text-document-compatibility-test
    act-media-worker-process-test
    act-media-control-candidate-test
    act-foreground-input-candidate-test
    act-application-launch-fixture
    act-application-launch-candidate-test
    act-structured-image-candidate-test
    act-standard-edit-module-test
    act-window-close-module-test
    act-browser-screenshot-module-test
    act-json-number-test
    act-recording-config-test
    act-recording-capture-probe-test
    act-recording-worker-process-test
    act-recording-analysis-test
    act-recording-commit-test
    act-recording-module-test
    act-recording-compatibility-test
    act-recording-command-system-policy-test
)
    target_compile_features(${target} PRIVATE cxx_std_23)
    target_include_directories(${target} PRIVATE include src)
    target_compile_definitions(${target} PRIVATE
        UNICODE
        _UNICODE
        WIN32_LEAN_AND_MEAN
        NOMINMAX
        _WIN32_WINNT=0x0A00
    )
endforeach()
target_link_libraries(act-structured-image-candidate-test PRIVATE
    ole32
    oleaut32
    uuid
    user32
    advapi32
    dwmapi
)
target_link_libraries(act-png-encoder-test PRIVATE
    ole32
    windowscodecs
)
target_link_libraries(act-png-file-output-test PRIVATE
    ole32
    windowscodecs
)
target_link_libraries(act-screenshot-module-policy-test PRIVATE
    ole32
    oleaut32
    uuid
    user32
    advapi32
    dwmapi
    runtimeobject
)
target_link_libraries(act-recording-capture-probe-test PRIVATE
    advapi32
)
target_link_libraries(act-recording-worker-process-test PRIVATE
    advapi32
)
target_link_libraries(act-recording-module-test PRIVATE
    ole32
    oleaut32
    uuid
    user32
    advapi32
    dwmapi
    runtimeobject
)
target_link_libraries(act-recording-command-system-policy-test PRIVATE
    ole32
    oleaut32
    uuid
    user32
    advapi32
    dwmapi
    runtimeobject
)
foreach(target
    act-standard-edit-module-test
    act-window-close-module-test
    act-browser-screenshot-module-test
)
    target_link_libraries(${target} PRIVATE
        ole32
        oleaut32
        uuid
        user32
        advapi32
        dwmapi
    )
endforeach()
add_test(NAME cpp-worker-process-lifecycle
    COMMAND act-worker-process-test
)
add_test(NAME cpp-capture-worker-process-lifecycle
    COMMAND act-capture-worker-process-test
)
add_test(NAME cpp-pixel-buffer
    COMMAND act-pixel-buffer-test
)
# 注册 Missing、Unique 与 Ambiguous 三态门禁。
add_test(NAME cpp-opaque-target-match
    COMMAND act-opaque-target-match-test
)
add_test(NAME cpp-png-encoder
    COMMAND act-png-encoder-test
)
add_test(NAME cpp-png-file-output
    COMMAND act-png-file-output-test
        "${CMAKE_CURRENT_BINARY_DIR}/atomic-png-ctest-fixture"
)
add_test(NAME cpp-screenshot-module-policy
    COMMAND act-screenshot-module-policy-test
)
add_test(NAME cpp-text-document-compatibility
    COMMAND act-text-document-compatibility-test
)
add_test(NAME cpp-media-worker-process-lifecycle
    COMMAND act-media-worker-process-test
)
add_test(NAME cpp-media-control-candidate
    COMMAND act-media-control-candidate-test
)
add_test(NAME cpp-foreground-input-candidate
    COMMAND act-foreground-input-candidate-test
)
add_test(NAME cpp-application-launch-candidate
    COMMAND act-application-launch-candidate-test
        "$<TARGET_FILE:act-application-launch-fixture>"
        "${CMAKE_CURRENT_BINARY_DIR}/act-application-launch-fixture.ready"
)
add_test(NAME cpp-structured-image-candidate
    COMMAND act-structured-image-candidate-test
        "${CMAKE_CURRENT_BINARY_DIR}/structured-image-candidate-fixture"
)
add_test(NAME cpp-standard-edit-module
    COMMAND act-standard-edit-module-test
)
add_test(NAME cpp-window-close-module
    COMMAND act-window-close-module-test
)
add_test(NAME cpp-browser-screenshot-module
    COMMAND act-browser-screenshot-module-test
)
add_test(NAME cpp-json-number
    COMMAND act-json-number-test
)
add_test(NAME cpp-recording-config
    COMMAND act-recording-config-test
)
add_test(NAME cpp-recording-capture-probe
    COMMAND act-recording-capture-probe-test
)
add_test(NAME cpp-recording-worker-process
    COMMAND act-recording-worker-process-test
)
add_test(NAME cpp-recording-analysis
    COMMAND act-recording-analysis-test
)
add_test(NAME cpp-recording-commit
    COMMAND act-recording-commit-test
)
add_test(NAME cpp-recording-compatibility
    COMMAND act-recording-compatibility-test
)
add_test(NAME cpp-recording-command-system-policy
    COMMAND act-recording-command-system-policy-test
)
