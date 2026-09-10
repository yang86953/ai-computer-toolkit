#![cfg(target_os = "windows")]

//! 验证 Rust 窗口录制的确认、封闭输入与 worker 协议边界。

// 导入文件系统、路径、进程与唯一时间工具。
use std::{
    // 创建和检查测试产物。
    fs,
    // 保存精确测试路径。
    path::{Path, PathBuf},
    // 直接启动 Cargo 构建的 Rust 二进制。
    process::{Child, Command, Output, Stdio},
    // 等待自有窗口进入发现快照。
    thread,
    // 生成低碰撞测试目录名。
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 固定 Rust 主程序测试二进制。
const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
// 固定 Rust recording worker 测试二进制。
const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-recording-worker");
// 固定可在 Job 中挂起的仓库自有 worker 测试替身。
const HANG_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-fixture");
// 固定 no-activate 自有窗口 fixture。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-fixture");

// 拥有一个精确测试目录。
struct FixtureDirectory {
    // 保存测试根路径。
    path: PathBuf,
}

// 拥有一个 no-activate 自有窗口进程。
struct FixtureWindow {
    // 保存唯一窗口标题。
    title: String,
    // 保存自有 fixture 子进程。
    child: Child,
}

// 提供自有窗口生命周期。
impl FixtureWindow {
    // 启动固定 Rust 窗口 fixture。
    fn start() -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞。
            .as_nanos();
        // 构造 fixture 允许的唯一 ASCII 标题。
        let title = format!("act-rust-capture-fixture-{}-{stamp}", std::process::id());
        // 启动固定 no-activate fixture。
        let child = Command::new(WINDOW_FIXTURE)
            // 传递受 fixture 前缀与长度校验的标题。
            .arg(&title)
            // 禁止继承输入。
            .stdin(Stdio::null())
            // fixture 不使用 stdout。
            .stdout(Stdio::null())
            // fixture 不使用 stderr。
            .stderr(Stdio::null())
            // 启动自有进程。
            .spawn()
            // 启动失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("window fixture launch failed: {error}"));
        // 返回唯一进程所有者。
        Self { title, child }
    }

    // 有界等待 canonical session ID。
    fn session_id(&mut self) -> String {
        // 最多等待五秒进入窗口目录。
        for _ in 0..50 {
            // 枚举完整窗口 surface。
            let output = Command::new(TOOLKIT)
                // 使用只读窗口发现。
                .args(["sessions", "window", "--max-items", "4096"])
                // 执行并收集 JSON。
                .output()
                // 启动失败时提供测试诊断。
                .unwrap_or_else(|error| panic!("window discovery failed: {error}"));
            // 发现命令必须成功。
            assert!(output.status.success());
            // 解析窗口清单。
            let value = output_json(&output);
            // 按唯一标题查找自有窗口。
            if let Some(session_id) = value["sessions"]
                // 读取 sessions 数组。
                .as_array()
                // 遍历所有安全窗口投影。
                .and_then(|sessions| {
                    // 查找标题精确匹配项。
                    sessions
                        .iter()
                        .find(|session| session["title"] == self.title)
                })
                // 读取 canonical sessionId。
                .and_then(|session| session["sessionId"].as_str())
            {
                // 返回当前快照中的 opaque ID。
                return session_id.to_owned();
            }
            // fixture 提前退出表示创建失败。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落真实用户窗口。
                panic!("window fixture exited before discovery");
            }
            // 短暂等待下一次快照。
            thread::sleep(Duration::from_millis(100));
        }
        // 超出边界时失败。
        panic!("window fixture was not discovered")
    }
}

// 作用域结束时只回收本测试创建的窗口进程。
impl Drop for FixtureWindow {
    // 终止并等待自有 fixture。
    fn drop(&mut self) {
        // 只终止本实例持有的精确子进程。
        let _ = self.child.kill();
        // 回收进程句柄。
        let _ = self.child.wait();
    }
}

