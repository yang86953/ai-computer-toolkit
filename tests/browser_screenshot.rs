#![cfg(target_os = "windows")]

//! 验证 Rust 浏览器截图的隔离、原子输出与 launcher 正式路由。

// 导入文件系统、路径、进程、线程与时间工具。
use std::{
    // 导入文件操作。
    fs,
    // 导入路径类型。
    path::{Path, PathBuf},
    // 导入子进程命令与输出。
    process::{Command, Output},
    // 导入进程内测试串行锁。
    sync::Mutex,
    // 导入清理轮询休眠。
    thread,
    // 导入唯一时间戳与等待时长。
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// 导入 JSON 值。
use serde_json::Value;

// 固定主程序测试二进制。
const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
// 固定浏览器 worker 测试二进制。
const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-worker");
// 固定浏览器 fixture 测试二进制。
const FIXTURE_BROWSER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-fixture");
// 串行化会观察共享临时 profile 根的动态测试。
static TEST_LOCK: Mutex<()> = Mutex::new(());

// 拥有一个精确测试目录。
struct FixtureDirectory {
    // 保存测试根路径。
    path: PathBuf,
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
            "act-browser-screenshot-test-{}-{stamp}",
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

// 运行固定 Rust 主程序浏览器截图。
fn run_browser(output: &Path, confirmed: bool, timeout_ms: u32, mode: Option<&str>) -> Output {
    // 构造不经过 shell 的主程序命令。
    let mut command = Command::new(TOOLKIT);
    // 固定使用仓库自有 browser fixture。
    command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", FIXTURE_BROWSER);
    // 按测试用例设置可选挂起模式。
    if let Some(mode) = mode {
        // 只向 fixture 子进程传递模式。
        command.env("ACT_BROWSER_FIXTURE_MODE", mode);
    }
    // 构造仓库自有 HTML fixture URL。
    let url = repository_fixture_url();
    // 传递固定公开命令与封闭参数。
    command.args([
        // 使用 run 动词。
        "run",
        // 使用 browser surface。
        "browser",
        // 使用 screenshot operation。
        "screenshot",
        // 开始 URL target。
        "--target",
        // 传递独立 URL assignment。
        &format!("url={url}"),
        // 开始输出参数。
        "--arg",
        // 传递精确输出路径。
        &format!("path={}", output.display()),
        // 开始宽度参数。
        "--arg",
        // 固定 fixture 宽度。
        "width=640",
        // 开始高度参数。
        "--arg",
        // 固定 fixture 高度。
        "height=360",
        // 开始 deadline 参数。
        "--arg",
        // 传递用例 deadline。
        &format!("timeoutMs={timeout_ms}"),
    ]);
    // 已确认用例显式添加确认标志。
    if confirmed {
        // confirmation 必须作为独立参数。
        command.arg("--confirm");
    }
    // 执行并收集 JSON stdout。
    command
        // 启动子进程并等待完成。
        .output()
        // 启动失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("toolkit launch failed: {error}"))
}

// 运行 PowerShell launcher 的同一浏览器请求。
fn run_launcher(output: &Path) -> Output {
    // 定位仓库 launcher 脚本。
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR"))
        // 进入工具目录。
        .join("tools/windows")
        // 选择固定 launcher。
        .join("Invoke-ComputerControl.ps1");
    // 构造仓库 HTML fixture URL。
    let url = repository_fixture_url();
    // 启动 Windows PowerShell。
    let mut command = Command::new("powershell.exe");
    // 固定使用仓库 browser fixture。
    command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", FIXTURE_BROWSER);
    // 传递脚本和公开参数。
    command.args([
        // 禁止加载用户 profile。
        "-NoProfile",
        // 允许执行仓库脚本。
        "-ExecutionPolicy",
        // 只绕过当前进程策略。
        "Bypass",
        // 以文件模式执行。
        "-File",
        // 传递 launcher 路径。
        &launcher.display().to_string(),
        // 使用 run 动词。
        "run",
        // 使用 browser surface。
        "browser",
        // 使用 screenshot operation。
        "screenshot",
        // 开始 URL target。
        "--target",
        // 传递 URL。
        &format!("url={url}"),
        // 开始路径参数。
        "--arg",
        // 传递输出路径。
        &format!("path={}", output.display()),
        // 开始宽度参数。
        "--arg",
        // 固定宽度。
        "width=640",
        // 开始高度参数。
        "--arg",
        // 固定高度。
        "height=360",
        // 开始 deadline 参数。
        "--arg",
        // 使用标准 deadline。
        "timeoutMs=30000",
        // 显式确认。
        "--confirm",
    ]);
    // 执行 launcher 并收集输出。
    command
        // 等待脚本结束。
        .output()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("launcher failed: {error}"))
}

// 构造仓库自有 HTML fixture 的 file URL。
fn repository_fixture_url() -> String {
    // 定位 fixture 文件。
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        // 进入 tests。
        .join("tests")
        // 进入 fixtures。
        .join("fixtures")
        // 选择固定页面。
        .join("headless-page.html");
    // 转换 Windows 分隔符为 URL 分隔符。
    let normalized = fixture.display().to_string().replace('\\', "/");
    // 构造本地文件 URL。
    format!("file:///{normalized}")
}

// 解析单个 JSON stdout。
fn json(output: &Output) -> Value {
    // stdout 必须是 UTF-8 JSON。
    serde_json::from_slice(&output.stdout)
        // 解析失败时附带安全测试诊断。
        .unwrap_or_else(|error| panic!("invalid JSON stdout: {error}"))
}

// 读取 PNG IHDR 尺寸。
fn png_dimensions(path: &Path) -> (u32, u32) {
    // 读取测试候选。
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("PNG read failed: {error}"));
    // 验证最短头长度。
    assert!(bytes.len() >= 24);
    // 验证 PNG 签名。
    assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    // 解析大端宽度。
    let width = u32::from_be_bytes(
        // 固定四字节转换。
        bytes[16..20]
            // 转换为数组。
            .try_into()
            // 固定长度必须成功。
            .unwrap_or_else(|_| panic!("PNG width bytes invalid")),
    );
    // 解析大端高度。
    let height = u32::from_be_bytes(
        // 固定四字节转换。
        bytes[20..24]
            // 转换为数组。
            .try_into()
            // 固定长度必须成功。
            .unwrap_or_else(|_| panic!("PNG height bytes invalid")),
    );
    // 返回尺寸。
    (width, height)
}

