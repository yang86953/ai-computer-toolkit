#![cfg(target_os = "windows")]

// 引入错误边界供测试辅助函数统一传播失败。
use std::error::Error;
// 引入路径类型以定位仓库契约与生产源码。
use std::path::{Path, PathBuf};

// 引入 JSON 值类型以读取版本化策略清单与公开 schema。
use serde_json::Value;

// 读取并解析一个仓库内 JSON 文件。
fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    // 读取完整文本并保留 I/O 错误上下文。
    let text = std::fs::read_to_string(path)?;
    // 解析 JSON 并把结构返回给断言层。
    Ok(serde_json::from_str(&text)?)
}

// 递归收集 Rust 与 C++ 生产源码文件。
fn collect_source_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    // 遍历当前目录的直接子项。
    for entry in std::fs::read_dir(directory)? {
        // 取得经过错误检查的绝对路径。
        let path = entry?.path();
        // 目录继续递归，文件进入扩展名筛选。
        if path.is_dir() {
            // 收集嵌套 Module、Component 与 Adapter 源码。
            collect_source_files(&path, files)?;
            // 当前目录项处理完毕。
            continue;
        }
        // 取得 UTF-8 扩展名供封闭白名单判断。
        let extension = path.extension().and_then(|value| value.to_str());
        // 只扫描项目使用的 Rust 与 C/C++ 源码扩展名。
        if matches!(
            extension,
            Some("rs" | "c" | "cc" | "cpp" | "cxx" | "h" | "hpp")
        ) {
            // 保存生产源码路径供统一策略扫描。
            files.push(path);
        }
    }
    // 所有目录项均已成功收集。
    Ok(())
}

// 去掉空白以避免格式变化绕过禁止调用检查。
fn without_whitespace(value: &str) -> String {
    // 过滤所有 Unicode 空白并保留符号顺序。
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

// 验证 WGC 生产路径保持系统默认隐私指示器行为。
#[test]
// 执行策略清单、公开 schema 与生产源码的联合门禁。
fn wgc_capture_never_requests_privacy_indicator_suppression() -> Result<(), Box<dyn Error>> {
    // 取得当前 Cargo 包根目录。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 加载版本化隐私指示器策略清单。
    let policy = read_json(&root.join("tests/contracts/capture-privacy-indicator-policy-v1.json"))?;
    // 契约版本必须保持稳定。
    assert_eq!(
        policy["contractVersion"],
        "act/capture-privacy-indicator-policy/v1"
    );
    // 系统指示器可能出现是公开事实。
    assert_eq!(policy["systemIndicatorMayAppear"], true);
    // 工具不得允许抑制系统边框。
    assert_eq!(policy["borderSuppressionAllowed"], false);
    // 真实可见性必须由人工验收。
    assert_eq!(policy["requiresHumanVisualAcceptance"], true);
    // 人工验收只能交给项目所有者账号。
    assert_eq!(policy["humanVisualAcceptance"]["assignee"], "project-owner");
    // AI 永远不得关闭视觉验收任务。
    assert_eq!(policy["humanVisualAcceptance"]["aiMayClose"], false);
    // 视觉门禁必须指向当前项目中的专属任务。
    assert_eq!(policy["humanVisualAcceptance"]["vikunjaProjectId"], 2);
    // 视觉门禁任务身份必须保持可追溯。
    assert_eq!(policy["humanVisualAcceptance"]["taskId"], 1650);

    // 加载截图兼容策略并核对禁止回退集合。
    let screenshot_policy = read_json(
        // 定位截图迁移期兼容策略清单。
        &root.join("tests/contracts/window-screenshot-compatibility-policy.json"),
    )?;
    // 截图路径必须显式禁止隐私指示器抑制。
    assert!(
        screenshot_policy["forbiddenFallbacks"]
            // 禁止回退字段必须保持为数组。
            .as_array()
            // 数组中必须保留稳定禁止项。
            .is_some_and(
                |items| items.contains(&Value::String("privacy-indicator-suppression".to_owned()))
            )
    );
    // 加载录制迁移策略以核对共享 WGC 不变量。
    let recording_policy =
        read_json(&root.join("tests/contracts/window-record-migration-policy.json"))?;
    // 录制结果必须继续报告系统指示器可能出现。
    assert_eq!(recording_policy["systemCaptureIndicatorMayAppear"], true);
    // 录制路径同样不得允许边框抑制。
    assert_eq!(recording_policy["borderSuppressionAllowed"], false);
    // 读取并核对每一项公开 schema 声明。
    for claim in policy["schemaClaims"]
        .as_array()
        .ok_or("schemaClaims must be an array")?
    {
        // 取得 schema 相对路径。
        let path = claim["path"]
            .as_str()
            .ok_or("schema claim path must be a string")?;
        // 取得需要保持为 true 的 JSON pointer。
        let pointer = claim["pointer"]
            .as_str()
            .ok_or("schema claim pointer must be a string")?;
        // 加载对应公开 schema。
        let schema = read_json(&root.join(path))?;
        // 公开结果必须继续声明系统指示器可能出现。
        assert_eq!(
            schema.pointer(pointer),
            Some(&Value::Bool(true)),
            "{path} lost {pointer}"
        );
    }

    // 核对两条实际 WGC 主路径仍然创建并启动 capture session。
    for relative in policy["activeWgcSources"]
        .as_array()
        .ok_or("activeWgcSources must be an array")?
    {
        // 读取源码相对路径。
        let relative = relative
            .as_str()
            .ok_or("active WGC source must be a string")?;
        // 读取主路径源码文本。
        let source = std::fs::read_to_string(root.join(relative))?;
        // 主路径必须真实创建 capture session。
        assert!(
            source.contains("CreateCaptureSession"),
            "{relative} does not create a capture session"
        );
        // 主路径必须真实启动 WGC 捕获。
        assert!(
            source.contains("StartCapture"),
            "{relative} does not start capture"
        );
    }

    // 收集策略声明的全部生产源码根。
    let mut sources = Vec::new();
    // 遍历 Rust 与 C++ 两个生产源码根。
    for relative in policy["productionSourceRoots"]
        .as_array()
        .ok_or("productionSourceRoots must be an array")?
    {
        // 取得受门禁保护的源码根路径。
        let relative = relative
            .as_str()
            .ok_or("production source root must be a string")?;
        // 递归加入所有匹配扩展名的源码。
        collect_source_files(&root.join(relative), &mut sources)?;
    }
    // 门禁必须实际覆盖非空生产源码集合。
    assert!(
        !sources.is_empty(),
        "production source scan must not be empty"
    );

    // 扫描每个生产源码文件中的禁止调用。
    for path in sources {
        // 读取源码并去掉空白，阻止换行规避。
        let source = without_whitespace(&std::fs::read_to_string(&path)?);
        // 遍历版本化策略列出的每个禁止符号。
        for token in policy["forbiddenSourceTokens"]
            .as_array()
            .ok_or("forbiddenSourceTokens must be an array")?
        {
            // 取得禁止符号文本。
            let token = token
                .as_str()
                .ok_or("forbidden source token must be a string")?;
            // 与源码使用相同的空白规范化。
            let normalized_token = without_whitespace(token);
            // 任何生产路径都不得请求无边框捕获或关闭系统边框。
            assert!(
                !source.contains(&normalized_token),
                "{} contains forbidden privacy-indicator suppression token {token}",
                path.display()
            );
        }
    }
    // 联合门禁全部通过。
    Ok(())
}