// 提供测试目录生命周期。
impl FixtureDirectory {
    // 创建本进程独占目录。
    fn create() -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试环境时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造工具自有精确测试目录。
        let path = std::env::temp_dir().join(format!(
            // 固定测试目录前缀。
            "act-window-record-test-{}-{stamp}",
            // 注入当前进程 ID。
            std::process::id(),
        ));
        // 创建精确目录。
        fs::create_dir(&path)
            // 创建失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("fixture directory failed: {error}"));
        // 返回唯一所有者。
        Self { path }
    }

    // 组合测试目录下路径。
    fn join(&self, name: &str) -> PathBuf {
        // 只返回当前 fixture 子路径。
        self.path.join(name)
    }
}

// 作用域结束时回收测试目录。
impl Drop for FixtureDirectory {
    // 删除当前实例创建的精确目录。
    fn drop(&mut self) {
        // 测试清理不覆盖主要断言。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 运行固定 Rust desktop record 请求。
fn run_desktop_record(
    // 接收 canonical session ID。
    session_id: &str,
    // 接收最终 MP4 路径。
    video: &Path,
    // 接收最终分析目录。
    analysis: &Path,
    // 接收确认状态。
    confirmed: bool,
    // 接收可选协议外字段。
    unknown_field: bool,
) -> Output {
    // 构造不经过 shell 的主程序命令。
    let mut command = Command::new(TOOLKIT);
    // 传递固定公开 surface、operation、target 与输出参数。
    command.args([
        // 使用 run 动词。
        "run",
        // 使用 legacy desktop surface。
        "desktop",
        // 使用 record operation。
        "record",
        // 开始 target 参数。
        "--target",
        // 使用语法有效但必然 stale 的 canonical 目标。
        &format!("sessionId={session_id}"),
        // 开始 MP4 参数。
        "--arg",
        // 传递精确最终路径。
        &format!("path={}", video.display()),
        // 开始分析目录参数。
        "--arg",
        // 传递精确最终目录。
        &format!("analysisDir={}", analysis.display()),
        // 使用最短公开时长。
        "--arg",
        // 传递固定时长。
        "durationMs=1000",
    ]);
    // 可选注入禁止字段以验证封闭配置。
    if unknown_field {
        // caller 不能提供编码器路径。
        command.args(["--arg", "encoderPath=forbidden.exe"]);
    }
    // 已确认用例添加确认标志。
    if confirmed {
        // confirmation 作为独立参数。
        command.arg("--confirm");
    }
    // 执行并收集单个 JSON stdout。
    command
        // 等待主程序完成。
        .output()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("toolkit launch failed: {error}"))
}

// 直接运行严格 recording worker 请求。
fn run_worker(request: &Value) -> Output {
    // 启动固定 Cargo worker 二进制。
    let mut child = Command::new(WORKER)
        // 打开 stdin 供单个 JSON 请求。
        .stdin(Stdio::piped())
        // 收集单行 stdout。
        .stdout(Stdio::piped())
        // 收集诊断但不解析为协议。
        .stderr(Stdio::piped())
        // 启动 worker。
        .spawn()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("recording worker launch failed: {error}"));
    // 序列化严格请求。
    let bytes = serde_json::to_vec(request)
        // 测试 JSON 必须可序列化。
        .unwrap_or_else(|error| panic!("worker request serialization failed: {error}"));
    // 写入单个请求并关闭 stdin。
    std::io::Write::write_all(
        // 取得 worker stdin。
        child
            // 借用可变 stdin。
            .stdin
            // 缺失表示测试进程配置失效。
            .as_mut()
            // 直接失败并附带诊断。
            .unwrap_or_else(|| panic!("recording worker stdin missing")),
        // 写入完整 JSON。
        &bytes,
    )
    // 写入失败时提供诊断。
    .unwrap_or_else(|error| panic!("recording worker stdin write failed: {error}"));
    // 关闭 stdin 触发 worker 单次执行。
    drop(child.stdin.take());
    // 等待并收集结果。
    child
        // 收集 stdout/stderr。
        .wait_with_output()
        // wait 失败时提供诊断。
        .unwrap_or_else(|error| panic!("recording worker wait failed: {error}"))
}

