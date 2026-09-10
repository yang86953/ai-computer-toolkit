#![cfg(target_os = "windows")]

//! 验证受保护上下文门禁的静态策略与生产 launcher 非输入桌面路线。

// 导入 Windows 宽字符串转换和测试文件工具。
use std::{
    // 读取环境、时间与仓库路径。
    env,
    // 使用 Windows 原生宽字符串编码。
    ffi::OsStr,
    // 创建并读取工具自有临时夹具。
    fs,
    // 使用 Windows OsStr 扩展。
    os::windows::ffi::OsStrExt,
    // 保存拥有型路径。
    path::{Path, PathBuf},
    // 取得唯一测试进程 ID。
    process,
    // 生成本次测试唯一目录后缀。
    time::{SystemTime, UNIX_EPOCH},
};

// 导入 JSON 值以核对生产错误 envelope。
use serde_json::Value;
// 导入低层 Windows 测试夹具 API。
use windows::{
    // 使用桌面、进程和 handle 生命周期 API。
    Win32::{
        // 关闭 handle 并核对等待结果。
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        // 创建工具自有非输入桌面与生产 launcher 进程。
        System::{
            // 创建与关闭测试桌面。
            StationsAndDesktops::{CloseDesktop, CreateDesktopW, DESKTOP_CONTROL_FLAGS, HDESK},
            // 在测试桌面启动生产 PowerShell launcher 并有界等待。
            Threading::{
                CREATE_NO_WINDOW, CreateProcessW, GetExitCodeProcess, PROCESS_INFORMATION,
                STARTUPINFOW, TerminateProcess, WaitForSingleObject,
            },
        },
    },
    // 构造只在测试进程内存在的宽字符串指针。
    core::{PCWSTR, PWSTR},
};

// 固定生产 launcher 子进程最大等待时间。
const LAUNCHER_TIMEOUT_MS: u32 = 60_000;

// 把 Windows 路径编码为带终止符的 UTF-16。
fn wide(value: &OsStr) -> Vec<u16> {
    // 收集平台宽字符并追加唯一终止符。
    value.encode_wide().chain(Some(0)).collect()
}

// 把普通字符串编码为带终止符的 UTF-16。
fn wide_text(value: &str) -> Vec<u16> {
    // 复用 OsStr 宽字符边界。
    wide(OsStr::new(value))
}

// 为 PowerShell 单引号字符串执行固定转义。
fn powershell_literal(path: &Path) -> String {
    // 双写单引号并保持其他路径字符原样。
    path.to_string_lossy().replace('\'', "''")
}

// 拥有经过父目录核对的工具自有临时目录。
struct TempFixture {
    // 保存唯一目录路径。
    path: PathBuf,
}

// 为测试临时目录提供创建与安全清理。
impl TempFixture {
    // 创建位于系统临时根目录下的唯一夹具。
    fn create() -> Result<Self, Box<dyn std::error::Error>> {
        // 解析系统临时根目录。
        let temporary_root = env::temp_dir().canonicalize()?;
        // 取得单调性不要求的唯一时间后缀。
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        // 构造带固定安全前缀的精确子目录。
        let candidate = temporary_root.join(format!(
            // 使用项目专属前缀、进程 ID 与时间值。
            "act-security-boundary-{}-{nonce}",
            process::id()
        ));
        // 创建唯一空目录。
        fs::create_dir(&candidate)?;
        // 解析刚创建的实际路径。
        let resolved = candidate.canonicalize()?;
        // 防御性要求父目录就是已解析临时根。
        if resolved.parent() != Some(temporary_root.as_path()) {
            // 拒绝清理范围不明确的路径。
            return Err(std::io::Error::other("security fixture escaped the temp root").into());
        }
        // 返回经过核对的所有权对象。
        Ok(Self { path: resolved })
    }
}

