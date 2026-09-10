//! 封装通用键盘输入所需的私有 Windows 键位映射与 SendInput 调用。

// 导入公开 JSON 构造宏以报告补偿事实。
use serde_json::json;
// 导入 Win32 键盘输入结构、标志和命名键常量。
use windows::Win32::UI::Input::KeyboardAndMouse::{
    // 导入统一输入结构与联合体。
    INPUT,
    INPUT_0,
    // 导入键盘输入类别与载荷。
    INPUT_KEYBOARD,
    // 导入键盘事件强类型标志。
    KEYBD_EVENT_FLAGS,
    KEYBDINPUT,
    // 导入扩展键、释放和 Unicode 标志。
    KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE,
    // 导入统一前景输入调用。
    SendInput,
    // 导入虚拟键强类型。
    VIRTUAL_KEY,
    // 导入数字区运算键。
    VK_ADD,
    // 导入基础编辑与导航键。
    VK_APPS,
    VK_BACK,
    VK_CAPITAL,
    VK_DECIMAL,
    VK_DELETE,
    VK_DIVIDE,
    VK_DOWN,
    VK_END,
    VK_ESCAPE,
    // 导入功能键和数字区基址。
    VK_F1,
    VK_HOME,
    VK_INSERT,
    // 导入左右修饰键。
    VK_LCONTROL,
    VK_LEFT,
    VK_LMENU,
    VK_LSHIFT,
    VK_LWIN,
    // 导入媒体键。
    VK_MEDIA_NEXT_TRACK,
    VK_MEDIA_PLAY_PAUSE,
    VK_MEDIA_PREV_TRACK,
    VK_MEDIA_STOP,
    VK_MULTIPLY,
    VK_NEXT,
    // 导入锁定键。
    VK_NUMLOCK,
    VK_NUMPAD0,
    // 导入 OEM 主键区位置键。
    VK_OEM_1,
    VK_OEM_2,
    VK_OEM_3,
    VK_OEM_4,
    VK_OEM_5,
    VK_OEM_6,
    VK_OEM_7,
    VK_OEM_COMMA,
    VK_OEM_MINUS,
    VK_OEM_PERIOD,
    VK_OEM_PLUS,
    VK_PAUSE,
    VK_PRIOR,
    VK_RCONTROL,
    VK_RETURN,
    VK_RIGHT,
    VK_RMENU,
    VK_RSHIFT,
    VK_RWIN,
    VK_SCROLL,
    VK_SNAPSHOT,
    VK_SPACE,
    VK_SUBTRACT,
    VK_TAB,
    VK_UP,
    VK_VOLUME_DOWN,
    VK_VOLUME_MUTE,
    VK_VOLUME_UP,
};

// 引入当前 Adapter 私有封闭错误类型。
#[path = "keyboard_input_windows_error.rs"]
mod error_code;

// 导入当前 Adapter 私有错误集合。
use error_code::KeyboardInputWindowsErrorCode;

// 导入 provider-neutral 键名与统一结果。
use crate::{
    // 导入已经通过 allowlist 的键名。
    components::keyboard_input_contract::KeyboardKey,
    // 导入统一结果类型。
    domain::AppResult,
};

// 保存私有 Windows 虚拟键和扩展键事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeKey {
    // 保存仅 Adapter 可见的虚拟键值。
    virtual_key: VIRTUAL_KEY,
    // 标记该键是否需要 Windows 扩展键位。
    extended: bool,
}

// 构造普通 Windows 虚拟键映射。
const fn standard(virtual_key: VIRTUAL_KEY) -> NativeKey {
    // 返回不带扩展标志的私有映射。
    NativeKey {
        // 保存虚拟键。
        virtual_key,
        // 标记普通键。
        extended: false,
    }
}

// 构造扩展 Windows 虚拟键映射。
const fn extended(virtual_key: VIRTUAL_KEY) -> NativeKey {
    // 返回带扩展标志的私有映射。
    NativeKey {
        // 保存虚拟键。
        virtual_key,
        // 标记扩展键。
        extended: true,
    }
}

// 尝试解析单字母或数字键。
fn alphanumeric_key(name: &str) -> Option<NativeKey> {
    // 只接受一个 ASCII 字节。
    if name.len() != 1 {
        // 非单字符交给其他封闭映射。
        return None;
    }
    // 读取唯一 ASCII 字节。
    let byte = name.as_bytes()[0];
    // 字母使用 Windows 大写虚拟键区间。
    if byte.is_ascii_lowercase() {
        // 映射到大写 ASCII 虚拟键。
        return Some(standard(VIRTUAL_KEY(u16::from(byte.to_ascii_uppercase()))));
    }
    // 数字直接使用 ASCII 虚拟键区间。
    if byte.is_ascii_digit() {
        // 映射数字虚拟键。
        return Some(standard(VIRTUAL_KEY(u16::from(byte))));
    }
    // 其他字符不是字母数字键。
    None
}

