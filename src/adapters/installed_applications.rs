//! 传统卸载注册源与公开 Shell AppsFolder 的只读已安装应用组件。

// 导入去重集合、C 指针和资源释放所需的标准类型。
use std::{
    // 同时使用集合去重与计数表验证跨来源名称唯一性。
    collections::{HashMap, HashSet},
    // 保留 COM 属性读取所需的无类型指针。
    ffi::c_void,
};

// 导入公开 Win32 Registry、COM 与 Shell 读取 API。
use windows::{
    // 导入只读 Windows API。
    Win32::{
        // 导入错误码、HRESULT 与 PROPERTYKEY。
        Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, PROPERTYKEY, RPC_E_CHANGED_MODE},
        // 导入 Registry 与 COM API。
        System::{
            // 导入 COM 初始化与 Shell 分配内存释放。
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx},
            // 导入只读 Registry 操作与类型。
            Registry::{
                HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY,
                KEY_WOW64_64KEY, REG_SAM_FLAGS, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
                RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
                RegOpenKeyExW,
            },
        },
        // 导入 AppsFolder 的只读 Shell item 枚举接口。
        UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2,
            KF_FLAG_DEFAULT, SHGetKnownFolderItem, SIGDN_DESKTOPABSOLUTEPARSING,
            SIGDN_NORMALDISPLAY,
        },
    },
    // 导入稳定 GUID、宽字符串指针与 COM cast。
    core::{GUID, Interface, PCWSTR, PWSTR, w},
};

// 导入 opaque 应用身份与保守名称规范化组件。
use crate::{
    // 接入仅枚举两个固定 Programs Known Folder 的认证快捷方式来源。
    adapters::start_menu_applications::enumerate_start_menu_applications,
    // 复用 provider-neutral opaque 应用身份与保守名称规范化原语。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind, normalized_name},
};

// 固定 AppUserModelId 的公开 PROPERTYKEY，仅用于私有身份输入。
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    // 使用 Windows System.AppUserModel.ID 的公开 fmtid。
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    // 使用公开 property id 5。
    pid: 5,
};

// 保存 provider-neutral 已安装应用事实与私有关联提示。
#[derive(Clone, Debug)]
pub(crate) struct InstalledApplicationRecord {
    // 保存 canonical s2:a 应用目标。
    pub(crate) session_id: String,
    // 保存公开显示名称。
    pub(crate) display_name: String,
    // 保存公开版本文本。
    pub(crate) version: String,
    // 保存公开发布者文本。
    pub(crate) publisher: String,
    // 保存公开发现源。
    pub(crate) discovery_sources: Vec<String>,
    // 保存只用于保守进程关联的规范化提示。
    pub(crate) process_match_hints: Vec<String>,
    // 保存仅用于后续启动认证的 Shell parsing identity，禁止序列化。
    pub(crate) launch_identity: String,
}

// 保存合并后的应用清单与逐源完整性。
pub(crate) struct InstalledApplicationInventory {
    // 保存合并去重后的应用记录。
    pub(crate) records: Vec<InstalledApplicationRecord>,
    // 标记任一传统卸载注册源可访问。
    pub(crate) registry_source_available: bool,
    // 标记公开 Shell AppsFolder 可访问。
    pub(crate) shell_source_available: bool,
    // 标记固定 Start Menu Programs 来源可访问。
    pub(crate) start_menu_source_available: bool,
    // 标记三个来源都自然枚举完成且合并未截断。
    pub(crate) complete: bool,
}

// 保存单一来源的应用枚举结果。
struct SourceInventory {
    // 保存来源记录。
    records: Vec<InstalledApplicationRecord>,
    // 标记来源可访问。
    available: bool,
    // 标记来源自然到达末尾。
    complete: bool,
}

// 使用 RAII 保证 Registry key 在所有分支关闭。
struct RegistryKey(HKEY);

