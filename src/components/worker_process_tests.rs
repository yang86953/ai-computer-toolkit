//! 验证 Job-bounded worker process Component 的协议与整树生命周期。

// 导入待测私有实现。
use super::*;
// 导入文件与进程标识工具。
use std::{
    // 导入精确临时标记文件操作。
    fs,
    // 导入当前测试进程 ID。
    process,
    // 导入取消夹具的原子计数器。
    sync::atomic::{AtomicUsize, Ordering},
};
// 导入 JSON 夹具构造宏。
use serde_json::json;
// 导入同用户子进程存活查询接口。
use windows::Win32::{
    // 导入活动进程状态常量。
    Foundation::STILL_ACTIVE,
    // 导入最小进程查询权限与打开接口。
    System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
};

// 返回不会创建可见窗口的系统 PowerShell fixture。
fn powershell_fixture() -> &'static Path {
    // 使用 Windows 固定系统路径，不接受外部输入。
    Path::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
}

// 为当前测试进程建立唯一 descendant PID 标记路径。
fn descendant_marker(label: &str) -> PathBuf {
    // 只组合固定测试标签与当前测试进程 ID。
    std::env::temp_dir().join(format!("act-worker-tree-{}-{label}.pid", process::id()))
}

// 把 Windows 路径编码成 PowerShell 单引号字面量内容。
fn powershell_literal(path: &Path) -> String {
    // 双写单引号，禁止测试临时路径改变脚本结构。
    path.to_string_lossy().replace('\'', "''")
}

// 构造会生成继承同一 Job 的隐藏 descendant 的固定 worker 参数。
fn descendant_arguments(marker: &Path, parent_tail: &str) -> Vec<String> {
    // 生成只含固定程序、固定参数和测试标记路径的脚本。
    let script = format!(
        "$child = Start-Process -FilePath '{}' -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 10') -NoNewWindow -PassThru; [System.IO.File]::WriteAllText('{}', $child.Id.ToString()); {parent_tail}",
        // 注入固定系统 PowerShell 路径。
        powershell_literal(powershell_fixture()),
        // 注入已转义测试标记路径。
        powershell_literal(marker),
    );
    // 返回封闭的 PowerShell 参数集合。
    vec![
        // 禁用用户 profile。
        "-NoProfile".to_owned(),
        // 禁止交互提示。
        "-NonInteractive".to_owned(),
        // 指定固定脚本模式。
        "-Command".to_owned(),
        // 传入完整固定脚本。
        script,
    ]
}

// 从测试标记读取 descendant PID。
fn read_descendant_pid(marker: &Path) -> u32 {
    // 读取自有临时标记。
    let text = fs::read_to_string(marker).unwrap_or_else(|error| {
        // 缺少标记时提供可诊断测试失败。
        panic!("descendant marker could not be read: {error}")
    });
    // 把十进制 PID 转换为仅供测试查询的原生值。
    text.parse::<u32>().unwrap_or_else(|error| {
        // 非法标记必须失败。
        panic!("descendant marker was invalid: {error}")
    })
}

// 只读判断同用户测试进程是否仍处于活动状态。
fn process_is_active(process_id: u32) -> bool {
    // 用最小查询权限打开进程。
    let Ok(handle) = (unsafe {
        // 不允许测试句柄被后续进程继承。
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
    }) else {
        // 无法打开通常表示进程已被回收。
        return false;
    };
    // 把查询句柄纳入 RAII 所有权。
    let Ok(handle) = OwnedHandle::new(handle, "OpenProcess(test descendant)") else {
        // 无效句柄按非活动处理。
        return false;
    };
    // 初始化退出码缓冲。
    let mut exit_code = 0_u32;
    // 查询当前退出状态。
    let queried = unsafe { GetExitCodeProcess(handle.raw(), &mut exit_code) }.is_ok();
    // 只有查询成功且仍为活动码才算残留。
    queried && exit_code == STILL_ACTIVE.0 as u32
}

// 删除测试 PID 标记并忽略不存在状态。
fn remove_marker(marker: &Path) {
    // 标记只属于当前测试进程，可安全按精确路径删除。
    let _ = fs::remove_file(marker);
}

// 验证 Component 私有封闭类型逐字保持九个公开错误码。
#[test]
fn worker_process_error_codes_preserve_public_contract() {
    // 按 Component 领域语义顺序收集全部封闭变体。
    let actual = [
        // 调用方取消。
        WorkerProcessErrorCode::Cancelled.as_str(),
        // 固定 companion 不可用。
        WorkerProcessErrorCode::CompanionUnavailable.as_str(),
        // 参数拒绝。
        WorkerProcessErrorCode::InvalidArgument.as_str(),
        // 请求序列化失败。
        WorkerProcessErrorCode::SerializationFailed.as_str(),
        // 单调 deadline 超时。
        WorkerProcessErrorCode::Timeout.as_str(),
        // 输出资源边界失败。
        WorkerProcessErrorCode::OutputTooLarge.as_str(),
        // stdio 或 JSON 协议失败。
        WorkerProcessErrorCode::ProtocolFailed.as_str(),
        // worker 启动失败。
        WorkerProcessErrorCode::StartFailed.as_str(),
        // 等待或整树回收失败。
        WorkerProcessErrorCode::WaitFailed.as_str(),
    ];
    // 公开文本必须与既有 error envelope 逐字一致。
    assert_eq!(
        actual,
        // 固定预期顺序使缺失、重复或误改都可诊断。
        [
            // 保持通用取消码。
            "CANCELLED",
            // 保持固定 companion 不可用码。
            "COMPANION_WORKER_UNAVAILABLE",
            // 保持通用参数码。
            "INVALID_ARGUMENT",
            // 保持序列化失败码。
            "SERIALIZATION_FAILED",
            // 保持通用超时码。
            "TIMEOUT",
            // 保持 worker 输出上限码。
            "WORKER_OUTPUT_TOO_LARGE",
            // 保持 worker 协议失败码。
            "WORKER_PROTOCOL_FAILED",
            // 保持 worker 启动失败码。
            "WORKER_START_FAILED",
            // 保持 worker 等待失败码。
            "WORKER_WAIT_FAILED",
        ]
    );
}