// 尝试解析 F1 到 F24。
fn function_key(name: &str) -> Option<NativeKey> {
    // 解析去除 F 前缀后的编号。
    let index = name.strip_prefix('f')?.parse::<u16>().ok()?;
    // 只允许公开契约的有界功能键。
    if !(1..=24).contains(&index) {
        // 超界编号没有平台映射。
        return None;
    }
    // 使用连续 Windows 虚拟键区间计算映射。
    Some(standard(VIRTUAL_KEY(VK_F1.0 + index - 1)))
}

// 尝试解析数字区 0 到 9。
fn numpad_digit_key(name: &str) -> Option<NativeKey> {
    // 读取固定前缀后的单字符编号。
    let value = name.strip_prefix("numpad-")?;
    // 非单个数字不是数字区数字键。
    if value.len() != 1 || !value.as_bytes()[0].is_ascii_digit() {
        // 交给其他数字区映射。
        return None;
    }
    // 取得数字偏移。
    let offset = u16::from(value.as_bytes()[0] - b'0');
    // 使用连续 Windows 虚拟键区间计算映射。
    Some(standard(VIRTUAL_KEY(VK_NUMPAD0.0 + offset)))
}

// 把 provider-neutral 命名键映射为私有 Windows 键位。
fn named_key(name: &str) -> Option<NativeKey> {
    // 穷举全部非规律生成键。
    let key = match name {
        // 基础编辑与空白键。
        "enter" => standard(VK_RETURN),
        // 映射 Tab。
        "tab" => standard(VK_TAB),
        // 映射 Escape。
        "escape" => standard(VK_ESCAPE),
        // 映射 Backspace。
        "backspace" => standard(VK_BACK),
        // 映射扩展 Delete。
        "delete" => extended(VK_DELETE),
        // 映射扩展 Insert。
        "insert" => extended(VK_INSERT),
        // 映射 Space。
        "space" => standard(VK_SPACE),
        // 映射扩展导航键。
        "left" => extended(VK_LEFT),
        // 映射扩展导航键。
        "up" => extended(VK_UP),
        // 映射扩展导航键。
        "right" => extended(VK_RIGHT),
        // 映射扩展导航键。
        "down" => extended(VK_DOWN),
        // 映射扩展导航键。
        "home" => extended(VK_HOME),
        // 映射扩展导航键。
        "end" => extended(VK_END),
        // 映射扩展翻页键。
        "page-up" => extended(VK_PRIOR),
        // 映射扩展翻页键。
        "page-down" => extended(VK_NEXT),
        // 映射左右修饰键。
        "left-control" => standard(VK_LCONTROL),
        // 右控制键使用扩展标志。
        "right-control" => extended(VK_RCONTROL),
        // 左 Alt 使用普通标志。
        "left-alt" => standard(VK_LMENU),
        // 右 Alt 使用扩展标志。
        "right-alt" => extended(VK_RMENU),
        // 左 Shift 使用普通标志。
        "left-shift" => standard(VK_LSHIFT),
        // 右 Shift 使用独立虚拟键。
        "right-shift" => standard(VK_RSHIFT),
        // 左 Windows 键使用扩展标志。
        "left-win" => extended(VK_LWIN),
        // 右 Windows 键使用扩展标志。
        "right-win" => extended(VK_RWIN),
        // 映射数字区运算键。
        "numpad-add" => standard(VK_ADD),
        // 映射数字区减号。
        "numpad-subtract" => standard(VK_SUBTRACT),
        // 映射数字区乘号。
        "numpad-multiply" => standard(VK_MULTIPLY),
        // 数字区除号使用扩展标志。
        "numpad-divide" => extended(VK_DIVIDE),
        // 映射数字区小数点。
        "numpad-decimal" => standard(VK_DECIMAL),
        // 数字区 Enter 通过扩展 Return 区分。
        "numpad-enter" => extended(VK_RETURN),
        // 映射主键区 OEM 位置键。
        "minus" => standard(VK_OEM_MINUS),
        // 映射等号位置键。
        "equals" => standard(VK_OEM_PLUS),
        // 映射左方括号位置键。
        "left-bracket" => standard(VK_OEM_4),
        // 映射右方括号位置键。
        "right-bracket" => standard(VK_OEM_6),
        // 映射反斜杠位置键。
        "backslash" => standard(VK_OEM_5),
        // 映射分号位置键。
        "semicolon" => standard(VK_OEM_1),
        // 映射撇号位置键。
        "apostrophe" => standard(VK_OEM_7),
        // 映射反引号位置键。
        "grave" => standard(VK_OEM_3),
        // 映射逗号位置键。
        "comma" => standard(VK_OEM_COMMA),
        // 映射句点位置键。
        "period" => standard(VK_OEM_PERIOD),
        // 映射斜杠位置键。
        "slash" => standard(VK_OEM_2),
        // 映射锁定与系统键。
        "caps-lock" => standard(VK_CAPITAL),
        // Num Lock 使用扩展标志。
        "num-lock" => extended(VK_NUMLOCK),
        // 映射 Scroll Lock。
        "scroll-lock" => standard(VK_SCROLL),
        // Print Screen 使用扩展标志。
        "print-screen" => extended(VK_SNAPSHOT),
        // 映射 Pause。
        "pause" => standard(VK_PAUSE),
        // 上下文菜单键使用扩展标志。
        "menu" => extended(VK_APPS),
        // 映射常用媒体键。
        "volume-mute" => extended(VK_VOLUME_MUTE),
        // 映射音量降低。
        "volume-down" => extended(VK_VOLUME_DOWN),
        // 映射音量提高。
        "volume-up" => extended(VK_VOLUME_UP),
        // 映射下一曲。
        "media-next" => extended(VK_MEDIA_NEXT_TRACK),
        // 映射上一曲。
        "media-previous" => extended(VK_MEDIA_PREV_TRACK),
        // 映射停止。
        "media-stop" => extended(VK_MEDIA_STOP),
        // 映射播放暂停。
        "media-play-pause" => extended(VK_MEDIA_PLAY_PAUSE),
        // 规律键或未知键交给调用方。
        _ => return None,
    };
    // 返回封闭映射。
    Some(key)
}