// 为 Registry key 实现确定性释放。
impl Drop for RegistryKey {
    // 关闭只读打开的 key。
    fn drop(&mut self) {
        // 忽略关闭返回码，因为读取结果已经确定。
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

// 保存当前线程 COM 初始化的释放责任。
struct ComInitialization {
    // 标记本次调用是否增加 COM 初始化引用计数。
    should_uninitialize: bool,
}

// 为 COM 初始化实现配对释放。
impl Drop for ComInitialization {
    // 在线程退出枚举作用域时释放本次初始化。
    fn drop(&mut self) {
        // 只有成功初始化才调用 CoUninitialize。
        if self.should_uninitialize {
            // 与 CoInitializeEx 配对。
            unsafe { CoUninitialize() };
        }
    }
}

// 初始化当前线程 COM，同时允许已由调用方选择其他 apartment。
fn initialize_com() -> Option<ComInitialization> {
    // 请求多线程 apartment，不启动或激活任何应用。
    let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // 成功状态需要配对释放。
    if status.is_ok() {
        // 返回持有初始化引用的 guard。
        return Some(ComInitialization {
            // 记录释放责任。
            should_uninitialize: true,
        });
    }
    // RPC_E_CHANGED_MODE 表示线程已初始化，仍可使用 Shell COM。
    if status == RPC_E_CHANGED_MODE {
        // 不释放调用方拥有的 apartment。
        return Some(ComInitialization {
            // 本组件没有新增引用。
            should_uninitialize: false,
        });
    }
    // 其他初始化失败使 Shell 来源显式不可用。
    None
}

// 从 Registry 读取 REG_SZ 或 REG_EXPAND_SZ，不展开或执行内容。
fn read_registry_string(key: HKEY, name: PCWSTR) -> String {
    // 保存值类型。
    let mut value_type = REG_VALUE_TYPE::default();
    // 保存所需字节数。
    let mut byte_count = 0_u32;
    // 查询所需缓冲区大小。
    let sized = unsafe {
        RegGetValueW(
            // 使用已打开 key。
            key,
            // 当前 key 内读取。
            PCWSTR::null(),
            // 读取指定值名。
            name,
            // 只接受字符串类型。
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            // 读取实际类型。
            Some(&mut value_type),
            // 首次不提供缓冲区。
            None,
            // 返回所需字节数。
            Some(&mut byte_count),
        )
    };
    // 查询失败或不足一个 UTF-16 code unit 时返回空值。
    if sized != ERROR_SUCCESS || byte_count < 2 {
        // 返回缺失哨兵。
        return String::new();
    }
    // 预留终止符空间并避免奇数字节向下截断。
    let code_units = usize::try_from(byte_count / 2).unwrap_or(0) + 1;
    // 创建零初始化 UTF-16 缓冲区。
    let mut buffer = vec![0_u16; code_units];
    // 允许 API 更新实际读取字节数。
    let mut requested = byte_count;
    // 读取字符串值。
    let read = unsafe {
        RegGetValueW(
            // 使用已打开 key。
            key,
            // 当前 key 内读取。
            PCWSTR::null(),
            // 使用同一值名。
            name,
            // 只接受字符串类型。
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            // 读取实际类型。
            Some(&mut value_type),
            // 写入本地缓冲区。
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            // 返回实际读取字节数。
            Some(&mut requested),
        )
    };
    // 读取失败时返回空值。
    if read != ERROR_SUCCESS {
        // 返回缺失哨兵。
        return String::new();
    }
    // 按实际字节数限制切片。
    buffer.truncate(usize::try_from(requested / 2).unwrap_or(0));
    // 删除全部尾随 NUL。
    while buffer.last() == Some(&0) {
        // 移除一个终止符。
        buffer.pop();
    }
    // 容错解码为 Rust UTF-8。
    String::from_utf16_lossy(&buffer)
}

// 从 Registry 读取 DWORD，用于排除 SystemComponent。
fn read_registry_dword(key: HKEY, name: PCWSTR) -> Option<u32> {
    // 保存 DWORD 值。
    let mut value = 0_u32;
    // 保存值类型。
    let mut value_type = REG_VALUE_TYPE::default();
    // 声明固定缓冲区大小。
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(0);
    // 读取 DWORD。
    let status = unsafe {
        RegGetValueW(
            // 使用已打开 key。
            key,
            // 当前 key 内读取。
            PCWSTR::null(),
            // 读取指定值名。
            name,
            // 只接受 DWORD。
            RRF_RT_REG_DWORD,
            // 读取实际类型。
            Some(&mut value_type),
            // 写入本地 DWORD。
            Some((&raw mut value).cast::<c_void>()),
            // 返回实际大小。
            Some(&mut size),
        )
    };
    // 成功时返回值，否则返回缺失。
    (status == ERROR_SUCCESS).then_some(value)
}

// 从 DisplayIcon 提取不含路径、引号和扩展名的私有进程关联提示。
fn icon_process_hint(display_icon: &str) -> String {
    // 空值没有提示。
    if display_icon.is_empty() {
        // 返回空提示。
        return String::new();
    }
    // 复制原始值供安全裁剪。
    let mut value = display_icon.to_owned();
    // 构造 Unicode 小写视图以搜索扩展名。
    let lower = value.to_lowercase();
    // 优先截断到第一个 .exe 结尾，丢弃图标索引参数。
    if let Some(end) = lower.find(".exe") {
        // 保留扩展名本身。
        value.truncate(end + 4);
    }
    // 只保留最后一个路径分隔符后的文件名。
    if let Some(separator) = value.rfind(['\\', '/']) {
        // 删除路径前缀。
        value.drain(..=separator);
    }
    // 删除首尾双引号。
    let value = value.trim_matches('"');
    // 删除最后一个扩展名。
    let stem = value.rsplit_once('.').map_or(value, |(stem, _)| stem);
    // 使用跨语言保守规范化规则。
    normalized_name(stem)
}

// 按 C++ 字节布局生成传统卸载应用目标。
fn registry_application_id(display_name: &str, publisher: &str, version: &str) -> String {
    // 以换行分隔三个公开事实作为私有 identity 输入。
    let identity = format!("{display_name}\n{publisher}\n{version}");
    // 只返回 canonical s2:a 指纹。
    OpaqueTargetId::new(OpaqueTargetKind::Application, &identity).to_string()
}

// 按 C++ 字节布局生成 Shell AppsFolder 应用目标。
fn shell_application_id(display_name: &str, model_id: &str) -> String {
    // 用固定 shell 前缀隔离不同来源的 identity 空间。
    let identity = format!("shell\n{display_name}\n{model_id}");
    // 只返回 canonical s2:a 指纹。
    OpaqueTargetId::new(OpaqueTargetKind::Application, &identity).to_string()
}

// 打开一个 Registry key 并交给 RAII guard。
fn open_registry_key(parent: HKEY, path: PCWSTR, access: REG_SAM_FLAGS) -> Option<RegistryKey> {
    // 保存打开结果。
    let mut key = HKEY::default();
    // 只申请读取权限。
    let status = unsafe { RegOpenKeyExW(parent, path, None, access, &mut key) };
    // 成功时返回 guard。
    (status == ERROR_SUCCESS).then_some(RegistryKey(key))
}

// 枚举单个 hive/view 的传统卸载项。
fn enumerate_registry_source(
    // 接收 hive。
    hive: HKEY,
    // 接收 WOW64 视图。
    view: REG_SAM_FLAGS,
    // 接收全局输出边界。
    maximum_items: usize,
    // 接收跨来源去重集合。
    seen: &mut HashSet<String>,
    // 接收输出记录集合。
    records: &mut Vec<InstalledApplicationRecord>,
    // 接收完整性标记。
    complete: &mut bool,
) -> bool {
    // 打开固定卸载根 key。
    let Some(uninstall) = open_registry_key(
        // 使用指定 hive。
        hive,
        // 使用公开卸载注册路径。
        w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        // 只读并指定视图。
        KEY_READ | view,
    ) else {
        // 不可访问时标记该来源不可用。
        return false;
    };
    // 从第一个子 key 开始枚举。
    let mut index = 0_u32;
    // 在全局边界内继续。
    while records.len() < maximum_items {
        // 使用与 C++ 相同的固定名称缓冲区。
        let mut subkey_name = [0_u16; 512];
        // 输入容量并接收实际长度。
        let mut length = u32::try_from(subkey_name.len()).unwrap_or(0);
        // 枚举下一个子 key。
        let next = unsafe {
            RegEnumKeyExW(
                // 使用卸载根 key。
                uninstall.0,
                // 使用当前索引。
                index,
                // 写入子 key 名。
                Some(PWSTR(subkey_name.as_mut_ptr())),
                // 返回实际长度。
                &mut length,
                // 保留参数必须为空。
                None,
                // 不读取 class。
                None,
                // 不读取 class 长度。
                None,
                // 不读取写入时间。
                None,
            )
        };
        // 每次调用后推进索引，避免错误项死循环。
        index = index.saturating_add(1);
        // 自然到达末尾。
        if next == ERROR_NO_MORE_ITEMS {
            // 完成该来源。
            break;
        }
        // 单项枚举失败时保留其他记录并标记不完整。
        if next != ERROR_SUCCESS {
            // 传播不完整状态。
            *complete = false;
            // 继续下一项。
            continue;
        }
        // 按返回长度截断并补 NUL。
        let mut path = subkey_name[..usize::try_from(length).unwrap_or(0)].to_vec();
        // 添加 Win32 字符串终止符。
        path.push(0);
        // 打开当前卸载项。
        let Some(item) = open_registry_key(
            // 使用卸载根 key。
            uninstall.0,
            // 使用动态子 key 名。
            PCWSTR(path.as_ptr()),
            // 只读并保持同一视图。
            KEY_READ | view,
        ) else {
            // 标记单项访问失败。
            *complete = false;
            // 继续下一项。
            continue;
        };
        // 排除系统组件。
        let hidden =
            read_registry_dword(item.0, w!("SystemComponent")).is_some_and(|value| value != 0);
        // 读取公开显示名称。
        let display_name = read_registry_string(item.0, w!("DisplayName"));
        // 隐藏项或缺失显示名不进入公共 inventory。
        if hidden || display_name.is_empty() {
            // 继续下一项。
            continue;
        }
        // 读取公开版本。
        let version = read_registry_string(item.0, w!("DisplayVersion"));
        // 读取公开发布者。
        let publisher = read_registry_string(item.0, w!("Publisher"));
        // 读取仅供进程提示解析的 DisplayIcon。
        let display_icon = read_registry_string(item.0, w!("DisplayIcon"));
        // 生成 canonical 应用目标。
        let session_id = registry_application_id(&display_name, &publisher, &version);
        // 跨 hive/view 重复目标只保留一个。
        if !seen.insert(session_id.clone()) {
            // 跳过重复记录。
            continue;
        }
        // 从显示名生成保守提示。
        let name_hint = normalized_name(&display_name);
        // 从 DisplayIcon 文件名生成保守提示。
        let icon_hint = icon_process_hint(&display_icon);
        // 收集非空且唯一的私有提示。
        let mut hints = Vec::new();
        // 保存显示名提示。
        if !name_hint.is_empty() {
            // 添加提示。
            hints.push(name_hint.clone());
        }
        // 保存不同的图标进程提示。
        if !icon_hint.is_empty() && icon_hint != name_hint {
            // 添加提示。
            hints.push(icon_hint);
        }
        // 保存 provider-neutral 应用记录。
        records.push(InstalledApplicationRecord {
            // 保存 opaque 目标。
            session_id,
            // 保存公开显示名。
            display_name,
            // 保存公开版本。
            version,
            // 保存公开发布者。
            publisher,
            // 标记传统卸载源。
            discovery_sources: vec!["registry-uninstall".to_owned()],
            // 保存私有关联提示。
            process_match_hints: hints,
            // 注册源不认证启动 identity。
            launch_identity: String::new(),
        });
    }
    // 达到边界时无法证明自然完成。
    if records.len() >= maximum_items {
        // 标记截断。
        *complete = false;
    }
    // 该 hive/view 可访问。
    true
}

// 枚举四个传统卸载 Registry 来源。
fn enumerate_registry_applications(maximum_items: usize) -> SourceInventory {
    // 初始化输出集合。
    let mut records = Vec::with_capacity(maximum_items);
    // 初始化跨来源 opaque 去重集合。
    let mut seen = HashSet::new();
    // 初始假定自然完成。
    let mut complete = true;
    // 初始没有可用来源。
    let mut available = false;
    // 按 C++ 相同顺序枚举当前用户/本机与 64/32 位视图。
    let sources = [
        // 当前用户 64 位视图。
        (HKEY_CURRENT_USER, KEY_WOW64_64KEY),
        // 当前用户 32 位视图。
        (HKEY_CURRENT_USER, KEY_WOW64_32KEY),
        // 本机 64 位视图。
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        // 本机 32 位视图。
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ];
    // 逐来源枚举。
    for (hive, view) in sources {
        // 全局边界已满时停止并标记不完整。
        if records.len() >= maximum_items {
            // 标记截断。
            complete = false;
            // 退出来源循环。
            break;
        }
        // 任一来源可访问即可声明 Registry coverage 可用。
        available = enumerate_registry_source(
            // 传入 hive。
            hive,
            // 传入视图。
            view,
            // 传入全局边界。
            maximum_items,
            // 传入去重集合。
            &mut seen,
            // 传入输出集合。
            &mut records,
            // 传入完整性标记。
            &mut complete,
        ) || available;
    }
    // 按显示名稳定排序。
    records.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    // 返回单源清单。
    SourceInventory {
        // 返回记录。
        records,
        // 返回可用性。
        available,
        // 返回完整性。
        complete,
    }
}

// 取得 Shell 分配的 PWSTR 内容并始终释放内存。
fn take_shell_string(result: windows::core::Result<PWSTR>) -> String {
    // 失败结果没有待释放指针。
    let Ok(pointer) = result else {
        // 返回空值。
        return String::new();
    };
    // 空指针表示属性缺失。
    if pointer.0.is_null() {
        // 返回空值。
        return String::new();
    }
    // 在释放前复制 UTF-16 内容。
    let value = unsafe { pointer.to_string() }.unwrap_or_default();
    // 使用 COM allocator 配对释放。
    unsafe { CoTaskMemFree(Some(pointer.0.cast::<c_void>())) };
    // 返回已复制字符串。
    value
}

// 枚举公开 Shell AppsFolder 来源。
fn enumerate_shell_applications(maximum_items: usize) -> SourceInventory {
    // COM 初始化失败时显式报告来源不可用。
    let Some(_com) = initialize_com() else {
        // 返回不可用来源。
        return SourceInventory {
            // 没有记录。
            records: Vec::new(),
            // 标记不可用。
            available: false,
            // 不可用来源不完整。
            complete: false,
        };
    };
    // 取得公开 AppsFolder Shell item。
    let Ok(apps_folder) = (unsafe {
        SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
    }) else {
        // 返回不可用来源。
        return SourceInventory {
            // 没有记录。
            records: Vec::new(),
            // 标记不可用。
            available: false,
            // 不可用来源不完整。
            complete: false,
        };
    };
    // 请求公开 Shell item 枚举器。
    let Ok(enumerator) = (unsafe {
        apps_folder.BindToHandler::<_, IEnumShellItems>(None::<&IBindCtx>, &BHID_EnumItems)
    }) else {
        // 返回不可用来源。
        return SourceInventory {
            // 没有记录。
            records: Vec::new(),
            // 标记不可用。
            available: false,
            // 不可用来源不完整。
            complete: false,
        };
    };
    // 初始化输出记录。
    let mut records = Vec::with_capacity(maximum_items);
    // 初始假定枚举自然完成。
    let mut complete = true;
    // 在调用方边界内枚举。
    while records.len() < maximum_items {
        // 为单项枚举准备 COM option 槽位。
        let mut items: [Option<IShellItem>; 1] = [None];
        // 保存实际返回数量。
        let mut fetched = 0_u32;
        // 请求下一项。
        let result = unsafe { enumerator.Next(&mut items, Some(&mut fetched)) };
        // COM 失败时保留已有记录并标记不完整。
        if result.is_err() {
            // 标记来源不完整。
            complete = false;
            // 结束枚举。
            break;
        }
        // S_FALSE 或零返回表示自然结束。
        if fetched == 0 {
            // 结束枚举。
            break;
        }
        // 缺失对象表示来源异常。
        let Some(item) = items[0].take() else {
            // 标记来源不完整。
            complete = false;
            // 继续下一项。
            continue;
        };
        // 读取公开显示名称。
        let display_name = take_shell_string(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) });
        // 尝试取得扩展属性接口。
        let item2 = item.cast::<IShellItem2>().ok();
        // 读取 AUMID 作为私有身份输入，禁止公开。
        let model_id = item2
            // 访问可用扩展接口。
            .as_ref()
            // 读取公开属性系统中的 AUMID。
            .map_or_else(String::new, |item| {
                // 复制并释放 Shell 字符串。
                take_shell_string(unsafe { item.GetString(&PKEY_APP_USER_MODEL_ID) })
            });
        // 读取 parsing identity 作为后续启动认证私有事实。
        let launch_identity =
            take_shell_string(unsafe { item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) });
        // 缺失显示名称的项不进入 inventory。
        if display_name.is_empty() {
            // 继续下一项。
            continue;
        }
        // 从显示名生成保守进程提示。
        let hint = normalized_name(&display_name);
        // 收集非空提示。
        let hints = if hint.is_empty() {
            // 非 ASCII 名称不猜测关联。
            Vec::new()
        } else {
            // 保存唯一显示名提示。
            vec![hint]
        };
        // 保存 Shell 应用记录。
        records.push(InstalledApplicationRecord {
            // 用显示名与私有 AUMID 生成 canonical 目标。
            session_id: shell_application_id(&display_name, &model_id),
            // 保存公开显示名。
            display_name,
            // Shell 来源没有稳定公开版本。
            version: String::new(),
            // Shell 来源没有稳定公开发布者。
            publisher: String::new(),
            // 标记公开来源。
            discovery_sources: vec!["shell-apps-folder".to_owned()],
            // 保存私有保守提示。
            process_match_hints: hints,
            // 保存私有 parsing identity。
            launch_identity,
        });
    }
    // 达到边界时无法证明来源自然完成。
    if records.len() >= maximum_items {
        // 标记截断。
        complete = false;
    }
    // 按显示名稳定排序。
    records.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    // 返回可用 Shell 来源。
    SourceInventory {
        // 返回记录。
        records,
        // AppsFolder 与枚举器均可访问。
        available: true,
        // 返回完整性。
        complete,
    }
}