// 在测试结束时清理精确工具自有目录。
impl Drop for TempFixture {
    // 删除测试生成的 wrapper 与输出文件。
    fn drop(&mut self) {
        // 只递归删除构造时已核对的唯一子目录。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 拥有工具自有非输入桌面 handle。
struct OwnedDesktop(HDESK);

// 保证测试桌面在子进程退出后关闭。
impl Drop for OwnedDesktop {
    // 释放唯一测试桌面 handle。
    fn drop(&mut self) {
        // 忽略测试清理阶段的关闭错误。
        let _ = unsafe { CloseDesktop(self.0) };
    }
}

// 拥有生产 launcher 的进程与主线程 handle。
struct OwnedChild {
    // 保存进程 handle 供等待与兜底回收。
    process: HANDLE,
    // 保存主线程 handle 供配对关闭。
    thread: HANDLE,
    // 标记正常有界等待是否完成。
    completed: bool,
}

// 为工具自有子进程提供有界等待。
impl OwnedChild {
    // 等待生产 launcher 并返回退出码。
    fn wait(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
        // 在固定 deadline 内等待进程结束。
        let wait = unsafe { WaitForSingleObject(self.process, LAUNCHER_TIMEOUT_MS) };
        // 正常退出是唯一成功等待结果。
        if wait == WAIT_OBJECT_0 {
            // 标记 Drop 不需要终止。
            self.completed = true;
            // 初始化退出码。
            let mut exit_code = u32::MAX;
            // 读取生产 launcher 退出状态。
            unsafe { GetExitCodeProcess(self.process, &mut exit_code) }?;
            // 返回实际退出码。
            return Ok(exit_code);
        }
        // 超时必须回收精确工具自有子进程。
        if wait == WAIT_TIMEOUT {
            // 只终止本测试刚创建且仍持有 handle 的进程。
            unsafe { TerminateProcess(self.process, 124) }?;
            // 等待终止完成，避免孤儿 launcher。
            let _ = unsafe { WaitForSingleObject(self.process, 5_000) };
            // 标记已执行回收。
            self.completed = true;
            // 返回明确测试超时。
            return Err(std::io::Error::new(
                // 使用超时错误类别。
                std::io::ErrorKind::TimedOut,
                // 不公开任何原生 handle。
                "security fixture launcher timed out",
            )
            // 转换到测试结果类型。
            .into());
        }
        // 其他等待结果是平台失败。
        Err(std::io::Error::other("security fixture launcher wait failed").into())
    }
}

// 保证测试失败或 panic 时也不遗留工具自有子进程。
impl Drop for OwnedChild {
    // 回收子进程并关闭两个 handle。
    fn drop(&mut self) {
        // 仅在尚未完成时终止精确工具自有进程。
        if !self.completed && unsafe { WaitForSingleObject(self.process, 0) } == WAIT_TIMEOUT {
            // 使用测试专属退出码执行兜底回收。
            let _ = unsafe { TerminateProcess(self.process, 125) };
            // 有界等待回收完成。
            let _ = unsafe { WaitForSingleObject(self.process, 5_000) };
        }
        // 关闭主线程 handle。
        let _ = unsafe { CloseHandle(self.thread) };
        // 关闭进程 handle。
        let _ = unsafe { CloseHandle(self.process) };
    }
}

// 在指定工具自有桌面启动 PowerShell wrapper。
fn spawn_on_desktop(
    // 接收完整桌面路径。
    desktop_path: &str,
    // 接收临时 wrapper 路径。
    wrapper: &Path,
    // 接收生产工作目录。
    current_directory: &Path,
) -> Result<OwnedChild, Box<dyn std::error::Error>> {
    // 从系统根目录定位固定 Windows PowerShell。
    let powershell = PathBuf::from(env::var_os("SystemRoot").ok_or_else(|| {
        // 缺失系统根目录时返回明确夹具错误。
        std::io::Error::new(std::io::ErrorKind::NotFound, "SystemRoot is unavailable")
    })?)
    // 进入系统 PowerShell 目录。
    .join("System32")
    // 进入 WindowsPowerShell 产品目录。
    .join("WindowsPowerShell")
    // 使用版本一目录。
    .join("v1.0")
    // 选择固定 powershell.exe。
    .join("powershell.exe");
    // 构造明确 application path。
    let application = wide(powershell.as_os_str());
    // 构造可变 Windows 命令行。
    let mut command_line = wide_text(&format!(
        // 只运行测试生成的固定 wrapper。
        "\"{}\" -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        powershell.display(),
        wrapper.display()
    ));
    // 构造完整桌面路径。
    let mut desktop = wide_text(desktop_path);
    // 构造生产仓库工作目录。
    let directory = wide(current_directory.as_os_str());
    // 初始化 Windows startup 结构。
    let startup = STARTUPINFOW {
        // 设置结构字节数。
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>())?,
        // 将子进程主线程绑定到工具自有非输入桌面。
        lpDesktop: PWSTR(desktop.as_mut_ptr()),
        // 其他字段使用零值默认。
        ..Default::default()
    };
    // 初始化进程结果结构。
    let mut information = PROCESS_INFORMATION::default();
    // 创建不显示窗口且不继承 handle 的生产 launcher 进程。
    unsafe {
        CreateProcessW(
            // 固定明确 PowerShell application。
            PCWSTR(application.as_ptr()),
            // Windows API 允许原地解析可变命令行。
            Some(PWSTR(command_line.as_mut_ptr())),
            // 不提供进程安全描述符。
            None,
            // 不提供线程安全描述符。
            None,
            // 禁止 handle 继承。
            false,
            // 禁止可见控制台窗口。
            CREATE_NO_WINDOW,
            // 继承普通环境块。
            None,
            // 使用仓库根目录。
            PCWSTR(directory.as_ptr()),
            // 传入测试桌面 startup 信息。
            &startup,
            // 接收唯一子进程 handle。
            &mut information,
        )
    }?;
    // 返回拥有型子进程生命周期。
    Ok(OwnedChild {
        // 保存进程 handle。
        process: information.hProcess,
        // 保存主线程 handle。
        thread: information.hThread,
        // 初始尚未完成等待。
        completed: false,
    })
}

// 递归读取全部 Rust 源文件供静态绕过扫描。
fn rust_sources(root: &Path) -> Result<Vec<(PathBuf, String)>, Box<dyn std::error::Error>> {
    // 初始化待扫描目录。
    let mut pending = vec![root.to_path_buf()];
    // 初始化源文件集合。
    let mut sources = Vec::new();
    // 逐目录扫描。
    while let Some(directory) = pending.pop() {
        // 枚举当前目录。
        for entry in fs::read_dir(directory)? {
            // 取得实际路径。
            let path = entry?.path();
            // 子目录加入待处理栈。
            if path.is_dir() {
                // 保存子目录。
                pending.push(path);
                // 跳过文件读取。
                continue;
            }
            // 只读取 Rust 源文件。
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                // 跳过其他制品。
                continue;
            }
            // 读取 UTF-8 源码。
            let source = fs::read_to_string(&path)?;
            // 保存路径和文本供断言报告。
            sources.push((path, source));
        }
    }
    // 返回完整源码集合。
    Ok(sources)
}