// 解析一个已验证键名的私有平台映射。
fn native_key(key: &KeyboardKey) -> AppResult<NativeKey> {
    // 按规律键和固定键顺序查找映射。
    alphanumeric_key(key.as_str())
        // 尝试功能键。
        .or_else(|| function_key(key.as_str()))
        // 尝试数字区数字键。
        .or_else(|| numpad_digit_key(key.as_str()))
        // 尝试固定命名键。
        .or_else(|| named_key(key.as_str()))
        // allowlist 与 Adapter 漂移必须结构化失败。
        .ok_or_else(|| {
            // 不回显原始键名或平台值。
            KeyboardInputWindowsErrorCode::MappingUnavailable
                .error("The certified keyboard key has no private Windows mapping.")
        })
}

// 在任何前景影响前验证键名与私有 Windows 映射保持一致。
pub(crate) fn validate_key_mapping(key: &KeyboardKey) -> AppResult<()> {
    // 只验证映射存在，不公开私有键位。
    let _ = native_key(key)?;
    // 返回映射有效。
    Ok(())
}

// 合并键位与阶段所需的 Windows 标志。
fn key_flags(key: NativeKey, pressed: bool) -> KEYBD_EVENT_FLAGS {
    // 从空标志开始。
    let mut flags = KEYBD_EVENT_FLAGS::default();
    // 扩展键加入固定标志。
    if key.extended {
        // 合并扩展键位。
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    // 释放阶段加入 KEYUP。
    if !pressed {
        // 合并释放标志。
        flags |= KEYEVENTF_KEYUP;
    }
    // 返回私有标志。
    flags
}

// 构造一个私有 Windows 键盘事件。
fn keyboard_input(
    // 接收私有虚拟键。
    virtual_key: VIRTUAL_KEY,
    // 接收 Unicode 单元或零。
    scan: u16,
    // 接收封闭事件标志。
    flags: KEYBD_EVENT_FLAGS,
) -> INPUT {
    // 返回只含 provider 私有字段的 INPUT。
    INPUT {
        // 固定键盘输入类别。
        r#type: INPUT_KEYBOARD,
        // 构造键盘联合体分支。
        Anonymous: INPUT_0 {
            // 填充键盘载荷。
            ki: KEYBDINPUT {
                // 保存虚拟键或 Unicode 路线的零。
                wVk: virtual_key,
                // 保存 Unicode 单元或零。
                wScan: scan,
                // 保存封闭事件标志。
                dwFlags: flags,
                // 不伪造消息时间。
                time: 0,
                // 不公开或注入额外信息。
                dwExtraInfo: 0,
            },
        },
    }
}

// 发送恰好一个键盘事件并验证完整接收。
fn send_keyboard_event(input: INPUT) -> AppResult<()> {
    // 构造单事件数组以消除批量部分接收歧义。
    let inputs = [input];
    // 安全转换固定结构尺寸。
    let size = i32::try_from(std::mem::size_of::<INPUT>()).map_err(|_| {
        // 返回稳定平台缺口。
        KeyboardInputWindowsErrorCode::KeyboardDispatchFailed
            .error("The Windows INPUT structure size is unsupported.")
    })?;
    // 调度单个事件。
    let sent = unsafe { SendInput(&inputs, size) };
    // 单事件必须完整接收。
    if sent != 1 {
        // 返回不确定平台拒绝。
        return Err(KeyboardInputWindowsErrorCode::KeyboardDispatchFailed
            .error("Windows did not accept the keyboard event."));
    }
    // 返回调度成功。
    Ok(())
}

// 发送一个显式命名键阶段。
pub(crate) fn dispatch_key(key: &KeyboardKey, pressed: bool) -> AppResult<()> {
    // 解析私有键位映射。
    let native = native_key(key)?;
    // 构造并发送单个键盘事件。
    send_keyboard_event(keyboard_input(
        // 使用私有虚拟键。
        native.virtual_key,
        // 命名键不公开或注入原始 scan code。
        0,
        // 合并扩展和阶段标志。
        key_flags(native, pressed),
    ))
}

// 构造单个 UTF-16 单元的 Windows Unicode 事件。
fn unicode_input(unit: u16, pressed: bool) -> INPUT {
    // 从 Unicode 标志开始。
    let mut flags = KEYEVENTF_UNICODE;
    // 释放阶段追加 KEYUP。
    if !pressed {
        // 合并释放标志。
        flags |= KEYEVENTF_KEYUP;
    }
    // Unicode 路线使用零虚拟键和私有 UTF-16 单元。
    keyboard_input(VIRTUAL_KEY(0), unit, flags)
}

// 发送一个 Unicode scalar 并在单元释放失败时立即补偿。
pub(crate) fn dispatch_unicode_scalar(character: char) -> AppResult<()> {
    // 创建足够容纳代理对的固定缓冲区。
    let mut buffer = [0u16; 2];
    // 把 Rust scalar 编码为一个或两个 UTF-16 单元。
    let units = character.encode_utf16(&mut buffer);
    // 按编码顺序调度每个单元。
    for unit in units {
        // 先发送 Unicode 按下。
        send_keyboard_event(unicode_input(*unit, true))?;
        // 立即发送配对释放。
        if let Err(error) = send_keyboard_event(unicode_input(*unit, false)) {
            // 首次释放失败后执行一次不受取消阻止的补偿释放。
            let release_succeeded = send_keyboard_event(unicode_input(*unit, false)).is_ok();
            // 返回不含 UTF-16 或字符值的补偿证据。
            return Err(
                KeyboardInputWindowsErrorCode::KeyboardDispatchFailed.with_details(
                    "The Unicode keyboard unit may have been accepted without a confirmed release.",
                    // 只发布 provider-neutral 补偿状态。
                    json!({
                        "causeCode": error.code,
                        "safeReleaseAttempted": true,
                        "safeReleaseSucceeded": release_succeeded,
                    }),
                ),
            );
        }
    }
    // 全部 Unicode 单元成对完成。
    Ok(())
}

// 声明无输入副作用的私有映射测试。
#[cfg(test)]
mod tests {
    // 导入被测私有原语。
    use super::*;
    // 导入契约键表与解析器。
    use crate::components::keyboard_input_contract::{
        // 导入固定命名键集合。
        NAMED_KEY_NAMES,
        // 导入单键解析器。
        parse_keyboard_key,
    };

    // 验证契约全部固定命名键都有私有 Windows 映射。
    #[test]
    fn all_named_contract_keys_have_private_mappings() -> AppResult<()> {
        // 逐项核对固定 allowlist。
        for name in NAMED_KEY_NAMES {
            // 通过正式契约解析键名。
            let key = parse_keyboard_key(name)?;
            // 要求私有映射存在。
            let _ = native_key(&key)?;
        }
        // 返回成功。
        Ok(())
    }

    // 验证全部规律键区间端点都有映射。
    #[test]
    fn generated_key_ranges_have_private_mappings() -> AppResult<()> {
        // 覆盖字母、数字、功能键和数字区数字的边界。
        for name in ["a", "z", "0", "9", "f1", "f24", "numpad-0", "numpad-9"] {
            // 通过正式契约解析键名。
            let key = parse_keyboard_key(name)?;
            // 要求私有映射存在。
            let _ = native_key(&key)?;
        }
        // 返回成功。
        Ok(())
    }

    // 验证左右控制键保持独立扩展语义。
    #[test]
    fn left_and_right_control_have_distinct_native_mappings() -> AppResult<()> {
        // 解析左控制键。
        let left = native_key(&parse_keyboard_key("left-control")?)?;
        // 解析右控制键。
        let right = native_key(&parse_keyboard_key("right-control")?)?;
        // 核对虚拟键或扩展位存在差异。
        assert_ne!(left, right);
        // 核对右控制键使用扩展标志。
        assert!(right.extended);
        // 返回成功。
        Ok(())
    }
}
