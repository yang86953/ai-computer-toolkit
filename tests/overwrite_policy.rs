#![cfg(target_os = "windows")]

//! 验证公开 Rust policy 的基础确认与覆盖确认保持独立且完整。

// 导入文件系统、路径与无锁序列工具。
use std::{
    // 导入测试夹具文件操作。
    fs,
    // 导入测试夹具路径类型。
    path::PathBuf,
    // 导入并行测试安全的唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入公开请求、隔离要求、操作类型与 policy。
use ai_computer_toolkit::{
    // 导入请求强类型。
    domain::{CommandRequest, IsolationRequirement, Verb},
    // 导入 System 策略入口。
    policy,
};
// 导入 JSON 映射和值构造器。
use serde_json::{Map, json};

// 为并行测试生成无碰撞目录序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占并清理一次公开策略测试目录。
struct Fixture {
    // 保存测试自有根目录。
    root: PathBuf,
}

// 创建唯一公开策略测试目录。
impl Fixture {
    // 建立当前测试的自有目录。
    fn new(name: &str) -> std::io::Result<Self> {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录下构造工具自有路径。
        let root = std::env::temp_dir().join(format!(
            // 固定前缀并加入进程与序列。
            "ai-computer-toolkit-overwrite-policy-{name}-{}-{sequence}",
            // 加入进程 ID 避免跨进程碰撞。
            std::process::id()
        ));
        // 创建测试根目录。
        fs::create_dir(&root)?;
        // 返回唯一清理所有者。
        Ok(Self { root })
    }

    // 在测试根目录下构造路径。
    fn path(&self, name: &str) -> PathBuf {
        // 只返回当前夹具内部路径。
        self.root.join(name)
    }
}

// 测试结束时递归清理唯一自有目录。
impl Drop for Fixture {
    // 回收本测试创建的全部夹具。
    fn drop(&mut self) {
        // 只删除构造器创建的精确根目录。
        let _ = fs::remove_dir_all(&self.root);
    }
}

// 构造标准确认式运行请求。
fn confirmed_request(app: &str, operation: &str) -> CommandRequest {
    // 返回只进入纯策略层的请求。
    CommandRequest {
        // 使用公开 run verb。
        verb: Verb::Run,
        // 保存目标 surface。
        app: app.to_owned(),
        // 保存固定 operation。
        operation: Some(operation.to_owned()),
        // 初始目标为空，由测试补齐。
        target: Map::new(),
        // 初始参数为空，由测试补齐。
        args: Map::new(),
        // 使用目录默认上限。
        max_items: 50,
        // 使用目录默认深度。
        max_depth: 4,
        // 满足基础变更确认。
        confirmed: true,
        // 当前输出操作不需要前台许可。
        foreground_consent: false,
        // 使用标准兼容隔离策略。
        isolation_requirement: IsolationRequirement::Standard,
    }
}

// 为精确窗口输出补齐测试目标。
fn add_window_target(request: &mut CommandRequest) {
    // 写入不会解析到真实窗口的旧兼容目标。
    request
        // 访问目标映射。
        .target
        // 插入固定 sessionId。
        .insert("sessionId".to_owned(), json!("window:1"));
}

// 基础确认必须先于覆盖确认返回。
#[test]
fn base_confirmation_precedes_overwrite_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("base-first")?;
    // 构造既有截图路径。
    let output = fixture.path("screen.png");
    // 写入可验证的原始内容。
    fs::write(&output, b"original")?;
    // 构造原本已满足基础确认的请求。
    let mut request = confirmed_request("desktop", "screenshot");
    // 撤销基础确认以验证门禁顺序。
    request.confirmed = false;
    // 补齐精确窗口目标。
    add_window_target(&mut request);
    // 写入既有输出路径。
    request.args.insert("path".to_owned(), json!(&output));
    // 基础确认缺失必须先返回。
    let error = match policy::validate(&request) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("基础确认必须优先").into()),
        // 保存稳定公开错误。
        Err(error) => error,
    };
    // 核对基础确认错误码。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    // 返回测试成功。
    Ok(())
}

// 桌面截图与浏览器截图必须共享覆盖确认错误。
#[test]
fn screenshot_surfaces_require_overwrite_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("screenshots")?;
    // 构造桌面截图既有输出。
    let desktop_output = fixture.path("desktop.png");
    // 写入桌面截图原始内容。
    fs::write(&desktop_output, b"desktop-original")?;
    // 构造确认式桌面截图请求。
    let mut desktop = confirmed_request("desktop", "screenshot");
    // 补齐精确窗口目标。
    add_window_target(&mut desktop);
    // 写入既有输出路径。
    desktop
        // 访问参数映射。
        .args
        // 插入桌面输出路径。
        .insert("path".to_owned(), json!(&desktop_output));
    // 未确认覆盖必须拒绝桌面截图。
    let desktop_error = match policy::validate(&desktop) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("桌面截图覆盖必须被拒绝").into()),
        // 保存稳定公开错误。
        Err(error) => error,
    };
    // 核对稳定覆盖错误码。
    assert_eq!(desktop_error.code, "OVERWRITE_CONFIRMATION_REQUIRED");

    // 构造浏览器截图既有输出。
    let browser_output = fixture.path("browser.png");
    // 写入浏览器截图原始内容。
    fs::write(&browser_output, b"browser-original")?;
    // 构造确认式浏览器截图请求。
    let mut browser = confirmed_request("browser", "screenshot");
    // 补齐固定安全 URL。
    browser
        // 访问目标映射。
        .target
        // 插入仓库无关的 HTTPS 目标。
        .insert("url".to_owned(), json!("https://example.com"));
    // 写入既有输出路径。
    browser
        // 访问参数映射。
        .args
        // 插入浏览器输出路径。
        .insert("path".to_owned(), json!(&browser_output));
    // 未确认覆盖必须拒绝浏览器截图。
    let browser_error = match policy::validate(&browser) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("浏览器截图覆盖必须被拒绝").into()),
        // 保存稳定公开错误。
        Err(error) => error,
    };
    // 核对稳定覆盖错误码。
    assert_eq!(browser_error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
    // 两个原始文件都不得被纯策略检查修改。
    assert_eq!(
        // 回读桌面截图夹具。
        fs::read(&desktop_output)?,
        // 核对桌面原始字节。
        b"desktop-original"
    );
    // 浏览器原始文件也必须保持不变。
    assert_eq!(
        // 回读浏览器截图夹具。
        fs::read(&browser_output)?,
        // 核对浏览器原始字节。
        b"browser-original"
    );
    // 返回测试成功。
    Ok(())
}