// 验证安全策略禁止新增提权、注入或全局 hook 路线。
#[test]
fn security_policy_prevents_bypass_sources_and_public_native_facts()
-> Result<(), Box<dyn std::error::Error>> {
    // 定位仓库根目录。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 读取版本化静态策略。
    let policy: Value = serde_json::from_str(&fs::read_to_string(
        // 定位策略夹具。
        root.join("tests")
            // 进入契约目录。
            .join("contracts")
            // 选择安全边界策略。
            .join("security-boundary-policy-v1.json"),
    )?)?;
    // 读取策略声明的禁止源码 token。
    let forbidden = policy["forbiddenSourceTokens"]
        // 要求数组形状。
        .as_array()
        // 缺失时返回契约错误。
        .ok_or_else(|| std::io::Error::other("forbiddenSourceTokens must be an array"))?;
    // 读取完整生产 Rust 源码。
    let sources = rust_sources(&root.join("src"))?;
    // 逐禁止 token 扫描全部生产源码。
    for token in forbidden {
        // 要求每项为字符串。
        let token = token
            // 取得 token 文本。
            .as_str()
            // 非字符串表示契约损坏。
            .ok_or_else(|| std::io::Error::other("forbidden source token must be a string"))?;
        // 逐生产源文件核对零命中。
        for (path, source) in &sources {
            // 报告精确文件与禁止符号。
            assert!(
                !source.contains(token),
                "{} must not contain security bypass token {token}",
                path.display()
            );
        }
    }
    // 读取策略声明的生产边界与必需 token。
    let assertions = policy["sourceAssertions"]
        // 要求数组形状。
        .as_array()
        // 缺失时返回契约错误。
        .ok_or_else(|| std::io::Error::other("sourceAssertions must be an array"))?;
    // 逐生产边界核对契约 token。
    for assertion in assertions {
        // 读取仓库相对路径。
        let relative_path = assertion["path"]
            // 只接受字符串。
            .as_str()
            // 非字符串表示契约损坏。
            .ok_or_else(|| std::io::Error::other("source assertion path must be a string"))?;
        // 读取指定生产源码。
        let source = fs::read_to_string(root.join(relative_path))?;
        // 读取必需 token 数组。
        let required = assertion["requiredTokens"]
            // 要求数组形状。
            .as_array()
            // 缺失时返回契约错误。
            .ok_or_else(|| std::io::Error::other("requiredTokens must be an array"))?;
        // 逐 token 核对生产边界。
        for token in required {
            // 只接受字符串 token。
            let token = token
                // 读取文本。
                .as_str()
                // 非字符串表示契约损坏。
                .ok_or_else(|| std::io::Error::other("required token must be a string"))?;
            // 缺失时报告精确路径与 token。
            assert!(
                source.contains(token),
                "{relative_path} must contain security boundary token {token}"
            );
        }
    }
    // 读取通用目标 mutation 的完整权限路线表。
    let mutation_routes = policy["targetMutationPermissionRoutes"]
        // 要求数组形状。
        .as_array()
        // 缺失时返回契约错误。
        .ok_or_else(|| {
            // 使用稳定契约失败消息。
            std::io::Error::other("targetMutationPermissionRoutes must be an array")
        })?;
    // 逐通用 mutation Module 核对静态权限入口。
    for route in mutation_routes {
        // 读取 Module 仓库相对路径。
        let relative_path = route["path"]
            // 只接受字符串。
            .as_str()
            // 非字符串表示契约损坏。
            .ok_or_else(|| std::io::Error::other("mutation route path must be a string"))?;
        // 读取权限入口 token。
        let required_token = route["requiredToken"]
            // 只接受字符串。
            .as_str()
            // 非字符串表示契约损坏。
            .ok_or_else(|| std::io::Error::other("permission token must be a string"))?;
        // 读取领域 Module 源码。
        let source = fs::read_to_string(root.join(relative_path))?;
        // 每条通用控制路线必须明确消费静态权限结论。
        assert!(
            source.contains(required_token),
            "{relative_path} must enforce target permission through {required_token}"
        );
        // capability 列表必须非空，避免空路线制造假覆盖。
        assert!(
            route["capabilities"]
                // 要求数组。
                .as_array()
                // 只接受至少一个 capability。
                .is_some_and(|capabilities| !capabilities.is_empty()),
            "{relative_path} must own at least one capability"
        );
    }
    // 读取生产平台 Adapter。
    let adapter = fs::read_to_string(root.join("src/adapters/security_context_windows.rs"))?;
    // 私有只读 Adapter 不得切换桌面。
    assert!(!adapter.contains("SwitchDesktop"));
    // 私有只读 Adapter 不得改变线程桌面。
    assert!(!adapter.contains("SetThreadDesktop"));
    // Adapter 必须只读取输入桌面。
    assert!(adapter.contains("OpenInputDesktop"));
    // 读取 System 源码验证顺序。
    let service = fs::read_to_string(root.join("src/service.rs"))?;
    // 定位通用 execute 编排。
    let execute = service
        // 查找唯一生产方法。
        .find("pub fn execute(&self, request: CommandRequest)")
        // 缺失表示生产路线漂移。
        .ok_or_else(|| std::io::Error::other("production execute route is missing"))?;
    // 截取通用 execute 后续源码。
    let execute_source = &service[execute..];
    // 安全授权必须存在于 provider registry 解析之前。
    assert!(
        execute_source
            .find("authorize_target_access")
            .ok_or_else(|| std::io::Error::other("security gate is missing"))?
            < execute_source
                .find("self.registry.get")
                .ok_or_else(|| std::io::Error::other("provider resolution is missing"))?
    );
    // 静态策略边界验证完成。
    Ok(())
}

