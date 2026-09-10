#![cfg(target_os = "windows")]

//! 为生产 launcher 集成测试提供精确、handle-bound Windows 进程见证。

// 导入 Windows 路径、进程快照与有界轮询工具。
use std::{
    // 导入拥有型 Windows 字符串。
    ffi::OsString,
    // 导入镜像规范化。
    fs,
    // 导入 Windows 快照结构长度。
    mem::size_of,
    // 导入 UTF-16 路径转换。
    os::windows::ffi::OsStringExt,
    // 导入精确镜像路径。
    path::{Path, PathBuf},
    // 导入短轮询休眠。
    thread,
    // 导入总预算与等待时长。
    time::{Duration, Instant},
};

// 导入只用于测试见证的 Windows API。
use windows::{
    // 导入进程观察与终止接口。
    Win32::{
        // 导入句柄与等待常量。
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        // 导入进程快照与内核对象接口。
        System::{
            // 导入只读进程快照接口。
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入精确镜像查询、等待与终止接口。
            Threading::{
                GetProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS, QueryFullProcessImageNameW,
                TerminateProcess, WaitForSingleObject,
            },
        },
    },
    // 导入可写 UTF-16 缓冲区包装。
    core::PWSTR,
};

// 固定精确镜像查询与同步等待权限。
const PROCESS_QUERY_AND_SYNC: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_1000);
// 固定测试拥有的 broker 终止权限。
const PROCESS_TERMINATE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0000_0001);
// 固定 Windows 完整镜像路径容量。
const MAXIMUM_IMAGE_PATH_UNITS: usize = 32_768;

// 保存一个精确镜像绑定的测试进程对象。
pub(super) struct ProcessWitness(HANDLE);

// 为测试进程对象提供生命周期操作。
impl ProcessWitness {
    // 接管有效 Windows handle。
    fn new(handle: HANDLE) -> Option<Self> {
        // 无效 handle 不建立见证。
        (!handle.is_invalid()).then_some(Self(handle))
    }

    // 返回仅供测试 API 使用的原始 handle。
    const fn raw(&self) -> HANDLE {
        // 不把 handle 投影到 JSON。
        self.0
    }

    // 等待同一内核对象退出。
    pub(super) fn wait_exited(&self, timeout: Duration) -> bool {
        // 将固定预算转换为 Windows 毫秒。
        let timeout_ms = u32::try_from(timeout.as_millis())
            // 测试预算固定适合 u32。
            .unwrap_or_else(|error| panic!("process wait timeout should fit u32: {error}"));
        // 只等待已经绑定的对象。
        (unsafe { WaitForSingleObject(self.raw(), timeout_ms) }) == WAIT_OBJECT_0
    }

    // 终止本测试隔离目录中的 broker。
    pub(super) fn terminate_owned(&self) {
        // 使用固定退出码模拟 broker 崩溃。
        unsafe { TerminateProcess(self.raw(), 2) }
            // 精确对象必须可被当前测试回收。
            .unwrap_or_else(|error| panic!("owned broker should terminate: {error:?}"));
        // broker 必须在有界时间退出。
        assert!(
            self.wait_exited(Duration::from_secs(5)),
            "owned broker should exit after termination"
        );
    }
}

