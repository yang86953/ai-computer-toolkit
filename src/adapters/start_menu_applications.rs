//! Windows 固定 Start Menu Known Folder 的只读应用来源 Component。

// 导入有界目录队列、去重集合、文件系统与 Windows 元数据扩展。
use std::{
    // 以宽度优先顺序执行有界目录遍历。
    collections::{HashSet, VecDeque},
    // 只读访问 Known Folder 中的目录项与元数据。
    fs::{self, File, Metadata},
    // 读取 Windows 文件时间和句柄事实而不公开原生路径。
    os::windows::{
        // 保留稳定的文件时间与属性读取能力。
        fs::MetadataExt,
        // 仅在当前 Component 内借用普通文件句柄。
        io::AsRawHandle,
    },
    // 保存私有 Known Folder 和快捷方式路径。
    path::{Path, PathBuf},
};

// 导入 Known Folder 解析与 Shell 分配内存释放 API。
use windows::{
    // 只在当前 Component 内使用 Win32 类型。
    Win32::{
        // 把 Rust 借用句柄转换为只读 Win32 查询参数。
        Foundation::HANDLE,
        // 查询不稳定标准库接口尚未公开的卷号与文件索引。
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
        // 释放 SHGetKnownFolderPath 返回的缓冲区。
        System::Com::CoTaskMemFree,
        // 取得当前用户与公共 Programs 目录。
        UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Programs, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        },
    },
    // 借用固定 Known Folder GUID。
    core::GUID,
};

// 导入统一应用记录、opaque 身份与名称规范化原语。
use crate::{
    // 只把已认证私有启动事实交回应用目录 Adapter。
    adapters::installed_applications::InstalledApplicationRecord,
    // 复用 canonical s2:a 与保守进程提示规则。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind, normalized_name},
};

// 限制 Start Menu 子目录递归深度，防止异常目录树无界遍历。
const MAXIMUM_DIRECTORY_DEPTH: usize = 16;
// 固定 Windows FILE_ATTRIBUTE_REPARSE_POINT 数值，禁止跟随 junction 或 symlink。
const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;
// 固定公开来源标签，不包含用户、路径或 Known Folder identity。
pub(crate) const START_MENU_SOURCE: &str = "shell-start-menu";

// 保存 Start Menu 来源的有界记录与可认证完整性。
pub(crate) struct StartMenuApplicationInventory {
    // 保存只含认证快捷方式的应用记录。
    pub(crate) records: Vec<InstalledApplicationRecord>,
    // 标记至少一个固定 Known Folder 路径可解析。
    pub(crate) available: bool,
    // 标记全部固定来源自然结束且没有超过边界。
    pub(crate) complete: bool,
}

// 将 Shell 分配的 Known Folder 路径复制为 Rust 私有路径并配对释放。
fn known_folder_path(folder_id: &GUID) -> Option<PathBuf> {
    // 请求当前用户上下文中的固定 Known Folder 路径。
    let pointer = unsafe { SHGetKnownFolderPath(folder_id, KF_FLAG_DEFAULT, None) }
        // 路径解析失败使该来源不可认证。
        .ok()?;
    // 空指针不能形成目录来源。
    if pointer.0.is_null() {
        // 返回来源不可用。
        return None;
    }
    // 在释放前严格复制 UTF-16 路径。
    let value = unsafe { pointer.to_string() }.ok();
    // 与 Shell allocator 配对释放返回缓冲区。
    unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
    // 只接受可无损表示且非空的路径。
    value
        // 删除空路径。
        .filter(|path| !path.is_empty())
        // 转换为不跨出 Component 的路径。
        .map(PathBuf::from)
}