// 验证路径编码以 NUL 结束且保留内容。
#[test]
fn wide_worker_path_is_nul_terminated() {
    // 编码合成 Windows 路径。
    let encoded = wide_path(Path::new(r"C:\tool\worker.exe"));
    // 断言最后一个 code unit 是 NUL。
    assert_eq!(encoded.last(), Some(&0));
    // 断言只附加一个终止符。
    assert_eq!(encoded.iter().filter(|unit| **unit == 0).count(), 1);
}

// 验证 Windows 引用规则覆盖空格、引号与尾随反斜杠。
#[test]
fn command_line_arguments_are_quoted() {
    // 引用含空格的参数。
    assert_eq!(quote_argument("a b"), "\"a b\"");
    // 引用空参数。
    assert_eq!(quote_argument(""), "\"\"");
    // 简单参数保持原样。
    assert_eq!(quote_argument("control"), "control");
    // 引号必须被转义。
    assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
}

// 验证 deadline 会终止并回收真实 Job 子孙进程树。
#[test]
fn timeout_terminates_and_reaps_descendant_tree() {
    // 建立独立 timeout 标记。
    let marker = descendant_marker("timeout");
    // 清理失败测试可能留下的旧标记。
    remove_marker(&marker);
    // 让 worker 与 descendant 都阻塞到 deadline 之后。
    let arguments = descendant_arguments(&marker, "Start-Sleep -Seconds 10");
    // 运行足以创建 descendant 的短 deadline worker。
    let result = run_companion(
        // 传入固定 PowerShell fixture。
        powershell_fixture(),
        // 传入封闭参数。
        &arguments,
        // 传入任意小 JSON 请求。
        &json!({ "fixture": true }),
        // 留出 worker 创建 descendant 的时间。
        Duration::from_millis(1_500),
        // 允许小输出。
        1024,
        // 不触发 cancellation。
        || false,
    );
    // deadline 必须返回稳定超时码。
    assert_eq!(result.err().map(|error| error.code), Some("TIMEOUT"));
    // 读取 worker 已创建的真实 descendant PID。
    let descendant = read_descendant_pid(&marker);
    // 返回前整个 Job 必须已经终止。
    assert!(!process_is_active(descendant));
    // 删除精确测试标记。
    remove_marker(&marker);
}

// 验证主 worker 退出时先终止残余 Job 成员再读取协议。
#[test]
fn worker_exit_reaps_descendant_tree_before_protocol_join() {
    // 建立独立 exit 标记。
    let marker = descendant_marker("exit");
    // 清理失败测试可能留下的旧标记。
    remove_marker(&marker);
    // 让主 worker 输出合法 envelope 后退出，descendant 继续阻塞。
    let arguments = descendant_arguments(&marker, "Write-Output '{\"ok\":true}'; exit 7");
    // 记录调用耗时以拒绝等待 descendant 自行结束。
    let started = Instant::now();
    // 执行 worker-exit 路径。
    let output = run_companion(
        // 传入固定 PowerShell fixture。
        powershell_fixture(),
        // 传入封闭参数。
        &arguments,
        // 传入任意小 JSON 请求。
        &json!({ "fixture": true }),
        // deadline 足以完成主 worker，但短于 descendant 睡眠。
        Duration::from_secs(5),
        // 允许小输出。
        1024,
        // 不触发 cancellation。
        || false,
    )
    // worker 合法退出必须返回协议结果。
    .unwrap_or_else(|error| panic!("worker exit fixture failed: {}", error.code));
    // 保留 worker 自身的非零退出码。
    assert_eq!(output.exit_code, 7);
    // 合法单行 JSON 必须完成解析。
    assert_eq!(output.envelope.get("ok"), Some(&Value::Bool(true)));
    // 调用不得等待十秒 descendant 自行结束。
    assert!(started.elapsed() < Duration::from_secs(5));
    // 读取 worker 已创建的真实 descendant PID。
    let descendant = read_descendant_pid(&marker);
    // 返回前残余 descendant 必须已经终止。
    assert!(!process_is_active(descendant));
    // 删除精确测试标记。
    remove_marker(&marker);
}

// 验证 cancellation 会终止并回收真实 Job 主进程。
#[test]
fn cancellation_terminates_and_reaps_job() {
    // 构造确定性阻塞参数。
    let arguments = vec![
        // 禁用 profile。
        "-NoProfile".to_owned(),
        // 指定命令模式。
        "-Command".to_owned(),
        // 阻塞十秒，等待取消。
        "Start-Sleep -Seconds 10".to_owned(),
    ];
    // 创建取消轮询计数器。
    let polls = AtomicUsize::new(0);
    // 运行带取消的 worker。
    let result = run_companion(
        // 传入 fixture 路径。
        powershell_fixture(),
        // 传入 fixture 参数。
        &arguments,
        // 传入任意小 JSON 请求。
        &json!({ "fixture": true }),
        // deadline 留足取消路径时间。
        Duration::from_secs(5),
        // 允许小输出。
        1024,
        // 第二次轮询起触发取消。
        || polls.fetch_add(1, Ordering::SeqCst) >= 1,
    );
    // 必须返回 cancelled 且已在返回前回收。
    assert_eq!(result.err().map(|error| error.code), Some("CANCELLED"));
}