// 统计单一来源快照中每个非空规范化显示名的出现次数。
fn normalized_name_counts(records: &[InstalledApplicationRecord]) -> HashMap<String, usize> {
    // 初始化只属于当前来源快照的计数表。
    let mut counts = HashMap::new();
    // 遍历全部来源记录而不修改它们。
    for application in records {
        // 生成保守 ASCII 规范化显示名。
        let name = normalized_name(&application.display_name);
        // 空规范化结果不提供关系证据。
        if name.is_empty() {
            // 继续统计其他记录。
            continue;
        }
        // 累加当前名称出现次数。
        *counts.entry(name).or_insert(0) += 1;
    }
    // 返回冻结计数快照。
    counts
}

// 把一个来源合并到现有清单，并要求名称在两侧快照中都唯一。
fn merge_source_records(
    // 接收已经按优先级合并的基础记录。
    records: &mut Vec<InstalledApplicationRecord>,
    // 接收待合并来源记录。
    incoming: Vec<InstalledApplicationRecord>,
    // 接收全局应用边界。
    maximum_items: usize,
) -> bool {
    // 在处理当前来源前冻结基础侧名称计数。
    let existing_counts = normalized_name_counts(records);
    // 同时冻结当前来源自身名称计数，防止顺序相关首项猜测。
    let incoming_counts = normalized_name_counts(&incoming);
    // 初始假定合并没有被全局边界截断。
    let mut complete = true;
    // 逐项处理当前来源。
    for mut application in incoming {
        // 计算当前记录的保守规范化名称。
        let name = normalized_name(&application.display_name);
        // 只有两侧都恰好唯一时才允许建立同一应用关系。
        let may_merge = !name.is_empty()
            // 基础侧必须唯一。
            && existing_counts.get(&name) == Some(&1)
            // 当前来源侧也必须唯一。
            && incoming_counts.get(&name) == Some(&1);
        // 在冻结计数已经证明唯一时取得基础记录。
        let existing = may_merge
            // 查找唯一规范化名称记录。
            .then(|| {
                // 只在已证明唯一的基础快照内定位记录。
                records
                    // 可变遍历基础记录。
                    .iter_mut()
                    // 精确比较保守规范化名称。
                    .find(|candidate| normalized_name(&candidate.display_name) == name)
            })
            // 展平条件与查找结果。
            .flatten();
        // 唯一关系成立时合并公开来源与私有认证事实。
        if let Some(existing) = existing {
            // 合并公开发现来源。
            for source in application.discovery_sources {
                // 去除重复来源。
                if !existing.discovery_sources.contains(&source) {
                    // 添加新来源。
                    existing.discovery_sources.push(source);
                }
            }
            // 合并私有进程提示。
            for hint in application.process_match_hints {
                // 去除重复提示。
                if !existing.process_match_hints.contains(&hint) {
                    // 添加新提示。
                    existing.process_match_hints.push(hint);
                }
            }
            // 只在高优先级基础记录没有启动路由时接受当前认证候选。
            if existing.launch_identity.is_empty() && !application.launch_identity.is_empty() {
                // 移交私有 Shell parsing identity。
                existing.launch_identity = std::mem::take(&mut application.launch_identity);
            }
            // 继续当前来源的下一记录。
            continue;
        }
        // 未命中或任一侧多命中时不猜测，保留独立应用。
        if records.len() < maximum_items {
            // 追加独立记录。
            records.push(application);
        } else {
            // 全局边界阻止追加时标记不完整。
            complete = false;
        }
    }
    // 返回当前来源是否全部进入结果。
    complete
}

