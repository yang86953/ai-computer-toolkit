//! 封装窗口 mutation 共享的当前 Windows token 与进程代际核对。

// 导入窗口句柄与只读身份查询 API。
use windows::Win32::{
    // 导入强类型窗口句柄。
    Foundation::HWND,
    // 导入窗口存在性与所属进程查询。
    UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow},
};

// 导入当前快照的私有窗口事实与进程代际查询。
use crate::adapters::windows::{WindowRecord, process_creation_time};

// 把私有整数句柄恢复为仅在 Windows Adapter 内使用的类型。
pub(crate) fn native_window(target: &WindowRecord) -> HWND {
    // 公共契约永不返回该句柄。
    HWND(target.hwnd as *mut std::ffi::c_void)
}

// 核对当前窗口 token 仍绑定到同一 PID 与进程代际。
pub(crate) fn same_window_identity(target: &WindowRecord) -> bool {
    // 恢复私有窗口句柄。
    let window = native_window(target);
    // 句柄必须仍表示窗口。
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        // 当前 token 失效即不再是同一目标。
        return false;
    }
    // 保存当前所属 PID。
    let mut process_id = 0_u32;
    // 只读查询当前句柄所属进程。
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    // PID 必须与重新发现快照一致；此查询不提供窗口创建代际。
    if process_id == 0 || process_id != target.process_id {
        // PID 缺失或变化表示目标已过期。
        return false;
    }
    // 进程创建时间只能检测 PID 进入新进程代际，不能证明同进程窗口 token 未被回收。
    process_creation_time(process_id) == target.process_creation_time
}

// 声明当前窄 Component 的纯身份核对测试。
#[cfg(test)]
mod tests {
    // 导入被测转换与私有窗口事实。
    use super::{WindowRecord, native_window};

    // 验证整数句柄转换不改变私有值。
    #[test]
    fn native_window_preserves_private_handle_value() {
        // 构造不触发平台查询的稳定窗口事实。
        let target = WindowRecord {
            // legacy ID 不参与当前测试。
            session_id: "window:4660".to_owned(),
            // 使用稳定私有句柄值。
            hwnd: 4660,
            // 使用无敏感含义标题。
            title: "Fixture".to_owned(),
            // 使用无敏感含义类名。
            class_name: "FixtureClass".to_owned(),
            // 使用稳定测试 PID。
            process_id: 42,
            // 使用安全进程名。
            process_name: Some("fixture.exe".to_owned()),
            // 标记夹具可见。
            visible: true,
            // 使用稳定进程代际。
            process_creation_time: 123,
        };
        // 核对恢复后的平台值。
        assert_eq!(native_window(&target).0 as isize, target.hwnd);
    }
}