// 等待 profile 根恢复为空。
fn assert_profiles_clean() {
    // 构造固定工具 profile 根。
    let root = std::env::temp_dir().join("ai-computer-toolkit-browser");
    // 给进程树回收与目录关闭固定短宽限。
    for _ in 0..50 {
        // 缺失根或空根都表示清理完成。
        if fs::read_dir(&root)
            // 成功枚举时检查是否为空。
            .map(|mut entries| entries.next().is_none())
            // 根不存在同样视为干净。
            .unwrap_or(true)
        {
            // 清理完成后返回。
            return;
        }
        // 等待下一次检查。
        thread::sleep(Duration::from_millis(20));
    }
    // 超出宽限时失败。
    panic!("isolated browser profile was not cleaned");
}

// 验证 confirmation-first、成功、覆盖与 timeout 契约。
#[test]
// 串行覆盖同一 fixture 生命周期。
fn rust_browser_screenshot_contract_is_closed_and_atomic() {
    // 独占共享 profile 根观察窗口。
    let _guard = TEST_LOCK
        // 获取串行锁。
        .lock()
        // 测试锁中毒时提供明确诊断。
        .unwrap_or_else(|error| panic!("browser test lock failed: {error}"));
    // 二进制必须由 Cargo 安装到同一 sibling 目录。
    assert!(Path::new(WORKER).is_file());
    // 创建独占测试目录。
    let fixture = FixtureDirectory::create();
    // 定义未确认输出。
    let unconfirmed_path = fixture.join("unconfirmed.png");
    // 未确认请求必须在任何写入前失败。
    let unconfirmed = run_browser(&unconfirmed_path, false, 30_000, None);
    // 进程必须非零退出。
    assert!(!unconfirmed.status.success());
    // 解析错误 envelope。
    let unconfirmed_json = json(&unconfirmed);
    // confirmation 必须是首个错误。
    assert_eq!(unconfirmed_json["error"]["code"], "CONFIRMATION_REQUIRED");
    // 不得产生输出。
    assert!(!unconfirmed_path.exists());
    // 定义成功输出。
    let output_path = fixture.join("fixture.png");
    // 执行确认后的固定 fixture 截图。
    let success = run_browser(&output_path, true, 30_000, None);
    // 成功必须零退出。
    assert!(success.status.success());
    // 解析成功 envelope。
    let success_json = json(&success);
    // 核对 Rust capability。
    assert_eq!(success_json["capability"], "browser.screenshot@1");
    // 核对隔离 worker 认证。
    assert_eq!(success_json["executionRealmCertified"], true);
    // 核对原子输出。
    assert_eq!(success_json["atomicOutput"], true);
    // 核对无 runtime 路径泄漏。
    assert_eq!(success_json["runtimePathExposed"], false);
    // 核对 IHDR 尺寸。
    assert_eq!(png_dimensions(&output_path), (640, 360));
    // 保存原始字节供覆盖拒绝检查。
    let original = fs::read(&output_path)
        // 读取失败时提供诊断。
        .unwrap_or_else(|error| panic!("original PNG read failed: {error}"));
    // 未带 overwrite 的重复请求必须失败。
    let overwrite = run_browser(&output_path, true, 30_000, None);
    // 重复请求必须非零退出。
    assert!(!overwrite.status.success());
    // 核对覆盖确认错误。
    assert_eq!(
        json(&overwrite)["error"]["code"],
        "OVERWRITE_CONFIRMATION_REQUIRED"
    );
    // 原始文件必须逐字节保持。
    assert_eq!(
        fs::read(&output_path).ok().as_deref(),
        Some(original.as_slice())
    );
    // 定义超时输出。
    let timeout_path = fixture.join("timeout.png");
    // 使用挂起 fixture 触发 worker Job deadline。
    let timeout = run_browser(&timeout_path, true, 1_000, Some("hang"));
    // 超时必须非零退出。
    assert!(!timeout.status.success());
    // 核对浏览器超时错误码。
    assert_eq!(json(&timeout)["error"]["code"], "BROWSER_TIMEOUT");
    // 超时不得提交公开输出。
    assert!(!timeout_path.exists());
    // 所有 profile 必须清理。
    assert_profiles_clean();
}

// 验证 launcher 不再路由 C++。
#[test]
// 使用真实 PowerShell launcher 与 Rust fixture。
fn launcher_uses_rust_browser_worker() {
    // 独占共享 profile 根观察窗口。
    let _guard = TEST_LOCK
        // 获取串行锁。
        .lock()
        // 测试锁中毒时提供明确诊断。
        .unwrap_or_else(|error| panic!("browser test lock failed: {error}"));
    // 创建独占测试目录。
    let fixture = FixtureDirectory::create();
    // 定义 launcher 输出。
    let output_path = fixture.join("launcher.png");
    // 执行 launcher。
    let output = run_launcher(&output_path);
    // launcher 必须成功。
    assert!(output.status.success());
    // 解析 Rust 结果。
    let value = json(&output);
    // 核对隔离 worker 认证。
    assert_eq!(value["executionRealmCertified"], true);
    // 核对固定 capability。
    assert_eq!(value["capability"], "browser.screenshot@1");
    // 核对输出尺寸。
    assert_eq!(png_dimensions(&output_path), (640, 360));
    // 所有 profile 必须清理。
    assert_profiles_clean();
}
