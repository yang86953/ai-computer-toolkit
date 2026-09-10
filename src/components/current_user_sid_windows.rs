//! 读取当前 Windows 进程 token 的 canonical 用户 SID 文本。

// 导入结构尺寸计算。
use std::mem::size_of;

// 导入最小 Windows token、SID 转换与配对释放接口。
use windows::{
    // 只在当前 Component 内使用原生类型。
    Win32::{
        // 导入 handle 与 LocalAlloc 配对回收函数。
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        // 导入 token 与 SID 接口。
        Security::{
            // 导入 canonical SID 文本转换。
            Authorization::ConvertSidToStringSidW,
            // 导入 token user 查询类型。
            GetTokenInformation,
            TOKEN_QUERY,
            TOKEN_USER,
            TokenUser,
        },
        // 导入当前进程与 token 打开接口。
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
    // 导入 LocalAlloc 宽字符串指针。
    core::PWSTR,
};

// 限制异常 token identity 缓冲区大小。
const MAXIMUM_TOKEN_IDENTITY_BYTES: u32 = 64 * 1024;

// 表示当前用户 SID 无法安全取得。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CurrentUserSidError {
    // token、SID 或 canonical 文本不可用。
    Unavailable,
}

// 让有效 token handle 在全部返回路径关闭。
struct OwnedHandle(HANDLE);

// 在作用域结束时回收 token handle。
impl Drop for OwnedHandle {
    // 调用配对关闭函数。
    fn drop(&mut self) {
        // SID 结论不依赖关闭诊断。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 使用 usize 存储保证 TOKEN_USER 结构对齐。
struct TokenUserBuffer {
    // 保存 GetTokenInformation 写入的对齐存储。
    storage: Vec<usize>,
}

// 为 token user 缓冲区提供受生命周期保护的 SID。
impl TokenUserBuffer {
    // 返回只供 Windows SID 转换使用的私有指针。
    fn sid(&self) -> windows::Win32::Security::PSID {
        // 存储尺寸和对齐已在构造阶段验证。
        let token_user = unsafe {
            // 只在 self 生命周期内解释固定头部。
            &*self.storage.as_ptr().cast::<TOKEN_USER>()
        };
        // 返回尾随 SID 指针。
        token_user.User.Sid
    }
}

// 读取当前进程 token 的 canonical 用户 SID 文本。
pub(crate) fn current_user_sid_string() -> Result<String, CurrentUserSidError> {
    // 初始化 token handle。
    let mut token = HANDLE::default();
    // 只申请用户主体查询权限。
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        // 不公开 token 或系统错误。
        .map_err(|_| CurrentUserSidError::Unavailable)?;
    // 拒绝无效 handle。
    if token.is_invalid() {
        // 空 token 不能建立身份事实。
        return Err(CurrentUserSidError::Unavailable);
    }
    // 接管 token handle。
    let token = OwnedHandle(token);
    // 首次调用只查询所需字节数。
    let mut bytes_needed = 0_u32;
    // 缓冲区不足是预期结果，只使用返回长度。
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut bytes_needed) };
    // 要求至少容纳固定头部并限制异常长度。
    if bytes_needed < u32::try_from(size_of::<TOKEN_USER>()).unwrap_or(u32::MAX)
        // 防止异常 token 扩大内存。
        || bytes_needed > MAXIMUM_TOKEN_IDENTITY_BYTES
    {
        // 返回封闭身份错误。
        return Err(CurrentUserSidError::Unavailable);
    }
    // 取得平台字长用于对齐向上取整。
    let word_size = size_of::<usize>();
    // 计算所需 usize 元素数量。
    let words = usize::try_from(bytes_needed)
        // 加入向上取整余量。
        .ok()
        // 防止加法溢出。
        .and_then(|bytes| bytes.checked_add(word_size.saturating_sub(1)))
        // 转换为元素数量。
        .map(|bytes| bytes / word_size)
        // 任一溢出都失败闭合。
        .ok_or(CurrentUserSidError::Unavailable)?;
    // 分配清零且正确对齐的 token 存储。
    let mut storage = vec![0_usize; words];
    // 读取完整 TOKEN_USER 与尾随 SID。
    unsafe {
        GetTokenInformation(
            // 使用当前进程 token。
            token.0,
            // 只读取用户主体。
            TokenUser,
            // 写入对齐存储。
            Some(storage.as_mut_ptr().cast()),
            // 传入平台报告长度。
            bytes_needed,
            // 允许平台核对最终长度。
            &mut bytes_needed,
        )
    }
    // 不公开 SID 或系统错误。
    .map_err(|_| CurrentUserSidError::Unavailable)?;
    // 接管包含尾随 SID 的存储。
    let current_user = TokenUserBuffer { storage };
    // 初始化 LocalAlloc 字符串指针。
    let mut sid_text = PWSTR::null();
    // 让 Windows 生成 canonical SDDL SID 文本。
    unsafe { ConvertSidToStringSidW(current_user.sid(), &mut sid_text) }
        // 转换失败保持封闭。
        .map_err(|_| CurrentUserSidError::Unavailable)?;
    // 为 LocalAlloc 字符串建立配对回收。
    struct OwnedLocalString(PWSTR);
    // 在全部返回路径释放 SID 文本。
    impl Drop for OwnedLocalString {
        // 调用 LocalFree 配对回收。
        fn drop(&mut self) {
            // 文本已经复制后忽略释放诊断。
            let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0.cast()))) };
        }
    }
    // 立即接管平台指针。
    let sid_text = OwnedLocalString(sid_text);
    // 空指针不能形成 identity。
    if sid_text.0.0.is_null() {
        // 返回封闭身份错误。
        return Err(CurrentUserSidError::Unavailable);
    }
    // 在释放前复制 canonical UTF-16 文本。
    let value = unsafe { sid_text.0.to_string() }
        // 异常 UTF-16 不得离开 Component。
        .map_err(|_| CurrentUserSidError::Unavailable)?;
    // SID 必须使用无结构字符的 canonical 前缀。
    if value.is_empty()
        // 固定 Windows SID 文本前缀。
        || !value.starts_with("S-1-")
        // 禁止注入后续 SDDL 结构。
        || value.contains(['(', ')', ';'])
    {
        // 拒绝异常文本。
        return Err(CurrentUserSidError::Unavailable);
    }
    // 返回拥有型 canonical SID 文本。
    Ok(value)
}

// 声明当前用户身份边界测试。
#[cfg(test)]
mod tests {
    // 导入被测实现。
    use super::*;

    // 验证当前进程 token 可产生不含 SDDL 结构字符的 SID。
    #[test]
    fn current_process_sid_is_canonical() {
        // 当前测试进程必须能查询自身 token。
        let sid = current_user_sid_string()
            // 失败时只报告封闭错误。
            .unwrap_or_else(|error| panic!("current SID unavailable: {error:?}"));
        // canonical SID 使用固定前缀。
        assert!(sid.starts_with("S-1-"));
        // 不允许注入 DACL 结构字符。
        assert!(!sid.contains(['(', ')', ';']));
    }
}
