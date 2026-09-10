#![cfg(target_os = "windows")]

//! 验证独立交互会话的生产 launcher 发现与零副作用 stale 路由。

// 导入精确临时文件、仓库路径与子进程接口。
use std::{
    // 写入并删除本测试拥有的 structured request。
    fs,
    // 构造生产 launcher 与临时文件路径。
    path::PathBuf,
    // 收集 launcher 的唯一标准输出。
    process::{Command, Output},
    // 生成不会与并行测试冲突的文件名。
    time::{SystemTime, UNIX_EPOCH},
};

// 导入通用 JSON 值与构造宏。
use serde_json::{Value, json};

// 保存本测试拥有的精确临时请求文件。
struct RequestFile {
    // 只保存已经成功写入的绝对路径。
    path: PathBuf,
}

// 提供 structured request 文件构造。
impl RequestFile {
    // 将固定 JSON 对象写入工具自有临时文件。
    fn create(request: &Value) -> Self {
        // 取得当前唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟异常必须立即失败。
            .unwrap_or_else(|error| panic!("request clock failed: {error}"))
            // 使用纳秒降低并行碰撞概率。
            .as_nanos();
        // 在系统临时目录中构造精确文件名。
        let path = std::env::temp_dir().join(format!(
            // 固定工具自有测试前缀。
            "act-interactive-isolation-{}-{stamp}.json",
            // 加入当前测试进程 ID。
            std::process::id(),
        ));
        // 将请求序列化为 UTF-8 JSON。
        let bytes = serde_json::to_vec(request)
            // 内部固定对象必须可序列化。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入唯一临时文件。
        fs::write(&path, bytes)
            // 文件系统失败必须显式报告。
            .unwrap_or_else(|error| panic!("request write failed: {error}"));
        // 返回精确文件所有者。
        Self { path }
    }
}

// 在作用域结束时删除且只删除本测试文件。
impl Drop for RequestFile {
    // 执行可恢复的局部清理。
    fn drop(&mut self) {
        // 清理失败不覆盖主要协议断言。
        let _ = fs::remove_file(&self.path);
    }
}

// 返回仓库内唯一生产 launcher 路径。
fn launcher_path() -> PathBuf {
    // 从 Cargo 根目录开始。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        // 进入工具目录。
        .join("tools")
        // 选择唯一受支持的生产入口。
        .join("Invoke-ComputerControl.ps1")
}

// 经正式 PowerShell launcher 执行固定参数。
fn launcher(arguments: &[&str]) -> Output {
    // 启动系统 PowerShell。
    Command::new("powershell")
        // 禁止加载用户 profile 并只为仓库脚本绕过会话策略。
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        // 传入唯一生产 launcher。
        .arg(launcher_path())
        // 传入封闭测试参数。
        .args(arguments)
        // 收集唯一标准输出和退出码。
        .output()
        // 无法启动生产入口时给出明确诊断。
        .unwrap_or_else(|error| panic!("production launcher failed to start: {error}"))
}