// 验证生产 launcher 在工具自有非输入桌面内失败闭合且不触碰目标。
#[test]
fn production_launcher_rejects_tool_owned_non_input_desktop_before_target_access()
-> Result<(), Box<dyn std::error::Error>> {
    // 定位生产仓库根目录。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 创建经过父目录核对的工具自有临时目录。
    let temporary = TempFixture::create()?;
    // 固定本次测试桌面名称。
    let desktop_name = format!("act-security-fixture-{}", process::id());
    // 构造创建桌面所需宽字符串。
    let desktop_name_wide = wide_text(&desktop_name);
    // 创建不会成为输入桌面的工具自有桌面。
    let desktop = OwnedDesktop(unsafe {
        CreateDesktopW(
            // 使用唯一测试桌面名称。
            PCWSTR(desktop_name_wide.as_ptr()),
            // 不指定显示设备。
            PCWSTR::null(),
            // 不指定 DEVMODE。
            None,
            // 不请求 hook 或切换标志。
            DESKTOP_CONTROL_FLAGS(0),
            // 测试所有者需要创建子进程并在结束后关闭桌面。
            GENERIC_ALL.0,
            // 不提供自定义安全描述符。
            None,
        )?
    });
    // 定位唯一生产 launcher。
    let launcher = root.join("tools/windows/Invoke-ComputerControl.ps1");
    // 复用固定 stale 进程请求，证明安全门禁早于目标重解析。
    let input = root.join("tests/fixtures/process-termination-force-stale.json");
    // 固定临时 stdout JSON 文件。
    let output = temporary.path.join("result.json");
    // 固定临时 PowerShell wrapper。
    let wrapper = temporary.path.join("invoke.ps1");
    // 构造不包含原生桌面事实的 wrapper 内容。
    let script = format!(
        // 捕获 launcher stdout，保留退出码并写无 BOM UTF-8。
        "$lines = & '{}' run app close --input '{}' 2>$null\n$code = $LASTEXITCODE\n[System.IO.File]::WriteAllText('{}', ($lines -join [Environment]::NewLine), [System.Text.UTF8Encoding]::new($false))\nexit $code\n",
        powershell_literal(&launcher),
        powershell_literal(&input),
        powershell_literal(&output),
    );
    // 写入工具自有临时 wrapper。
    fs::write(&wrapper, script)?;
    // 使用完整 window-station 与桌面路径启动生产 launcher。
    let mut child = spawn_on_desktop(
        // 指定当前 window station 内的工具自有桌面。
        &format!("winsta0\\{desktop_name}"),
        // 传入固定 wrapper。
        &wrapper,
        // 传入仓库根目录。
        &root,
    )?;
    // 有界等待生产 launcher。
    let exit_code = child.wait()?;
    // 安全失败必须返回非零 launcher 状态。
    assert_ne!(exit_code, 0);
    // 读取唯一 JSON stdout 文件。
    let result: Value = serde_json::from_str(&fs::read_to_string(&output)?)?;
    // 核对结构化失败 envelope。
    assert_eq!(result["ok"], false);
    // 非输入桌面必须返回稳定权限拒绝。
    assert_eq!(result["error"]["code"], "PERMISSION_DENIED");
    // 不公开测试桌面名称，只输出稳定分类。
    assert_eq!(
        result["error"]["details"]["reason"],
        "protected-or-non-input-desktop"
    );
    // 证明 System 尚未读取 stale 目标。
    assert_eq!(result["error"]["details"]["targetReadAttempted"], false);
    // 证明 System 尚未写入任何目标。
    assert_eq!(result["error"]["details"]["targetWriteAttempted"], false);
    // 证明没有尝试提权。
    assert_eq!(result["error"]["details"]["elevationAttempted"], false);
    // 证明没有尝试注入或备用路线。
    assert_eq!(result["error"]["details"]["injectionAttempted"], false);
    // 证明没有 fallback。
    assert_eq!(result["error"]["details"]["fallbackAttempted"], false);
    // 序列化公开错误用于隐私扫描。
    let serialized = result.to_string();
    // 不得泄漏真实测试桌面名称。
    assert!(!serialized.contains(&desktop_name));
    // 不得泄漏 SID。
    assert!(!serialized.contains("S-1-"));
    // 不得泄漏 token 字段。
    assert!(!serialized.to_ascii_lowercase().contains("token"));
    // 显式保持桌面所有权直到子进程完成。
    drop(desktop);
    // 生产 launcher 安全门禁验证完成。
    Ok(())
}
