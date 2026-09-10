//! 验证输出覆盖门禁不会修改任何测试夹具。

// 导入文件系统、路径与无锁序列工具。
use std::{
    // 导入测试夹具文件操作。
    fs,
    // 导入测试夹具路径类型。
    path::{Path, PathBuf},
    // 导入并行测试安全的唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入被测封闭门禁接口。
use super::{
    // 导入目录状态证据。
    OutputDirectoryState,
    // 导入封闭错误集合。
    OutputGuardError,
    // 导入目录门禁。
    guard_directory_output,
    // 导入文件门禁。
    guard_file_output,
};

// 为并行测试生成无碰撞目录序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占并清理一次测试创建的根目录。
struct Fixture {
    // 保存测试自有根目录。
    root: PathBuf,
}

// 创建唯一测试根目录。
impl Fixture {
    // 建立当前测试的自有目录。
    fn new(name: &str) -> std::io::Result<Self> {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录下构造工具自有路径。
        let root = std::env::temp_dir().join(format!(
            // 固定前缀并加入进程与序列。
            "ai-computer-toolkit-output-guard-{name}-{}-{sequence}",
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

// 缺失文件目标应允许首次输出。
#[test]
fn missing_file_is_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("missing-file")?;
    // 构造尚不存在的输出路径。
    let output = fixture.path("output.png");
    // 缺失目标无需覆盖许可。
    assert_eq!(guard_file_output(&output, false), Ok(()));
    // 门禁本身不得创建文件。
    assert!(!output.exists());
    // 返回测试成功。
    Ok(())
}

// 未确认覆盖必须保留既有普通文件。
#[test]
fn unconfirmed_file_is_preserved() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("unconfirmed-file")?;
    // 构造既有输出路径。
    let output = fixture.path("output.png");
    // 写入可验证的原始内容。
    fs::write(&output, b"original")?;
    // 未确认覆盖必须返回独立门禁错误。
    assert_eq!(
        // 调用纯检查 Component。
        guard_file_output(&output, false),
        // 核对稳定封闭错误。
        Err(OutputGuardError::ConfirmationRequired)
    );
    // 门禁不得改变原始内容。
    assert_eq!(
        // 回读原始文件。
        fs::read(&output)?,
        // 核对原始字节保持不变。
        b"original"
    );
    // 返回测试成功。
    Ok(())
}

// 明确确认后只允许真实普通文件进入 writer。
#[test]
fn confirmed_regular_file_is_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("confirmed-file")?;
    // 构造既有输出路径。
    let output = fixture.path("output.mp4");
    // 写入普通文件夹具。
    fs::write(&output, b"original")?;
    // 明确覆盖许可允许后续 writer 处理。
    assert_eq!(guard_file_output(&output, true), Ok(()));
    // Component 仍不得自行修改文件。
    assert_eq!(
        // 回读原始文件。
        fs::read(&output)?,
        // 核对原始字节保持不变。
        b"original"
    );
    // 返回测试成功。
    Ok(())
}

// 目录不得被当作单文件覆盖目标。
#[test]
fn directory_is_rejected_as_file_even_when_confirmed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("directory-as-file")?;
    // 创建带文件扩展名的真实目录。
    let output = fixture.path("output.png");
    // 建立目录夹具。
    fs::create_dir(&output)?;
    // 覆盖许可不得放宽目标类型。
    assert_eq!(
        // 调用单文件门禁。
        guard_file_output(&output, true),
        // 核对类型错误。
        Err(OutputGuardError::InvalidTargetType)
    );
    // 返回测试成功。
    Ok(())
}

// 缺失与空分析目录都无需覆盖许可。
#[test]
fn missing_and_empty_directories_are_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("empty-directory")?;
    // 构造尚不存在的分析目录。
    let missing = fixture.path("missing.analysis");
    // 缺失目录必须返回明确状态。
    assert_eq!(
        // 调用目录门禁。
        guard_directory_output(&missing, false),
        // 核对缺失状态。
        Ok(OutputDirectoryState::Missing)
    );
    // 创建空分析目录。
    let empty = fixture.path("empty.analysis");
    // 建立空目录夹具。
    fs::create_dir(&empty)?;
    // 空目录无需覆盖许可。
    assert_eq!(
        // 调用目录门禁。
        guard_directory_output(&empty, false),
        // 核对空目录状态。
        Ok(OutputDirectoryState::Empty)
    );
    // 返回测试成功。
    Ok(())
}

// 非空分析目录必须取得独立覆盖许可。
#[test]
fn non_empty_directory_requires_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("non-empty-directory")?;
    // 构造分析目录。
    let analysis = fixture.path("output.analysis");
    // 建立分析目录夹具。
    fs::create_dir(&analysis)?;
    // 写入工具自有占位产物。
    fs::write(analysis.join("manifest.json"), b"fixture")?;
    // 未确认时必须拒绝。
    assert_eq!(
        // 调用目录门禁。
        guard_directory_output(&analysis, false),
        // 核对覆盖确认错误。
        Err(OutputGuardError::ConfirmationRequired)
    );
    // 明确确认后返回非空确认状态。
    assert_eq!(
        // 再次调用目录门禁。
        guard_directory_output(&analysis, true),
        // 核对确认状态。
        Ok(OutputDirectoryState::NonEmptyConfirmed)
    );
    // 返回测试成功。
    Ok(())
}

// 目标检查失败必须失败闭合。
#[test]
fn inspection_failure_is_closed() -> Result<(), Box<dyn std::error::Error>> {
    // 构造 Windows 文件系统 API 明确拒绝的含 NUL 路径。
    let invalid = Path::new("\0");
    // InvalidInput 检查失败必须显式关闭。
    assert_eq!(
        // 调用单文件门禁。
        guard_file_output(invalid, false),
        // 核对检查失败错误。
        Err(OutputGuardError::InspectionFailed)
    );
    // 返回测试成功。
    Ok(())
}