// 确保测试进程 handle 不泄漏。
impl Drop for ProcessWitness {
    // 关闭当前唯一 handle。
    fn drop(&mut self) {
        // 关闭失败不改变已经建立的进程事实。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 从已打开进程对象读取完整镜像路径。
fn process_image(process: HANDLE) -> Option<PathBuf> {
    // 分配 Windows 文档允许的固定最大路径缓冲区。
    let mut buffer = vec![0_u16; MAXIMUM_IMAGE_PATH_UNITS];
    // 传入容量并接收实际 UTF-16 长度。
    let mut length = u32::try_from(buffer.len()).ok()?;
    // 查询同一内核对象的默认 DOS 路径。
    unsafe {
        QueryFullProcessImageNameW(
            // 使用 handle-bound 对象。
            process,
            // 保持默认路径格式。
            Default::default(),
            // 提供可写缓冲区。
            PWSTR(buffer.as_mut_ptr()),
            // 提供并接收长度。
            &mut length,
        )
    }
    // 进程退出或无权限时拒绝候选。
    .ok()?;
    // 转换平台返回长度。
    let length = usize::try_from(length).ok()?;
    // 拒绝空路径与越界长度。
    if length == 0 || length > buffer.len() {
        // 不使用不可信镜像路径。
        return None;
    }
    // 返回拥有型 Windows 路径。
    Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

// 枚举当前仍 live 且完整镜像匹配的进程。
pub(super) fn matching_processes(expected: &Path, terminate: bool) -> Vec<ProcessWitness> {
    // 规范化测试自行复制的精确镜像。
    let expected = fs::canonicalize(expected)
        // 测试镜像必须存在。
        .unwrap_or_else(|error| panic!("expected process image should exist: {error:?}"));
    // 创建当前 Windows 进程快照。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        // 当前环境必须允许读取进程清单。
        .unwrap_or_else(|error| panic!("process snapshot should be available: {error:?}"));
    // 接管快照 handle。
    let snapshot = ProcessWitness::new(snapshot)
        // 无效快照不能证明资源回收。
        .unwrap_or_else(|| panic!("process snapshot handle should be valid"));
    // 初始化 Windows 要求的结构长度。
    let mut entry = PROCESSENTRY32W {
        // 写入精确结构大小。
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            // 平台结构长度必须适合 u32。
            .unwrap_or_else(|error| panic!("process entry size should fit u32: {error}")),
        // 其余字段保持平台零值。
        ..Default::default()
    };
    // 当前进程快照必须至少包含测试进程。
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        // 无法读取首项不能证明资源回收。
        .unwrap_or_else(|error| panic!("process snapshot should contain entries: {error:?}"));
    // 保存全部精确匹配。
    let mut matches = Vec::new();
    // 遍历完整快照。
    loop {
        // 按场景组合最小权限。
        let access = if terminate {
            // broker 见证需要额外终止权限。
            PROCESS_QUERY_AND_SYNC | PROCESS_TERMINATE
        } else {
            // worker/runtime 只需要查询与等待。
            PROCESS_QUERY_AND_SYNC
        };
        // 尝试打开快照候选。
        if let Ok(handle) = unsafe { OpenProcess(access, false, entry.th32ProcessID) }
            // 接管有效 handle。
            && let Some(handle) = ProcessWitness::new(handle)
            // 拒绝快照后的 PID 复用。
            && unsafe { GetProcessId(handle.raw()) } == entry.th32ProcessID
            // 只接受仍 live 的对象。
            && unsafe { WaitForSingleObject(handle.raw(), 0) } == WAIT_TIMEOUT
            // 读取同一对象的完整镜像。
            && let Some(image) = process_image(handle.raw())
            // 规范化仍存在的镜像。
            && let Ok(image) = fs::canonicalize(image)
            // Windows 路径比较忽略大小写。
            && image
                // 转换为文本视图。
                .to_string_lossy()
                // 与精确测试镜像比较。
                .eq_ignore_ascii_case(&expected.to_string_lossy())
        {
            // 保存 handle-bound 见证。
            matches.push(handle);
        }
        // 推进到下一条快照记录。
        if let Err(error) = unsafe { Process32NextW(snapshot.raw(), &mut entry) } {
            // 只允许自然无更多记录结束。
            assert_eq!(
                error.code(),
                ERROR_NO_MORE_FILES.to_hresult(),
                "process snapshot ended unexpectedly"
            );
            // 完整枚举结束。
            break;
        }
    }
    // 返回不公开 PID 的精确对象集合。
    matches
}

// 在总预算内等待唯一精确进程出现。
pub(super) fn wait_for_process(expected: &Path, terminate: bool) -> ProcessWitness {
    // 建立不会因轮询重置的 deadline。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询精确镜像。
    loop {
        // 读取当前精确匹配。
        let mut matches = matching_processes(expected, terminate);
        // 同一路径不允许出现多个实例。
        assert!(
            matches.len() <= 1,
            "fixed process image should have one live instance"
        );
        // 唯一匹配建立进程见证。
        if let Some(process) = matches.pop() {
            // 返回持有型内核对象。
            return process;
        }
        // 超时表示进程未按契约启动。
        assert!(
            Instant::now() < deadline,
            "fixed process image did not become live"
        );
        // 限制快照轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}

// 在总预算内等待精确进程全部退出。
pub(super) fn wait_for_no_process(expected: &Path) {
    // 建立固定回收预算。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询至零匹配。
    loop {
        // 无匹配表示精确镜像已经回收。
        if matching_processes(expected, false).is_empty() {
            // 完成回收证明。
            return;
        }
        // 超时不能伪造完成。
        assert!(
            Instant::now() < deadline,
            "fixed process image remained live"
        );
        // 限制快照轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}
