//! 创建并验证仅 SYSTEM 与当前用户完全访问的 Windows 固定子目录。

// 导入结构尺寸、Windows 元数据、路径与字节比较工具。
use std::{
    // 导入文件系统目录验证。
    fs,
    // 导入安全属性结构尺寸。
    mem::size_of,
    // 导入 Windows 路径编码与文件属性。
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    // 导入真实路径类型。
    path::{Path, PathBuf},
    // 导入 DACL 字节比较切片。
    slice,
};

// 导入 Windows 目录、安全描述符与配对释放接口。
use windows::{
    // 只在当前 Component 内使用平台类型。
    Win32::{
        // 导入 LocalAlloc 描述符释放类型。
        Foundation::{HLOCAL, LocalFree},
        // 导入 DACL、owner 与安全属性接口。
        Security::{
            // 导入描述符字段查询与固定安全标志。
            ACL,
            // 导入 SDDL、文件安全查询与安装入口。
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION,
            EqualSid,
            GetSecurityDescriptorControl,
            GetSecurityDescriptorDacl,
            GetSecurityDescriptorOwner,
            OWNER_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR,
            PSID,
            SE_DACL_PROTECTED,
            SECURITY_ATTRIBUTES,
        },
        // 导入创建时安装安全属性的目录入口。
        Storage::FileSystem::CreateDirectoryW,
    },
    // 导入 BOOL 与宽字符串指针。
    core::{BOOL, PCWSTR},
};

// 导入当前用户 canonical SID 文本。
use super::current_user_sid_windows::current_user_sid_string;

// 固定 Windows reparse point 属性值。
const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;
// 限制固定私有目录单段名称长度。
const MAXIMUM_DIRECTORY_NAME_BYTES: usize = 64;

// 表示 owner-only 目录无法安全建立。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnerOnlyDirectoryError {
    // 父目录不存在、是 reparse 或不是普通目录。
    InvalidParent,
    // 子目录名称不是固定安全单段。
    InvalidName,
    // 当前主体或安全描述符不可用。
    SecurityUnavailable,
    // 目录无法创建或打开。
    DirectoryUnavailable,
    // 最终项目是链接、reparse 或非目录。
    DirectoryUntrusted,
    // 受保护 owner-only DACL 无法安装或回读。
    PermissionUnavailable,
}

// 让 LocalAlloc 安全描述符在全部返回路径释放。
struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

