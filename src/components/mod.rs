// 导出同目录 staging 与原子文件提交 Component。
pub(crate) mod atomic_file;
// 导出固定 Chromium runtime 私有发现 Component。
pub(crate) mod browser_runtime;
// 导出浏览器会话生命周期公开输入的严格无状态解析 Component。
pub(crate) mod browser_session_lifecycle_input;
// 导出浏览器会话独立公开身份分类 Component。
pub(crate) mod browser_session_identity;
// 公开 Browser Session lifecycle 只使用封闭错误投影 Component。
pub(crate) mod browser_session_lifecycle_error;
// 导出显式浏览器会话 worker 的严格双阶段打开协议 Component。
// #2026 冻结协议状态机，后续子任务再接入生产 worker 与公开能力。
#[allow(dead_code)]
pub(crate) mod browser_session_protocol;
// 导出同会话浏览器 broker 的封闭 parser 与状态机 Component。
// #2044 仅冻结纯 Rust 协议，后续任务再接入 IPC 与 broker binary。
#[allow(dead_code)]
pub(crate) mod browser_session_broker_protocol;
// 导出 broker 物理宿主与唯一 System dispatcher 共用的 epoch 运行时协调 Component。
pub(crate) mod browser_session_broker_runtime;
// 导出 Module 自有的后台浏览器会话 close 回收 Component。
pub(crate) mod browser_session_close_task;
// 导出显式浏览器会话的 Job-bounded 私有协议客户端。
pub(crate) mod browser_session_process;
// 导出浏览器页面命令的强类型双阶段协议 Component。
// #2031 冻结协议状态机，#2032–#2033 接入 worker 与 Module 前保留完整实现。
#[allow(dead_code)]
pub(crate) mod browser_page_protocol;
// 导出浏览器页面公开输入的严格 provider-neutral 解析 Component。
pub(crate) mod browser_page_input;
// 导出固定 loopback WebSocket/CDP 私有传输 Component。
// #2032 页面循环接入前保留独立验证的传输实现。
#[allow(dead_code)]
pub(crate) mod browser_cdp_transport;
// 提供固定 CDP target 的建立与持有生命周期。
pub(crate) mod browser_cdp_session;
// 提供固定页面命令的 worker 侧帧循环。
pub(crate) mod browser_page_worker;
// 导出 Accessibility 只读查询共用的语义 selector Component。
pub(crate) mod accessibility_selector;
// 导出所有 CLI 共用的有界 JSON 输入 Component。
pub(crate) mod bounded_json_input;
// 导出浏览器临时 profile 生命周期 Component。
pub(crate) mod browser_profile;
// 导出字节摘要 Component 供像素与制品事实复用。
pub(crate) mod byte_digest;
// 导出进程级取消信号 Component。
pub(crate) mod cancellation;
// 导出仅返回 canonical 文本的当前 Windows 用户 SID Component。
pub(crate) mod current_user_sid_windows;
// 导出项目自有 Media Foundation H.264/MP4 编码 Component。
pub(crate) mod media_foundation_encoder;
// 导出无 provider 依赖的 JSON Pointer 条件判定 Component。
pub(crate) mod json_postcondition;
// 导出长操作 broker 的封闭版本化协议 Component。
// #2014 接入生产 broker 前保留已冻结但尚无生产调用方的协议实现。
#[allow(dead_code)]
pub(crate) mod long_operation_protocol;
// 导出主 launcher 使用的严格 broker response parser。
pub(crate) mod long_operation_broker_response;
// 导出长操作业务接受前错误的封闭投影 Component。
pub(crate) mod long_operation_error_projection;
// 导出长操作随机 opaque handle 身份 Component。
pub(crate) mod long_operation_identity;
// 导出长操作 registry 专用的原子单记录 journal Component。
// #2017 接入固定 broker 前保留当前仅被测试与 registry 使用的实现。
#[allow(dead_code)]
pub(crate) mod long_operation_journal;
// 导出长操作 broker 使用的固定 LocalAppData journal 根 Component。
// #2020 由固定 broker 首次接入生产生命周期。
pub(crate) mod long_operation_storage_windows;
// 导出独立交互会话 command worker 的封闭 JSON 协议 Component。
pub(crate) mod interactive_command_protocol;
// 导出 command worker 的完成、拒绝与 OutcomeUnknown 投影。
pub(crate) mod interactive_command_result;
// 导出独立交互会话 broker 的封闭本机协议 Component。
pub(crate) mod interactive_session_broker_protocol;
// 导出 broker 结果包装与 dispatch 前拒绝 Component。
pub(crate) mod interactive_session_broker_response;
// 导出 Windows 系统随机的一次性 endpoint nonce Component。
pub(crate) mod secure_nonce_windows;
// 导出 provider-neutral 键盘输入与请求内按键所有权契约 Component。
pub(crate) mod keyboard_input_contract;
// 向 crate 内部公开版本化 opaque target ID 原语。
pub(crate) mod opaque_id;
// 导出统一文件与目录覆盖许可 Component。
pub(crate) mod output_guard;
// 导出创建时即安装受保护 DACL 的 owner-only Windows 目录 Component。
pub(crate) mod owner_only_directory_windows;
// 导出 provider-neutral 指针输入与请求内按钮所有权契约 Component。
pub(crate) mod pointer_input_contract;
// 导出 provider-neutral 进程终止风险与有界输入契约 Component。
pub(crate) mod process_termination_contract;
// 导出录制多产物 staging、安装与回滚 Component。
pub(crate) mod recording_artifacts;
// 导出无 provider 依赖的 sequence 结果预算 Component。
pub(crate) mod sequence_result_budget;
// 导出 sequence Workflow 共用的单调执行预算与停止观察 Component。
// #2025 已将两级预算接入生产 sequence Workflow。
pub(crate) mod sequence_execution_budget;
// 导出 sequence execution、step、nonce 与 digest 的 canonical 文本 Component。
pub(crate) mod sequence_execution_identity;
// 导出 sequence execution forget 的紧凑永久去重 tombstone Component。
// #2391 生产 Module 组合前保留独立验证的单调索引实现。
#[allow(dead_code)]
pub(crate) mod sequence_execution_forgotten;
// 导出 sequence execution 固定目录内的原子多记录 journal Component。
// #2391 生产 Module 组合前保留独立验证的文件生命周期实现。
#[allow(dead_code)]
pub(crate) mod sequence_execution_journal;
// 导出 sequence execution broker 使用的固定 owner-only LocalAppData 存储根。
// #2391 生产 Module 组合前保留独立验证的 Known Folder 适配。
#[allow(dead_code)]
pub(crate) mod sequence_execution_storage_windows;
// 导出 sequence step worker 的严格双阶段 JSON Lines 协议 Component。
// #2023–#2025 已将该协议接入固定 worker、Job runner 与生产 Workflow。
pub(crate) mod sequence_step_protocol;
// 导出语义元素动作共用的 provider-neutral 输入契约 Component。
pub(crate) mod semantic_action_contract;
// 导出无主动写探针的静态 mutation 权限评估 Component。
pub(crate) mod static_permission_assessment;
// 导出无 provider 依赖的稳定等待状态 Component。
pub(crate) mod stable_wait;
// 导出 UIA runtime identity 窄 Component。
pub(crate) mod uia_runtime_id;
// 导出无平台类型的 Unix 毫秒到 UTC RFC 3339 转换 Component。
pub(crate) mod utc_timestamp;
// 导出 provider-neutral 窗口状态与几何生命周期输入契约 Component。
pub(crate) mod window_lifecycle_contract;
// 导出 opaque 窗口目标的身份材料与公开保证强度 Component。
pub(crate) mod window_target_identity;
// 导出持久窗口代际 broker 的严格私有协议 Component。
// #2336 只冻结协议，后续子任务再接 Windows broker 与生产调用方。
#[allow(dead_code)]
pub(crate) mod window_generation_protocol;
// 导出 Job-bounded JSON companion worker Component。
pub(crate) mod worker_process;

// 桌面会话的共用输入、坐标、帧和取消合同。
pub(crate) mod desktop_frame_changes;
pub(crate) mod desktop_frame_stream;
pub(crate) mod desktop_interaction;
pub(crate) mod desktop_session_frame_capture;
pub(crate) mod desktop_session_identity;
pub(crate) mod desktop_session_input_cancellation;
pub(crate) mod desktop_session_keyboard_input;
pub(crate) mod desktop_session_pointer_input;

pub(crate) mod input_deadline;
