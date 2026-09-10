#![cfg(target_os = "windows")]

//! 验证通用进程终止的 System 策略与生产 facade 路由。

// 导入生产 launcher 进程与仓库路径工具。
use std::{path::Path, process::Command};

// 导入生产 System、强类型隔离要求与请求类型。
use ai_computer_toolkit::{
    // 构造 launcher 使用的同一 ComputerControlSystem。
    AppControlService,
    // 构造 provider-neutral 请求并切换严格隔离模式。
    domain::{CommandRequest, IsolationRequirement, Verb},
};
// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 固定两个互不回退的进程终止 capability。
const TERMINATION_CAPABILITIES: [&str; 2] = [
    // 优雅终止只投递通用关闭请求。
    "process.terminate.graceful@1",
    // 强制终止只执行内核终止。
    "process.terminate.force@1",
];

// 构造完整 provider-neutral app.close 请求。
fn termination_request(capability: &str) -> CommandRequest {
    // 从统一运行请求开始。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 两级风险共享 generic close 动作。
    request.operation = Some("close".to_owned());
    // 写入不会命中当前进程的 canonical opaque 目标。
    request.target.insert(
        // 使用稳定 sessionId 字段。
        "sessionId".to_owned(),
        // 使用固定全零测试指纹。
        json!("s2:p:0000000000000000"),
    );
    // 写入调用方明确选择的版本化风险 capability。
    request
        // 访问参数对象。
        .args
        // 插入固定 capability 字段。
        .insert("capability".to_owned(), json!(capability));
    // 空对象选择契约默认有界 deadline。
    request
        // 访问参数对象。
        .args
        // 插入严格 input。
        .insert("input".to_owned(), json!({}));
    // 返回尚未确认的请求供门禁测试决定。
    request
}

// 通过唯一生产 PowerShell launcher 执行固定 stale 请求夹具。
fn launcher_stale_request(fixture: &str) -> Result<Value, Box<dyn std::error::Error>> {
    // 读取编译期仓库根目录。
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // 定位唯一生产 launcher。
    let launcher = root.join("tools").join("Invoke-ComputerControl.ps1");
    // 定位本轮固定 JSON 请求夹具。
    let input = root.join("tests").join("fixtures").join(fixture);
    // 启动生产 PowerShell launcher 并收集唯一 JSON 输出。
    let output = Command::new("powershell")
        // 禁止加载用户 profile。
        .arg("-NoProfile")
        // 只为仓库脚本绕过本机会话策略。
        .args(["-ExecutionPolicy", "Bypass"])
        // 使用脚本文件入口。
        .arg("-File")
        // 传入唯一生产 launcher。
        .arg(launcher)
        // 选择通用运行命令。
        .args(["run", "app", "close"])
        // 选择完整 JSON 请求文件。
        .arg("--input")
        // 传入固定 stale 夹具。
        .arg(input)
        // 执行并等待有界 stale 路由完成。
        .output()?;
    // stale 目标必须以非零状态失败闭合。
    assert!(!output.status.success());
    // stdout 必须是唯一结构化 JSON envelope。
    let result = serde_json::from_slice(&output.stdout)?;
    // 返回供测试核对领域分类。
    Ok(result)
}

// 验证两级进程终止共享确认门禁，但保持独立 capability 与固定后台执行域。
#[test]
fn confirmed_background_plans_remain_distinct() -> Result<(), Box<dyn std::error::Error>> {
    // 逐一覆盖优雅与强制终止能力。
    for capability in TERMINATION_CAPABILITIES {
        // 构造最小 provider-neutral app.close 请求。
        let mut request = termination_request(capability);
        // 未确认请求必须先被 System 策略拒绝。
        let error = match ai_computer_toolkit::policy::validate(&request) {
            // 意外成功表示进程终止绕过了确认门禁。
            Ok(()) => return Err(std::io::Error::other("进程终止必须要求逐操作确认").into()),
            // 保存结构化策略错误。
            Err(error) => error,
        };
        // 确认门禁必须早于目标重解析。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        // 满足逐操作确认以验证普通后台计划可由 System 接受。
        request.confirmed = true;
        // 普通隔离要求不得错误拒绝主机后台域。
        ai_computer_toolkit::policy::validate(&request)?;
        // 严格零打扰模式必须拒绝主机后台域并公开要求域证据。
        request.isolation_requirement = IsolationRequirement::Strict;
        // 冻结严格执行计划而不触碰真实 provider。
        let error = match ai_computer_toolkit::policy::validate(&request) {
            // 意外成功表示主机后台域被错误认证为严格零打扰。
            Ok(()) => return Err(std::io::Error::other("进程终止不得绕过严格隔离门禁").into()),
            // 保存结构化隔离错误。
            Err(error) => error,
        };
        // 主机后台 mutation 必须返回稳定隔离错误。
        assert_eq!(error.code, "ISOLATION_REQUIRED");
        // 错误证据必须来自 registry 中冻结的真实要求域。
        assert_eq!(error.details["requiredExecutionRealm"], "host-background");
    }
    // 两个独立 capability 都满足策略不变量。
    Ok(())
}

// 验证生产 System 与 facade 把 canonical 过期进程交给领域 Module 分类。
#[test]
fn production_route_preserves_module_stale_semantics() -> Result<(), Box<dyn std::error::Error>> {
    // 逐一覆盖两个独占终止 capability 的 stale 旁路。
    for capability in TERMINATION_CAPABILITIES {
        // 构造完整生产服务请求。
        let mut request = termination_request(capability);
        // 满足逐操作确认门禁。
        request.confirmed = true;
        // 通过 ComputerControlSystem 与生产 provider 链执行。
        let error = match AppControlService::new().execute(request) {
            // 过期目标不得成功。
            Ok(_) => return Err(std::io::Error::other("过期进程目标不得终止任何进程").into()),
            // 保存领域错误。
            Err(error) => error,
        };
        // facade 不得提前改写为 TARGET_NOT_FOUND。
        assert_eq!(error.code, "STALE_SESSION");
        // 错误消息不得泄漏原生身份。
        assert!(!error.message.to_ascii_lowercase().contains("pid"));
    }
    // 两个 capability 都保留 Module 权威 stale 语义。
    Ok(())
}

// 验证唯一生产 launcher 对两个 capability 都固定进入 Rust 领域 Module。
#[test]
fn production_launcher_routes_both_capabilities_to_rust_module()
-> Result<(), Box<dyn std::error::Error>> {
    // 逐夹具覆盖优雅与强制终止。
    for fixture in [
        // 使用优雅终止 stale 请求。
        "process-termination-graceful-stale.json",
        // 使用强制终止 stale 请求。
        "process-termination-force-stale.json",
    ] {
        // 经生产 launcher 执行固定请求。
        let result = launcher_stale_request(fixture)?;
        // launcher 必须保持结构化失败 envelope。
        assert_eq!(result["ok"], false);
        // 两个风险都必须到达 Process Lifecycle Module 的权威 stale 分类。
        assert_eq!(result["error"]["code"], "STALE_SESSION");
    }
    // 两个正式 launcher 路由均已验证。
    Ok(())
}
