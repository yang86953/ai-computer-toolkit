//! 验证结构化图像输出在 COM dispatch 前执行统一覆盖门禁。

// 导入文件系统、路径与无锁序列工具。
use std::{
    // 导入测试夹具文件操作。
    fs,
    // 导入测试夹具路径类型。
    path::PathBuf,
    // 导入并行测试安全的唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入被 save/export 共同调用的路径门禁。
use super::validate_output_path;

// 为并行测试生成无碰撞目录序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占并清理一次结构化输出测试目录。
struct Fixture {
    // 保存测试自有根目录。
    root: PathBuf,
}

// 创建唯一结构化输出测试目录。
impl Fixture {
    // 建立当前测试的自有目录。
    fn new(name: &str) -> std::io::Result<Self> {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录下构造工具自有路径。
        let root = std::env::temp_dir().join(format!(
            // 固定前缀并加入进程与序列。
            "ai-computer-toolkit-photoshop-output-{name}-{}-{sequence}",
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

// PSD 保存和 PNG 导出必须共享未确认覆盖错误。
#[test]
fn psd_and_png_require_overwrite_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("confirmation")?;
    // 逐一覆盖两个认证输出扩展名。
    for (name, extension) in [("document.psd", "psd"), ("image.png", "png")] {
        // 构造既有输出路径。
        let output = fixture.path(name);
        // 写入可验证的原始内容。
        fs::write(&output, b"original")?;
        // 转换为 provider 接受的 UTF-8 路径。
        let output_text = output.to_string_lossy();
        // 未确认覆盖必须返回稳定公开错误。
        let error = match validate_output_path(&output_text, extension, false) {
            // 意外成功转换为显式测试错误。
            Ok(()) => return Err(std::io::Error::other("既有结构化输出必须要求覆盖确认").into()),
            // 保存稳定领域错误。
            Err(error) => error,
        };
        // 核对公开覆盖确认错误码。
        assert_eq!(error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
        // 门禁不得修改原始内容。
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

// 明确许可只允许真实普通文件通过共同路径门禁。
#[test]
fn confirmed_regular_output_is_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("confirmed")?;
    // 构造既有 PNG 输出路径。
    let output = fixture.path("image.png");
    // 写入普通文件夹具。
    fs::write(&output, b"original")?;
    // 转换为 provider 接受的 UTF-8 路径。
    let output_text = output.to_string_lossy();
    // 明确覆盖许可允许后续固定 provider writer 执行。
    assert!(validate_output_path(&output_text, "png", true).is_ok());
    // 纯路径门禁不得自行修改文件。
    assert_eq!(
        // 回读原始文件。
        fs::read(&output)?,
        // 核对原始字节保持不变。
        b"original"
    );
    // 返回测试成功。
    Ok(())
}

// 覆盖许可不得把目录放宽为文件目标。
#[test]
fn confirmed_directory_target_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // 创建独立测试根目录。
    let fixture = Fixture::new("directory")?;
    // 构造带 PNG 扩展名的目录路径。
    let output = fixture.path("image.png");
    // 创建真实目录夹具。
    fs::create_dir(&output)?;
    // 转换为 provider 接受的 UTF-8 路径。
    let output_text = output.to_string_lossy();
    // 即使确认覆盖也必须拒绝非文件目标。
    let error = match validate_output_path(&output_text, "png", true) {
        // 意外成功转换为显式测试错误。
        Ok(()) => return Err(std::io::Error::other("目录不得作为结构化文件输出").into()),
        // 保存稳定领域错误。
        Err(error) => error,
    };
    // 核对稳定参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 返回测试成功。
    Ok(())
}