// 回收 SDDL 或安全查询返回的描述符。
impl Drop for OwnedSecurityDescriptor {
    // 调用 LocalFree 配对释放。
    fn drop(&mut self) {
        // 安全结论不依赖释放诊断。
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

// 为期望安全描述符提供 owner 与 DACL 投影。
impl OwnedSecurityDescriptor {
    // 取得 self 生命周期内有效的 owner 和 DACL。
    fn owner_and_dacl(&self) -> Result<(PSID, *const ACL), OwnerOnlyDirectoryError> {
        // 初始化 owner 指针。
        let mut owner = PSID::default();
        // 初始化 owner defaulted 标记。
        let mut owner_defaulted = BOOL::from(false);
        // 从描述符读取显式 owner。
        unsafe { GetSecurityDescriptorOwner(self.0, &mut owner, &mut owner_defaulted) }
            // 解析失败保持封闭。
            .map_err(|_| OwnerOnlyDirectoryError::SecurityUnavailable)?;
        // owner 必须存在且不是 defaulted。
        if owner.0.is_null() || owner_defaulted.as_bool() {
            // 拒绝模糊 owner。
            return Err(OwnerOnlyDirectoryError::SecurityUnavailable);
        }
        // 初始化 DACL 存在标记。
        let mut dacl_present = BOOL::from(false);
        // 初始化 DACL 指针。
        let mut dacl = std::ptr::null_mut();
        // 初始化 DACL defaulted 标记。
        let mut dacl_defaulted = BOOL::from(false);
        // 从描述符读取显式 DACL。
        unsafe {
            GetSecurityDescriptorDacl(
                // 借用自有描述符。
                self.0,
                // 接收存在事实。
                &mut dacl_present,
                // 接收 ACL 指针。
                &mut dacl,
                // 接收 defaulted 事实。
                &mut dacl_defaulted,
            )
        }
        // 读取失败保持封闭。
        .map_err(|_| OwnerOnlyDirectoryError::SecurityUnavailable)?;
        // DACL 必须显式存在、非空且非 defaulted。
        if !dacl_present.as_bool() || dacl.is_null() || dacl_defaulted.as_bool() {
            // 拒绝 null DACL 或继承默认值。
            return Err(OwnerOnlyDirectoryError::SecurityUnavailable);
        }
        // 返回受描述符生命周期保护的字段。
        Ok((owner, dacl.cast_const()))
    }
}

// 在真实父目录内创建或加固一个固定 owner-only 子目录。
pub(crate) fn ensure_owner_only_child(
    // 借用已经固定选择的父目录。
    parent: &Path,
    // 接收受严格字符集约束的单段名称。
    child_name: &str,
) -> Result<PathBuf, OwnerOnlyDirectoryError> {
    // 父目录必须是真实非 reparse 目录。
    validate_real_directory(parent).map_err(|_| OwnerOnlyDirectoryError::InvalidParent)?;
    // 子名称必须是固定 ASCII 单段。
    if !valid_child_name(child_name) {
        // 拒绝路径、别名与异常长度。
        return Err(OwnerOnlyDirectoryError::InvalidName);
    }
    // 取得当前用户 canonical SID 文本。
    let current_user = current_user_sid_string()
        // 不公开 token 或 SID 细节。
        .map_err(|_| OwnerOnlyDirectoryError::SecurityUnavailable)?;
    // 构造 owner=current-user 且只授权 SYSTEM/current-user 的 protected DACL。
    let descriptor = security_descriptor(&current_user)?;
    // 从期望描述符取得 owner 和 DACL。
    let (owner, dacl) = descriptor.owner_and_dacl()?;
    // 构造精确子目录路径。
    let directory = parent.join(child_name);
    // 编码为 NUL 结尾 UTF-16。
    let path = wide_path(&directory);
    // 转换安全属性结构尺寸。
    let length = u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
        // 异常平台尺寸失败闭合。
        .map_err(|_| OwnerOnlyDirectoryError::SecurityUnavailable)?;
    // 构造创建时即生效且不继承 handle 的安全属性。
    let attributes = SECURITY_ATTRIBUTES {
        // 保存结构尺寸。
        nLength: length,
        // 借用自有安全描述符。
        lpSecurityDescriptor: descriptor.0.0,
        // 目录创建不继承 handle。
        bInheritHandle: BOOL::from(false),
    };
    // 尝试以 owner-only DACL 原子创建精确目录。
    let created = unsafe {
        CreateDirectoryW(
            // 传入固定真实路径。
            PCWSTR(path.as_ptr()),
            // 创建时安装安全属性，避免公开权限窗口。
            Some(&attributes),
        )
    };
    // 创建失败只在既有真实目录情形继续加固。
    if created.is_err() && validate_real_directory(&directory).is_err() {
        // 其他错误不猜测平台原因。
        return Err(OwnerOnlyDirectoryError::DirectoryUnavailable);
    }
    // 新建或既有目录都必须再次通过非跟随验证。
    validate_real_directory(&directory)?;
    // 固定安装 owner、DACL 与 protected 继承边界。
    let security_information = OWNER_SECURITY_INFORMATION
        // 安装完整 DACL。
        | DACL_SECURITY_INFORMATION
        // 阻断父目录继承扩大权限。
        | PROTECTED_DACL_SECURITY_INFORMATION;
    // 让 Windows 把期望安全事实安装到精确目录。
    let status = unsafe {
        SetNamedSecurityInfoW(
            // 传入固定目录路径。
            PCWSTR(path.as_ptr()),
            // 目标是文件系统对象。
            SE_FILE_OBJECT,
            // 同时安装 owner 和 protected DACL。
            security_information,
            // owner 固定为当前用户。
            Some(owner),
            // 不改变主组。
            None,
            // DACL 只含 SYSTEM 与当前用户。
            Some(dacl),
            // 不安装 SACL。
            None,
        )
    };
    // 安装失败保持目录不可用。
    if status.is_err() {
        // 不公开权限或 SID 细节。
        return Err(OwnerOnlyDirectoryError::PermissionUnavailable);
    }
    // 从文件系统回读并逐值验证 owner、DACL 与 protected 标记。
    verify_permissions(&directory, &descriptor)?;
    // 安装权限后再次确认项目形状未被替换。
    validate_real_directory(&directory)?;
    // 返回已加固的精确目录。
    Ok(directory)
}

// 验证一个既有路径是真实非 reparse 目录。
pub(crate) fn validate_real_directory(
    // 借用精确路径。
    directory: &Path,
) -> Result<(), OwnerOnlyDirectoryError> {
    // 非跟随读取最终项目元数据。
    let metadata = fs::symlink_metadata(directory)
        // 读取失败视为目录不可用。
        .map_err(|_| OwnerOnlyDirectoryError::DirectoryUnavailable)?;
    // 拒绝符号链接、junction、reparse 与非目录项目。
    if metadata.file_type().is_symlink()
        // 普通目录形状必须成立。
        || !metadata.is_dir()
        // Windows reparse point 不得改变目录边界。
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0
    {
        // 返回独立不可信错误。
        return Err(OwnerOnlyDirectoryError::DirectoryUntrusted);
    }
    // 目录形状可信。
    Ok(())
}

// 构造 current-user owner 与两条 full-access ACE 的 protected 描述符。
fn security_descriptor(
    // 借用 canonical 当前用户 SID 文本。
    current_user: &str,
) -> Result<OwnedSecurityDescriptor, OwnerOnlyDirectoryError> {
    // 生成 owner 与目录/子项继承的 protected DACL。
    let sddl = format!(
        // SYSTEM 与当前用户拥有对象、容器继承的完全访问。
        "O:{current_user}D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{current_user})"
    );
    // 编码为 NUL 结尾 UTF-16。
    let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // 初始化 LocalAlloc 描述符指针。
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // 让 Windows 解析固定 SDDL v1。
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            // 传入固定结构与 canonical SID。
            PCWSTR(sddl.as_ptr()),
            // 使用固定 SDDL v1。
            SDDL_REVISION_1,
            // 接收 LocalAlloc 描述符。
            &mut descriptor,
            // 不公开描述符长度。
            None,
        )
    }
    // 解析失败保持封闭。
    .map_err(|_| OwnerOnlyDirectoryError::SecurityUnavailable)?;
    // 空描述符不能创建安全目录。
    if descriptor.0.is_null() {
        // 返回封闭安全错误。
        return Err(OwnerOnlyDirectoryError::SecurityUnavailable);
    }
    // 返回唯一描述符所有者。
    Ok(OwnedSecurityDescriptor(descriptor))
}