// 严格解析 launcher 的唯一 JSON stdout。
fn output_json(output: &Output) -> Value {
    // 只接受完整 UTF-8 JSON envelope。
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        // 解析失败时展示受控标准输出。
        panic!(
            // 使用固定诊断模板。
            "launcher JSON failed: {error}; stdout={}",
            // 对异常字节执行仅测试可见的宽松展示。
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

// 从当前发现结果选择一条确定不在集合内的 canonical s2:i。
fn absent_interactive_session_id(discovery: &Value) -> String {
    // 读取公开 endpoint 数组。
    let sessions = discovery["sessions"]
        // 发现契约必须返回数组。
        .as_array()
        // 形状漂移立即失败。
        .unwrap_or_else(|| panic!("discovery sessions must be an array"));
    // 在足够大的固定候选空间内寻找缺失身份。
    for value in 0_u64..=1_024 {
        // 构造 canonical 十六进制 opaque ID。
        let candidate = format!("s2:i:{value:016x}");
        // 检查当前认证集合是否含该公开身份。
        let present = sessions.iter().any(|session| {
            // 只比较公开 sessionId。
            session["sessionId"].as_str() == Some(candidate.as_str())
        });
        // 返回当前集合中第一条确定缺失的 ID。
        if !present {
            // 交付拥有型公开目标。
            return candidate;
        }
    }
    // 单机不可能合法发布超过候选空间的 endpoint，异常时停止测试。
    panic!("no absent canonical interactive session ID was available")
}

// 验证生产入口只读发现，并让 stale 精确目标在 provider 前失败闭合。
#[test]
fn production_launcher_discovers_and_rejects_absent_endpoint_without_fallback() {
    // 经正式 launcher 执行只读隔离发现。
    let discovery_output = launcher(&["discover", "isolation"]);
    // 发现命令即使集合为空也必须成功。
    assert!(discovery_output.status.success());
    // 解析公开发现 envelope。
    let discovery = output_json(&discovery_output);
    // 顶层必须报告成功。
    assert_eq!(discovery["ok"], true);
    // 固定发布版本化发现 capability。
    assert_eq!(discovery["capability"], "interactive.session.discover@1");
    // 发现必须明确只读。
    assert_eq!(discovery["readOnly"], true);
    // host 发现不得进入任一交互桌面。
    assert_eq!(discovery["executionRealm"], "host-headless");
    // 当前桌面 fallback 必须始终关闭。
    assert_eq!(discovery["foregroundFallbackUsed"], false);
    // 读取公开 session 数组。
    let sessions = discovery["sessions"]
        // 契约必须保持数组形状。
        .as_array()
        // 形状漂移立即失败。
        .unwrap_or_else(|| panic!("discovery sessions must be an array"));
    // count 必须与实际投影数量一致。
    assert_eq!(discovery["count"].as_u64(), Some(sessions.len() as u64));
    // 逐条核对认证 endpoint 的 provider-neutral 投影。
    for session in sessions {
        // 公开 ID 必须是 canonical s2:i。
        let session_id = session["sessionId"]
            // 只接受字符串目标。
            .as_str()
            // 缺失身份立即失败。
            .unwrap_or_else(|| panic!("interactive session ID must be a string"));
        // ID 必须使用冻结前缀与 16 位指纹。
        assert!(
            session_id.len() == 21
                // 要求固定 opaque 类别。
                && session_id.starts_with("s2:i:")
                // 指纹必须为小写十六进制。
                && session_id[5..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        // 目标类别不得暴露 Windows session。
        assert_eq!(session["targetKind"], "independent-interactive-session");
        // endpoint 只在认证可用时发布。
        assert_eq!(session["state"], "available");
        // command 固定在隔离 worker 执行。
        assert_eq!(session["executionRealm"], "isolated-worker");
        // 身份必须绑定授权与登录会话生命周期。
        assert_eq!(
            session["identityFreshness"],
            "authorization-and-session-lifetime"
        );
    }

    // 选择当前认证集合中确定不存在的公开 endpoint。
    let absent_session_id = absent_interactive_session_id(&discovery);
    // 构造不会越过 endpoint 选择的通用键盘请求。
    let request = json!({
        // 同时携带原窗口身份与独立会话授权代际。
        "target": {
            // 固定 canonical 窗口 ID，但不会进入 worker 解析。
            "sessionId": "s2:w:0000000000000000",
            // 使用刚确认不在发现集合中的公开 endpoint。
            "interactiveSessionId": absent_session_id
        },
        // 选择冻结的通用键盘 capability。
        "args": {
            // 不包含任何软件专用语义。
            "capability": "ui.input.key@1",
            // 提供合法领域输入和短总预算。
            "input": { "key": "ENTER", "timeoutMs": 1000 }
        },
        // 满足逐操作确认门禁。
        "confirmed": true,
        // 许可只可能作用于目标独立会话前景。
        "foregroundConsent": true,
        // 冻结严格零当前桌面干扰要求。
        "isolationRequirement": "strict"
    });
    // 将 structured wrapper 写入精确临时文件。
    let input = RequestFile::create(&request);
    // 取得 UTF-8 路径用于 PowerShell 参数。
    let input_path = input
        // 借用精确路径。
        .path
        // 当前 Windows 临时路径必须可表示为 UTF-8。
        .to_str()
        // 异常编码立即失败。
        .unwrap_or_else(|| panic!("request path must be valid UTF-8"));
    // 经生产 launcher 执行严格 generic route。
    let mutation_output = launcher(&[
        // 选择通用运行入口。
        "run",
        // 使用 provider-neutral app surface。
        "app",
        // capability 固定对应 generic apply operation。
        "apply",
        // 读取 structured wrapper。
        "--input",
        // 传入测试拥有的精确文件。
        input_path,
        // CLI 侧再次冻结严格隔离要求。
        "--strict-isolation",
        // CLI 侧再次满足确认门禁。
        "--confirm",
        // CLI 侧再次允许目标会话前景。
        "--allow-foreground",
    ]);
    // 缺失 endpoint 必须以非零状态失败闭合。
    assert!(!mutation_output.status.success());
    // 解析统一错误 envelope。
    let mutation = output_json(&mutation_output);
    // 顶层必须报告失败。
    assert_eq!(mutation["ok"], false);
    // endpoint 集合为空时不可用，非空时缺失授权代际为 stale。
    assert!(matches!(
        mutation["error"]["code"].as_str(),
        Some("ISOLATED_WORKER_UNAVAILABLE" | "STALE_SESSION")
    ));
    // host 本地 provider 必须保持未调用。
    assert_eq!(mutation["error"]["details"]["localProviderInvoked"], false);
    // 当前桌面 fallback 必须保持关闭。
    assert_eq!(
        mutation["error"]["details"]["foregroundFallbackUsed"],
        false
    );
    // 选择失败必须可以安全重试。
    assert_eq!(mutation["error"]["details"]["retrySafe"], true);
    // 请求不得改变目标。
    assert_eq!(mutation["error"]["details"]["targetMayHaveMutated"], false);
}