// 判断目录项扩展名是否属于 Windows 可启动应用快捷方式。
fn is_supported_entry(path: &Path) -> bool {
    // 只读取最后一个扩展名且不解析目标内容。
    path.extension()
        // 要求扩展名可无损表示为 Unicode。
        .and_then(|extension| extension.to_str())
        // 只接受普通 Shell link 与 ClickOnce application reference。
        .is_some_and(|extension| {
            // Windows 文件扩展名比较不区分 ASCII 大小写。
            extension.eq_ignore_ascii_case("lnk")
                // ClickOnce 入口同样由 Shell 解析且不接受调用方参数。
                || extension.eq_ignore_ascii_case("appref-ms")
        })
}

// 读取普通快捷方式文件的稳定卷号与文件索引事实。
fn stable_file_identity(path: &Path) -> Option<(u32, u64)> {
    // 只读打开当前已认证普通文件。
    let file = File::open(path).ok()?;
    // 为 Win32 句柄查询初始化输出缓冲区。
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // 查询借用句柄，不转移或关闭 Rust 文件所有权。
    unsafe {
        // 获取卷号、文件索引和其余文件事实。
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information)
    }
    // 查询失败时不签发无法认证代际的目标。
    .ok()?;
    // 组合高低位为单一 64 位文件索引。
    let file_index = (u64::from(information.nFileIndexHigh) << 32)
        // 保留索引低位。
        | u64::from(information.nFileIndexLow);
    // 返回仅参与私有 identity 的稳定文件事实。
    Some((information.dwVolumeSerialNumber, file_index))
}

// 从固定目录项和文件代际事实构造 canonical Start Menu 应用目标。
fn start_menu_application_id(
    // 接收不离开 Component 的完整快捷方式路径。
    path: &str,
    // 接收不含扩展名的认证显示名。
    display_name: &str,
    // 接收卷号与文件索引代际事实。
    stable_identity: (u32, u64),
    // 接收稳定标准库能够读取的文件事实。
    metadata: &Metadata,
) -> String {
    // 将路径、卷、文件代际、时间和大小都保留在私有 identity 输入。
    let identity = format!(
        // 使用固定来源前缀隔离其他 application identity 空间。
        "start-menu\n{path}\n{display_name}\n{}\n{}\n{}\n{}\n{}",
        // 卷序列号阻止跨卷文件代际复用。
        stable_identity.0,
        // 文件索引阻止同路径替换静默复用旧身份。
        stable_identity.1,
        // 创建时间补充文件代际事实。
        metadata.creation_time(),
        // 写入时间使快捷方式内容更新产生新代际。
        metadata.last_write_time(),
        // 大小变化也必须使旧目标 stale。
        metadata.file_size(),
    );
    // 只返回 canonical s2:a 指纹。
    OpaqueTargetId::new(OpaqueTargetKind::Application, &identity).to_string()
}

// 把一个已确认普通文件的固定目录项转换为私有启动记录。
fn record_for_entry(
    path: &Path,
    metadata: &Metadata,
) -> Result<Option<InstalledApplicationRecord>, ()> {
    // 非应用快捷方式不是来源异常。
    if !is_supported_entry(path) {
        // 返回自然跳过。
        return Ok(None);
    }
    // 启动 identity 必须无损保留 Windows 路径。
    let launch_identity = path.to_str().ok_or(())?;
    // 卷号与文件索引必须由当前普通文件句柄重新认证。
    let stable_identity = stable_file_identity(path).ok_or(())?;
    // 显示名只来自固定目录项文件名，不读取目标路径或参数。
    let display_name = path
        // 删除已验证的快捷方式扩展名。
        .file_stem()
        // 要求存在文件名 stem。
        .and_then(|name| name.to_str())
        // 删除无意义外围空白。
        .map(str::trim)
        // 拒绝空显示名。
        .filter(|name| !name.is_empty())
        // 建立独立公开字符串所有权。
        .ok_or(())?
        // 复制为应用记录字段。
        .to_owned();
    // 生成保守进程名称提示。
    let hint = normalized_name(&display_name);
    // 非空提示才可参与关系建立。
    let hints = if hint.is_empty() {
        // 非 ASCII 或无字母数字名称不猜测进程关系。
        Vec::new()
    } else {
        // 保存唯一规范化显示名提示。
        vec![hint]
    };
    // 返回只携带私有 Shell parsing path 的应用记录。
    Ok(Some(InstalledApplicationRecord {
        // 用固定路径和文件代际生成 canonical 目标。
        session_id: start_menu_application_id(
            // 只把私有路径送入 opaque 哈希输入。
            launch_identity,
            // 绑定公开显示名，避免同文件不同声明复用身份。
            &display_name,
            // 绑定卷号与文件索引代际。
            stable_identity,
            // 绑定创建、写入时间与大小事实。
            metadata,
        ),
        // 公开固定目录项显示名。
        display_name,
        // Start Menu 不提供认证版本事实。
        version: String::new(),
        // Start Menu 不提供认证发布者事实。
        publisher: String::new(),
        // 只发布 provider-neutral 固定来源标签。
        discovery_sources: vec![START_MENU_SOURCE.to_owned()],
        // 保存保守进程关联提示。
        process_match_hints: hints,
        // 保存只供 Shell PIDL 重新解析的私有路径。
        launch_identity: launch_identity.to_owned(),
    }))
}