// 回读并逐值验证 owner、DACL 字节与 protected 标记。
fn verify_permissions(
    // 借用精确目录路径。
    directory: &Path,
    // 借用期望描述符。
    expected: &OwnedSecurityDescriptor,
) -> Result<(), OwnerOnlyDirectoryError> {
    // 编码为 NUL 结尾 UTF-16。
    let path = wide_path(directory);
    // 初始化回读 owner。
    let mut owner = PSID::default();
    // 初始化回读 DACL。
    let mut dacl = std::ptr::null_mut();
    // 初始化 LocalAlloc 完整描述符。
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // 只查询 owner 与 DACL。
    let status = unsafe {
        GetNamedSecurityInfoW(
            // 传入固定目录路径。
            PCWSTR(path.as_ptr()),
            // 目标是文件系统对象。
            SE_FILE_OBJECT,
            // 只读取 owner 和 DACL。
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            // 接收 owner。
            Some(&mut owner),
            // 不读取 group。
            None,
            // 接收 DACL。
            Some(&mut dacl),
            // 不读取 SACL。
            None,
            // 接收配对释放的完整描述符。
            &mut descriptor,
        )
    };
    // 查询失败或空描述符均失败闭合。
    if status.is_err() || descriptor.0.is_null() {
        // 不公开 ACL 细节。
        return Err(OwnerOnlyDirectoryError::PermissionUnavailable);
    }
    // 立即接管查询描述符。
    let descriptor = OwnedSecurityDescriptor(descriptor);
    // 取得期望 owner 与 DACL。
    let (expected_owner, expected_dacl) = expected.owner_and_dacl()?;
    // owner 与 DACL 都必须存在。
    if owner.0.is_null() || dacl.is_null() {
        // null DACL 不表示 owner-only。
        return Err(OwnerOnlyDirectoryError::PermissionUnavailable);
    }
    // owner 必须逐 SID 相等。
    if unsafe { EqualSid(owner, expected_owner) }.is_err()
        // ACL 字节必须逐值等于两条固定 ACE。
        || !equal_acl(dacl.cast_const(), expected_dacl)
    {
        // 拒绝额外主体、继承或权限漂移。
        return Err(OwnerOnlyDirectoryError::PermissionUnavailable);
    }
    // 初始化描述符 control 位。
    let mut control = 0_u16;
    // 初始化描述符 revision。
    let mut revision = 0_u32;
    // 回读 protected DACL 标志。
    unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) }
        // 查询失败保持封闭。
        .map_err(|_| OwnerOnlyDirectoryError::PermissionUnavailable)?;
    // DACL 必须受保护而不再继承父目录 ACE。
    if control & SE_DACL_PROTECTED.0 == 0 {
        // 拒绝可被继承扩大的边界。
        return Err(OwnerOnlyDirectoryError::PermissionUnavailable);
    }
    // 权限事实可信。
    Ok(())
}