// 显式覆盖许可必须允许纯策略继续而不修改既有文件。
#[test]
fn explicit_overwrite_allows_public_policy_to_continue() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("confirmed-public")?;
    // 为桌面截图、浏览器截图和录制构造既有输出。
    for (app, operation, name) in [
        // 桌面截图输出。
        ("desktop", "screenshot", "desktop.png"),
        // 浏览器截图输出。
        ("browser", "screenshot", "browser.png"),
        // 桌面录制输出。
        ("desktop", "record", "recording.mp4"),
    ] {
        // 构造本次既有输出路径。
        let output = fixture.path(name);
        // 写入可验证的原始内容。
        fs::write(&output, b"original")?;
        // 构造基础确认已满足的请求。
        let mut request = confirmed_request(app, operation);
        // 浏览器截图使用 URL 目标。
        if app == "browser" {
            // 插入固定安全 URL。
            request
                // 访问目标映射。
                .target
                // 写入 URL。
                .insert("url".to_owned(), json!("https://example.com"));
        } else {
            // 窗口截图和录制补齐精确目标。
            add_window_target(&mut request);
        }
        // 写入既有输出路径。
        request.args.insert("path".to_owned(), json!(&output));
        // 提供独立覆盖许可。
        request
            // 访问参数映射。
            .args
            // 插入严格布尔 true。
            .insert("overwrite".to_owned(), json!(true));
        // 纯策略层必须允许请求继续到后续目标/provider 边界。
        assert!(policy::validate(&request).is_ok(), "{app}.{operation}");
        // 纯策略检查不得自行修改文件。
        assert_eq!(
            // 回读原始文件。
            fs::read(&output)?,
            // 核对原始字节保持不变。
            b"original"
        );
    }
    // 返回测试成功。
    Ok(())
}

// 主视频已存在时必须在窗口解析前要求覆盖确认。
#[test]
fn recording_file_requires_overwrite_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("recording-file")?;
    // 构造既有视频输出。
    let output = fixture.path("operation.mp4");
    // 写入视频原始内容。
    fs::write(&output, b"video-original")?;
    // 构造确认式录制请求。
    let mut request = confirmed_request("desktop", "record");
    // 补齐精确窗口目标。
    add_window_target(&mut request);
    // 写入既有视频路径。
    request.args.insert("path".to_owned(), json!(&output));
    // 未确认覆盖必须拒绝录制。
    let error = match policy::validate(&request) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("视频覆盖必须被拒绝").into()),
        // 保存稳定公开错误。
        Err(error) => error,
    };
    // 核对稳定覆盖错误码。
    assert_eq!(error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
    // 视频原始文件必须保持不变。
    assert_eq!(
        // 回读视频夹具。
        fs::read(&output)?,
        // 核对视频原始字节。
        b"video-original"
    );
    // 返回测试成功。
    Ok(())
}

// 非空分析目录必须取得同一次独立覆盖许可。
#[test]
fn non_empty_analysis_directory_requires_overwrite_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("analysis-directory")?;
    // 构造首次写入的视频路径。
    let output = fixture.path("operation.mp4");
    // 构造既有分析目录。
    let analysis = fixture.path("operation.analysis");
    // 创建分析目录。
    fs::create_dir(&analysis)?;
    // 写入工具自有分析占位文件。
    fs::write(analysis.join("manifest.json"), b"analysis-original")?;
    // 构造确认式录制请求。
    let mut request = confirmed_request("desktop", "record");
    // 补齐精确窗口目标。
    add_window_target(&mut request);
    // 写入首次视频输出路径。
    request.args.insert("path".to_owned(), json!(&output));
    // 显式指定非空分析目录。
    request
        // 访问参数映射。
        .args
        // 插入分析目录路径。
        .insert("analysisDir".to_owned(), json!(&analysis));
    // 未确认覆盖必须拒绝复用非空目录。
    let error = match policy::validate(&request) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("非空分析目录必须要求覆盖确认").into()),
        // 保存稳定公开错误。
        Err(error) => error,
    };
    // 核对稳定覆盖错误码。
    assert_eq!(error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
    // 首次视频目标不得被策略创建。
    assert!(!output.exists());
    // 分析原始文件必须保持不变。
    assert_eq!(
        // 回读分析夹具。
        fs::read(analysis.join("manifest.json"))?,
        // 核对分析原始字节。
        b"analysis-original"
    );
    // 返回测试成功。
    Ok(())
}