// 解析单个 JSON stdout。
fn output_json(output: &Output) -> Value {
    // stdout 必须是 UTF-8 JSON。
    serde_json::from_slice(&output.stdout)
        // 解析失败时附带安全测试诊断。
        .unwrap_or_else(|error| panic!("invalid JSON stdout: {error}"))
}

// confirmation 必须早于路径、目标和 worker 可达检查。
#[test]
// 验证未确认请求不产生任何文件。
fn desktop_record_is_confirmation_first() {
    // recording worker 必须由 Cargo 构建。
    assert!(Path::new(WORKER).is_file());
    // 创建独占测试目录。
    let fixture = FixtureDirectory::create();
    // 定义未确认视频路径。
    let video = fixture.join("unconfirmed.mp4");
    // 定义未确认分析目录。
    let analysis = fixture.join("unconfirmed.analysis");
    // 执行未确认请求。
    let output = run_desktop_record(
        // 使用语法有效但 stale 的 canonical 目标。
        "s2:w:0000000000000000",
        // 传递视频路径。
        &video,
        // 传递分析目录。
        &analysis,
        // 缺少确认。
        false,
        // 不注入未知字段。
        false,
    );
    // 请求必须失败。
    assert!(!output.status.success());
    // 首个错误必须是 confirmation required。
    assert_eq!(
        output_json(&output)["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    // 不得创建 MP4。
    assert!(!video.exists());
    // 不得创建分析目录。
    assert!(!analysis.exists());
}

// 公开配置必须拒绝任意编码器字段且不触碰输出。
#[test]
// 验证 caller-supplied encoder path 失败闭合。
fn desktop_record_rejects_unknown_encoder_field() {
    // 创建独占测试目录。
    let fixture = FixtureDirectory::create();
    // 定义视频路径。
    let video = fixture.join("unknown.mp4");
    // 定义分析目录。
    let analysis = fixture.join("unknown.analysis");
    // 执行带确认但含协议外字段的请求。
    let output = run_desktop_record(
        // 使用语法有效但 stale 的 canonical 目标。
        "s2:w:0000000000000000",
        // 传递视频路径。
        &video,
        // 传递分析目录。
        &analysis,
        // 提供确认。
        true,
        // 注入未知字段。
        true,
    );
    // 请求必须失败。
    assert!(!output.status.success());
    // 必须返回参数错误而不是启动 target 或 runtime。
    assert_eq!(output_json(&output)["error"]["code"], "INVALID_ARGUMENT");
    // 不得创建 MP4。
    assert!(!video.exists());
    // 不得创建分析目录。
    assert!(!analysis.exists());
}

// worker 必须独立执行 confirmation-first。
#[test]
// 验证无效 staging 不能掩盖确认缺失。
fn recording_worker_is_independently_confirmation_first() {
    // 构造含无效路径但未确认的严格请求。
    let request = json!({
        // 使用正确 worker 协议。
        "contractVersion": "act/recording-worker/v2",
        // 使用 canonical 目标语法。
        "sessionId": "s2:w:0000000000000000",
        // 明确缺少确认。
        "confirmed": false,
        // 提供不会被读取的输入。
        "input": { "path": "missing.mp4" },
        // 提供不会被检查的视频 staging。
        "videoStagingPath": "missing-video-staging",
        // 提供不会被检查的分析 staging。
        "analysisStagingDir": "missing-analysis-staging",
    });
    // 直接执行 worker。
    let output = run_worker(&request);
    // worker 必须非零退出。
    assert!(!output.status.success());
    // confirmation 必须是首个错误。
    assert_eq!(
        output_json(&output)["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
}

// 项目自有 Media Foundation 编码必须完成真实工具窗口录制。
#[test]
// 使用 no-activate 自有窗口验证生产 launcher、worker、MP4 与分析事务。
fn media_foundation_recording_succeeds_for_owned_window() {
    // 查询当前项目自有编码链路可达事实。
    let status_output = Command::new(TOOLKIT)
        // 使用只读 desktop status。
        .args(["status", "desktop"])
        // 执行并收集 JSON。
        .output()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("desktop status failed: {error}"));
    // status 必须成功。
    assert!(status_output.status.success());
    // 解析当前编码器状态。
    let available = output_json(&status_output)["videoRecording"]["available"]
        // 必须是布尔值。
        .as_bool()
        // 缺失表示 status 契约失效。
        .unwrap_or_else(|| panic!("desktop recording availability missing"));
    // 当前 Windows 验证机必须提供系统内建 H.264 编码能力。
    assert!(available);
    // 创建独占输出目录。
    let fixture = FixtureDirectory::create();
    // 启动 no-activate 自有窗口。
    let mut window = FixtureWindow::start();
    // 唯一发现 canonical session。
    let session_id = window.session_id();
    // 对同一自有目标执行无副作用 capability assessment。
    let assessment_output = Command::new(TOOLKIT)
        // 传递固定 assessment 参数。
        .args([
            // 使用 assess 动词。
            "assess",
            // 使用统一 app surface。
            "app",
            // 开始 capability 参数。
            "--capability",
            // 选择窗口录制 capability。
            "window.record@1",
            // 开始 target 参数。
            "--target",
            // 传递自有 canonical 目标。
            &format!("sessionId={session_id}"),
        ])
        // 执行并收集 JSON。
        .output()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("recording assessment failed: {error}"));
    // assessment 必须成功。
    assert!(assessment_output.status.success());
    // 解析静态许可与动态目标结果。
    let assessment = output_json(&assessment_output);
    // 未确认时必须返回 confirmation-required。
    assert_eq!(assessment["decision"], "confirmation-required");
    // 认证执行域必须是隔离 worker。
    assert_eq!(assessment["executionRealm"], "isolated-worker");
    // 实现状态必须标记 Rust 可用待确认。
    assert_eq!(
        assessment["evidence"]["implementationState"],
        "rust-available-awaiting-confirmation"
    );
    // 定义最终视频路径。
    let video = fixture.join("media-foundation.mp4");
    // 定义最终分析目录。
    let analysis = fixture.join("media-foundation.analysis");
    // 执行确认后的真实窗口录制。
    let output = run_desktop_record(
        // 传递自有 canonical 目标。
        &session_id,
        // 传递最终视频路径。
        &video,
        // 传递最终分析目录。
        &analysis,
        // 提供逐操作确认。
        true,
        // 不注入未知字段。
        false,
    );
    // 生产录制必须成功。
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // 解析 provider-neutral 成功结果。
    let result = output_json(&output);
    // 公开编码器不得出现外部 runtime。
    assert_eq!(result["encoder"], "windows-media-foundation-h264");
    // 前景必须保持不变。
    assert_eq!(result["foreground"]["unchanged"], true);
    // 真实 MP4 必须存在且非空。
    assert!(fs::metadata(&video).map(|value| value.len()).unwrap_or(0) > 1_024);
    // 分析 manifest 必须完成事务提交。
    let manifest = fs::read(analysis.join("manifest.json"))
        // 提交后的 manifest 必须可读。
        .unwrap_or_else(|error| panic!("recording manifest missing: {error}"));
    // 解析 manifest。
    let manifest: Value = serde_json::from_slice(&manifest)
        // 动态产物必须是合法 JSON。
        .unwrap_or_else(|error| panic!("recording manifest invalid: {error}"));
    // manifest 必须公开 provider-neutral 质量而不是 CRF。
    assert_eq!(manifest["video"]["quality"], 75);
    // manifest 必须记录固定 H.264 编码事实。
    assert_eq!(manifest["video"]["codec"], "H.264");
    // 根目录只允许最终 MP4 和最终分析目录，不得遗留私有 staging。
    let entries = fs::read_dir(&fixture.path)
        // 测试目录必须可读。
        .unwrap_or_else(|error| panic!("fixture read failed: {error}"))
        // 统计最终产物与残留。
        .count();
    // 只保留两个已提交最终目标。
    assert_eq!(entries, 2);
}

// 录制 worker deadline 必须回收 Job 并清理父 Module staging。
#[test]
// 使用挂起 sibling 替身验证生产 launcher 的真实 timeout 与事务回滚。
fn recording_timeout_reaps_worker_and_cleans_staging() {
    // 创建只属于本测试的可执行文件与输出根。
    let fixture = FixtureDirectory::create();
    // 构造复制后的 toolkit 路径。
    let toolkit = fixture.join("ai-computer-toolkit-timeout.exe");
    // 构造生产代码固定查找的 recording worker sibling 路径。
    let worker = fixture.join("ai-computer-toolkit-recording-worker.exe");
    // 复制当前测试构建的正式 toolkit。
    fs::copy(TOOLKIT, &toolkit)
        // 复制失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("toolkit fixture copy failed: {error}"));
    // 把仓库自有挂起 fixture 复制为固定 sibling worker。
    fs::copy(HANG_FIXTURE, &worker)
        // 复制失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("worker fixture copy failed: {error}"));
    // 创建隔离输出目录以单独统计 staging 残留。
    let outputs = fixture.join("outputs");
    // 输出目录必须真实存在。
    fs::create_dir(&outputs)
        // 创建失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("recording output fixture failed: {error}"));
    // 启动 no-activate 自有窗口供主 Module 完成真实 target 与 preflight。
    let mut window = FixtureWindow::start();
    // 唯一发现 canonical session。
    let session_id = window.session_id();
    // 构造最终视频路径。
    let video = outputs.join("timeout.mp4");
    // 构造最终分析目录。
    let analysis = outputs.join("timeout.analysis");
    // 启动复制后的正式 toolkit。
    let output = Command::new(&toolkit)
        // 让 sibling 替身进入永久挂起模式。
        .env("ACT_BROWSER_FIXTURE_MODE", "hang")
        // 传递固定录制 surface、精确目标和最短 deadline 参数。
        .args([
            // 使用 run 动词。
            "run",
            // 使用 desktop surface。
            "desktop",
            // 使用 record operation。
            "record",
            // 开始目标参数。
            "--target",
            // 传递 canonical 目标。
            &format!("sessionId={session_id}"),
            // 开始视频路径参数。
            "--arg",
            // 传递最终视频路径。
            &format!("path={}", video.display()),
            // 开始分析路径参数。
            "--arg",
            // 传递最终分析路径。
            &format!("analysisDir={}", analysis.display()),
            // 使用最短录制时长。
            "--arg",
            // 固定一秒录制。
            "durationMs=1000",
            // 使用最短单帧等待时长。
            "--arg",
            // 把 watchdog 收窄到 6.25 秒总边界。
            "timeoutMs=250",
            // 提供逐操作确认。
            "--confirm",
        ])
        // 执行并等待 Job watchdog 回收。
        .output()
        // 启动失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("timeout toolkit launch failed: {error}"));
    // timeout 必须返回非零退出码。
    assert!(!output.status.success());
    // 结构化错误必须说明录制 deadline。
    assert_eq!(
        output_json(&output)["error"]["code"],
        "VIDEO_RECORDING_TIMEOUT"
    );
    // 最终视频不得被提交。
    assert!(!video.exists());
    // 最终分析目录不得被提交。
    assert!(!analysis.exists());
    // Job 返回后输出目录不得留下任何 staging。
    assert_eq!(
        // 枚举隔离输出目录。
        fs::read_dir(&outputs)
            // 目录必须可读。
            .unwrap_or_else(|error| panic!("timeout output read failed: {error}"))
            // 统计所有残留。
            .count(),
        // 期望零残留。
        0
    );
}