// 比较两个有效 ACL 的完整内联字节。
fn equal_acl(left: *const ACL, right: *const ACL) -> bool {
    // 空指针不能形成受限 DACL。
    if left.is_null() || right.is_null() {
        // 返回不相等。
        return false;
    }
    // 读取两个 ACL 固定头部。
    let (left_size, right_size) = unsafe {
        // ACL 指针由 Windows 描述符拥有。
        ((*left).AclSize as usize, (*right).AclSize as usize)
    };
    // 尺寸必须相等且至少容纳 ACL 头。
    if left_size != right_size || left_size < size_of::<ACL>() {
        // 拒绝异常 ACL。
        return false;
    }
    // 在各自描述符生命周期内借用完整 ACL 字节。
    let left_bytes = unsafe { slice::from_raw_parts(left.cast::<u8>(), left_size) };
    // 借用期望 ACL 字节。
    let right_bytes = unsafe { slice::from_raw_parts(right.cast::<u8>(), right_size) };
    // 逐字节比较 revision、ACE 顺序、mask、flags 与 SID。
    left_bytes == right_bytes
}

// 验证固定私有子目录名称字符集。
fn valid_child_name(value: &str) -> bool {
    // 名称必须非空、有界且只含小写 ASCII、数字与连字符。
    !value.is_empty()
        && value.len() <= MAXIMUM_DIRECTORY_NAME_BYTES
        && value
            // 遍历全部字节。
            .bytes()
            // 固定字符集不会形成路径或别名。
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

// 把 Windows 路径编码为 NUL 结尾 UTF-16。
fn wide_path(path: &Path) -> Vec<u16> {
    // 保留原始 Windows 路径编码并附加终止符。
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// 声明 owner-only 目录生命周期测试。
#[cfg(test)]
#[path = "owner_only_directory_windows_tests.rs"]
mod tests;