// 枚举已经由固定 Known Folder API 解析出的根目录。
fn enumerate_roots(roots: Vec<PathBuf>, maximum_items: usize) -> StartMenuApplicationInventory {
    // 零边界不能证明来源完整。
    if maximum_items == 0 {
        // 返回有界空结果。
        return StartMenuApplicationInventory {
            // 不枚举任何记录。
            records: Vec::new(),
            // 调用方已经提供根目录即表示来源可解析。
            available: !roots.is_empty(),
            // 零边界必然截断潜在来源。
            complete: false,
        };
    }
    // 去除两个 Known Folder 理论上的重复路径。
    let mut roots = roots;
    // 按 Windows 字符串表示稳定排序。
    roots.sort();
    // 删除完全相同路径。
    roots.dedup();
    // 初始化宽度优先目录队列。
    let mut queue = roots
        // 消费固定根目录集合。
        .iter()
        // 根目录从深度零开始。
        .cloned()
        // 附加深度事实。
        .map(|path| (path, 0_usize))
        // 收集为双端队列。
        .collect::<VecDeque<_>>();
    // 保存 canonical ID 去重集合。
    let mut seen = HashSet::new();
    // 保存有界应用记录。
    let mut records = Vec::with_capacity(maximum_items);
    // 初始假定全部已解析根目录自然完成。
    let mut complete = true;
    // 持续处理有界目录队列。
    while let Some((directory, depth)) = queue.pop_front() {
        // 尝试读取当前固定后代目录。
        let entries = match fs::read_dir(&directory) {
            // 收集后排序以稳定边界选择。
            Ok(entries) => entries,
            // 不存在的可选用户目录等价于自然空来源。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // 其他访问失败使来源不完整。
            Err(_) => {
                // 保留已经认证的其他记录。
                complete = false;
                // 继续其他固定根目录。
                continue;
            }
        };
        // 收集当前目录项并保留单项读取错误事实。
        let mut entries = entries
            // 把成功项和失败项都保留到本轮判断。
            .collect::<Vec<_>>();
        // 使用无损 OsString 排序，避免目录枚举顺序漂移。
        entries.sort_by(|left, right| match (left, right) {
            // 两个成功项按文件名排序。
            (Ok(left), Ok(right)) => left.file_name().cmp(&right.file_name()),
            // 成功项稳定排在失败项前。
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            // 失败项稳定排在成功项后。
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            // 两个失败项不携带可安全比较事实。
            (Err(_), Err(_)) => std::cmp::Ordering::Equal,
        });
        // 逐项处理当前目录。
        for entry in entries {
            // 目录项读取失败使来源不完整。
            let Ok(entry) = entry else {
                // 记录完整性缺口。
                complete = false;
                // 继续其他项。
                continue;
            };
            // 读取不跟随链接的元数据以识别 reparse point。
            let metadata = match fs::symlink_metadata(entry.path()) {
                // 保存当前目录项事实。
                Ok(metadata) => metadata,
                // 元数据竞态失败使来源不完整。
                Err(_) => {
                    // 记录完整性缺口。
                    complete = false;
                    // 继续其他项。
                    continue;
                }
            };
            // 绝不跟随 junction、symlink 或其他 reparse point。
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0 {
                // 非普通目录项不属于认证来源。
                continue;
            }
            // 普通子目录按固定深度继续遍历。
            if metadata.is_dir() {
                // 超过深度时拒绝继续并标记不完整。
                if depth >= MAXIMUM_DIRECTORY_DEPTH {
                    // 当前子树没有自然完成。
                    complete = false;
                    // 继续同层其他项。
                    continue;
                }
                // 把普通后代目录加入宽度优先队列。
                queue.push_back((entry.path(), depth.saturating_add(1)));
                // 当前目录项处理完成。
                continue;
            }
            // 非普通文件不进入应用来源。
            if !metadata.is_file() {
                // 跳过设备或其他特殊类型。
                continue;
            }
            // 只转换支持的应用快捷方式。
            let record = match record_for_entry(&entry.path(), &metadata) {
                // 保存认证记录或自然跳过。
                Ok(record) => record,
                // 无法无损绑定身份时标记来源不完整。
                Err(()) => {
                    // 保留其他已认证记录。
                    complete = false;
                    // 继续同层其他项。
                    continue;
                }
            };
            // 非支持文件自然跳过。
            let Some(record) = record else {
                // 继续同层其他项。
                continue;
            };
            // 达到全局记录边界时停止并标记截断。
            if records.len() >= maximum_items {
                // 尚有认证候选未返回。
                complete = false;
                // 清空队列以结束外层循环。
                queue.clear();
                // 结束当前目录循环。
                break;
            }
            // 相同文件代际只返回一次。
            if seen.insert(record.session_id.clone()) {
                // 保存认证记录。
                records.push(record);
            }
        }
    }
    // 按显示名和 opaque ID 稳定排序公开顺序。
    records.sort_by(|left, right| {
        // 先比较显示名。
        left.display_name
            // 与右侧名称比较。
            .cmp(&right.display_name)
            // 同名时按 canonical ID 打破平局。
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    // 返回固定来源枚举结果。
    StartMenuApplicationInventory {
        // 返回认证记录。
        records,
        // 至少一个 Known Folder 路径已解析即表示来源可用。
        available: !roots.is_empty(),
        // 返回自然完成与边界事实。
        complete,
    }
}

// 枚举当前用户与公共 Start Menu Programs 两个固定来源。
pub(crate) fn enumerate_start_menu_applications(
    // 接收全局应用硬上限。
    maximum_items: usize,
) -> StartMenuApplicationInventory {
    // 保存成功解析的固定 Known Folder 根目录。
    let mut roots = Vec::new();
    // 初始假定两个固定路径都可解析。
    let mut paths_complete = true;
    // 按当前用户、公共程序目录的固定顺序解析。
    for folder_id in [&FOLDERID_Programs, &FOLDERID_CommonPrograms] {
        // 只接受 Known Folder API 返回的路径。
        match known_folder_path(folder_id) {
            // 保存私有固定根目录。
            Some(path) => roots.push(path),
            // 任一固定路径不可解析使来源不完整。
            None => paths_complete = false,
        }
    }
    // 枚举已经认证来源的后代快捷方式。
    let mut inventory = enumerate_roots(roots, maximum_items);
    // Known Folder 解析失败不能被剩余目录自然结束掩盖。
    inventory.complete = inventory.complete && paths_complete;
    // 返回完整来源事实。
    inventory
}

// 将文件系统夹具测试放在独立文件，保持生产 Component 紧凑。
#[cfg(test)]
#[path = "start_menu_applications_tests.rs"]
mod tests;