// 合并 Registry、AppsFolder 与固定 Start Menu 来源，并只建立双侧唯一关系。
fn merge_sources(
    // 接收传统卸载来源。
    mut registry: SourceInventory,
    // 接收 Shell AppsFolder 来源。
    shell: SourceInventory,
    // 接收固定 Start Menu Programs 来源。
    start_menu: SourceInventory,
    // 接收全局应用边界。
    maximum_items: usize,
) -> InstalledApplicationInventory {
    // 三个来源都必须自然枚举完整。
    let mut complete = registry.complete && shell.complete && start_menu.complete;
    // 先合并优先级较高的 AppsFolder 认证路由。
    let shell_complete =
        // 执行单一来源的双侧唯一合并。
        merge_source_records(&mut registry.records, shell.records, maximum_items);
    // 保留先前来源与 AppsFolder 的组合完整性。
    complete = shell_complete && complete;
    // 再用固定 Start Menu 补齐尚无 AppsFolder 路由的应用。
    let start_menu_complete =
        // 执行固定 Start Menu 来源的双侧唯一合并。
        merge_source_records(&mut registry.records, start_menu.records, maximum_items);
    // 保留全部来源的组合完整性。
    complete = start_menu_complete && complete;
    // 规范化每个应用的来源与提示顺序。
    for application in &mut registry.records {
        // 排序公开来源。
        application.discovery_sources.sort();
        // 去除重复来源。
        application.discovery_sources.dedup();
        // 排序私有提示。
        application.process_match_hints.sort();
        // 去除重复提示。
        application.process_match_hints.dedup();
    }
    // 按显示名和 opaque ID 稳定排序。
    registry.records.sort_by(|left, right| {
        // 先比较显示名。
        left.display_name
            // 与右侧显示名比较。
            .cmp(&right.display_name)
            // 相同时使用 opaque ID 打破平局。
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    // 返回合并清单。
    InstalledApplicationInventory {
        // 返回记录。
        records: registry.records,
        // 返回 Registry 可用性。
        registry_source_available: registry.available,
        // 返回 Shell 可用性。
        shell_source_available: shell.available,
        // 返回固定 Start Menu 可用性。
        start_menu_source_available: start_menu.available,
        // 返回组合完整性。
        complete,
    }
}

// 枚举并合并所有公开已安装应用来源。
pub(crate) fn enumerate_installed_applications(
    maximum_items: usize,
) -> InstalledApplicationInventory {
    // 枚举传统卸载来源。
    let registry = enumerate_registry_applications(maximum_items);
    // 枚举公开 Shell AppsFolder 来源。
    let shell = enumerate_shell_applications(maximum_items);
    // 枚举两个固定 Programs Known Folder 来源。
    let start_menu = enumerate_start_menu_applications(maximum_items);
    // 转换为当前 Adapter 私有的统一来源快照。
    let start_menu = SourceInventory {
        // 移交已认证快捷方式记录。
        records: start_menu.records,
        // 保留固定来源可用性。
        available: start_menu.available,
        // 保留固定来源完整性。
        complete: start_menu.complete,
    };
    // 按双侧唯一规则合并三个来源。
    merge_sources(registry, shell, start_menu, maximum_items)
}

// 声明不触发应用启动的已安装应用组件测试。
#[cfg(test)]
// 把测试夹具拆到独立文件，保持生产组件低于 900 行。
#[path = "installed_applications_tests.rs"]
mod tests;
