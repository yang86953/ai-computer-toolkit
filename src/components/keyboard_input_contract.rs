//! 定义 provider-neutral 键盘输入与请求内按键所有权契约。

// 导入有序集合以验证按键状态机。
use std::collections::BTreeSet;

// 导入严格 JSON 反序列化。
use serde::Deserialize;
// 导入公开 JSON 值。
use serde_json::Value;

// 导入统一结果与错误类型。
use crate::domain::{AppControlError, AppResult};

// 固定一次请求允许的最大公开步骤数。
pub(crate) const MAXIMUM_KEYBOARD_STEPS: usize = 128;
// 固定展开后的最大平台工作单元数。
pub(crate) const MAXIMUM_KEYBOARD_WORK_UNITS: usize = 16_384;
// 固定单个文本步骤允许的最大 UTF-16 单元数。
pub(crate) const MAXIMUM_TEXT_UTF16_UNITS: usize = 4_096;
// 固定同步键盘请求最短 deadline。
pub(crate) const MINIMUM_KEYBOARD_TIMEOUT_MS: u32 = 1;
// 固定同步键盘请求最长 deadline。
pub(crate) const MAXIMUM_KEYBOARD_TIMEOUT_MS: u32 = 30_000;
// 固定同步键盘请求缺省 deadline。
pub(crate) const DEFAULT_KEYBOARD_TIMEOUT_MS: u32 = 2_000;
// 固定一次 press 或 chord 的最长持有时间。
pub(crate) const MAXIMUM_KEY_HOLD_MS: u32 = 5_000;
// 固定一次 press 或 chord 的最大重复次数。
pub(crate) const MAXIMUM_KEY_REPEAT: u16 = 100;
// 固定重复之间的最长等待。
pub(crate) const MAXIMUM_KEY_INTERVAL_MS: u32 = 1_000;
// 固定单个 chord 的最大同时按键数。
pub(crate) const MAXIMUM_CHORD_KEYS: usize = 8;

// 保存全部非规律生成的 canonical 命名键。
pub(crate) const NAMED_KEY_NAMES: &[&str] = &[
    // 基础编辑与空白键。
    "enter",
    "tab",
    "escape",
    "backspace",
    "delete",
    "insert",
    "space",
    // 导航键。
    "left",
    "up",
    "right",
    "down",
    "home",
    "end",
    "page-up",
    "page-down",
    // 左右修饰键。
    "left-control",
    "right-control",
    "left-alt",
    "right-alt",
    "left-shift",
    "right-shift",
    "left-win",
    "right-win",
    // 数字区运算与特殊键。
    "numpad-add",
    "numpad-subtract",
    "numpad-multiply",
    "numpad-divide",
    "numpad-decimal",
    "numpad-enter",
    // 主键区标点位置键。
    "minus",
    "equals",
    "left-bracket",
    "right-bracket",
    "backslash",
    "semicolon",
    "apostrophe",
    "grave",
    "comma",
    "period",
    "slash",
    // 锁定与系统键。
    "caps-lock",
    "num-lock",
    "scroll-lock",
    "print-screen",
    "pause",
    "menu",
    // 常用媒体键。
    "volume-mute",
    "volume-down",
    "volume-up",
    "media-next",
    "media-previous",
    "media-stop",
    "media-play-pause",
];

// 保存已经规范化且通过 allowlist 的 provider-neutral 键名。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct KeyboardKey {
    // 保存 canonical 小写连字符名称。
    name: String,
}

// 提供键名的稳定公开投影。
impl KeyboardKey {
    // 返回不含平台键码的 canonical 名称。
    pub(crate) fn as_str(&self) -> &str {
        // 借用经过验证的内部字符串。
        self.name.as_str()
    }
}

// 表示显式按键阶段。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum KeyboardKeyPhase {
    // 表示成对按下、等待并释放。
    Press,
    // 表示按下并由当前请求取得释放责任。
    Down,
    // 表示释放当前请求持有的按键。
    Up,
}

// 返回缺省按键阶段。
const fn default_key_phase() -> KeyboardKeyPhase {
    // 单次成对 press 是默认语义。
    KeyboardKeyPhase::Press
}

// 返回缺省重复次数。
const fn default_repeat() -> u16 {
    // 缺省只执行一次。
    1
}

// 表示一次请求中的 provider-neutral 键盘步骤。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeyboardStep {
    // 表示单个命名键阶段。
    Key {
        // 保存 provider-neutral 键名。
        key: KeyboardKey,
        // 保存 press、down 或 up 阶段。
        phase: KeyboardKeyPhase,
        // 保存 press 持续时间。
        hold_ms: u32,
        // 保存 press 重复次数。
        repeat: u16,
        // 保存重复之间等待。
        interval_ms: u32,
    },
    // 表示按给定顺序按下并逆序释放的快捷键。
    Chord {
        // 保存有序且不重复的键名。
        keys: Vec<KeyboardKey>,
        // 保存整组按键持续时间。
        hold_ms: u32,
        // 保存快捷键重复次数。
        repeat: u16,
        // 保存重复之间等待。
        interval_ms: u32,
    },
    // 表示 Unicode 文本输入。
    Text {
        // 保存不含 NUL 的有界 Unicode 文本。
        text: String,
        // 保存已经验证的 UTF-16 单元数。
        utf16_units: usize,
    },
}

// 提供步骤的稳定公开类型与工作预算。
impl KeyboardStep {
    // 返回公开类型名称。
    pub(crate) const fn kind(&self) -> &'static str {
        // 穷举全部三种步骤。
        match self {
            // 映射单键步骤。
            Self::Key { .. } => "key",
            // 映射快捷键步骤。
            Self::Chord { .. } => "chord",
            // 映射文本步骤。
            Self::Text { .. } => "text",
        }
    }

    // 估算展开后平台调用的工作单元数。
    const fn work_units(&self) -> usize {
        // 为每种步骤计算保守上界。
        match self {
            // 单键 down/up 是一个事件，press 每次包含一对事件。
            Self::Key { phase, repeat, .. } => match phase {
                // 成对 press 按重复次数展开。
                KeyboardKeyPhase::Press => *repeat as usize * 2,
                // 显式阶段只发送一个事件。
                KeyboardKeyPhase::Down | KeyboardKeyPhase::Up => 1,
            },
            // 快捷键每次对全部键成对调度。
            Self::Chord { keys, repeat, .. } => keys.len() * *repeat as usize * 2,
            // 每个 UTF-16 单元最多包含按下和释放两个事件。
            Self::Text { utf16_units, .. } => *utf16_units * 2,
        }
    }
}

// 保存通过验证的键盘请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyboardInput {
    // 保存有界动作序列。
    pub(crate) steps: Vec<KeyboardStep>,
    // 保存单调 deadline。
    pub(crate) timeout_ms: u32,
    // 标记是否来自旧 key/chord 兼容形状。
    pub(crate) legacy_key_compatibility: bool,
}

// 保存单键 wire 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KeyStepWire {
    // 保存调用方键名。
    key: String,
    // 保存可选阶段。
    #[serde(default = "default_key_phase")]
    phase: KeyboardKeyPhase,
    // 保存可选持续时间。
    #[serde(default)]
    hold_ms: u32,
    // 保存可选重复次数。
    #[serde(default = "default_repeat")]
    repeat: u16,
    // 保存可选重复间隔。
    #[serde(default)]
    interval_ms: u32,
}

// 保存快捷键 wire 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ChordStepWire {
    // 保存调用方有序键名。
    keys: Vec<String>,
    // 保存可选持续时间。
    #[serde(default)]
    hold_ms: u32,
    // 保存可选重复次数。
    #[serde(default = "default_repeat")]
    repeat: u16,
    // 保存可选重复间隔。
    #[serde(default)]
    interval_ms: u32,
}

// 保存文本 wire 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextStepWire {
    // 保存 Unicode 文本。
    text: String,
}

// 保存公开步骤 wire 判别联合。
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum KeyboardStepWire {
    // 保存单键步骤。
    Key(KeyStepWire),
    // 保存快捷键步骤。
    Chord(ChordStepWire),
    // 保存文本步骤。
    Text(TextStepWire),
}

// 保存正式动作序列 JSON 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KeyboardSequenceWire {
    // 保存公开步骤数组。
    steps: Vec<KeyboardStepWire>,
    // 保存可选 deadline。
    #[serde(default = "default_keyboard_timeout_ms")]
    timeout_ms: u32,
}

// 保存旧 `{key,phase,holdMs}` 兼容形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyKeyboardWire {
    // 保存单键或加号组合。
    key: String,
    // 保存旧可选阶段。
    #[serde(default = "default_key_phase")]
    phase: KeyboardKeyPhase,
    // 保存旧可选持续时间。
    #[serde(default)]
    hold_ms: u32,
    // 保存可选 deadline。
    #[serde(default = "default_keyboard_timeout_ms")]
    timeout_ms: u32,
}

// 返回 serde 使用的缺省 deadline。
const fn default_keyboard_timeout_ms() -> u32 {
    // 复用公开固定常量。
    DEFAULT_KEYBOARD_TIMEOUT_MS
}

// 构造不回显原始 JSON 的稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回统一公开错误 envelope。
    AppControlError::new("INVALID_ARGUMENT", message)
}

// 判断规范化文本是否是单个字母或数字键。
fn is_alphanumeric_key(name: &str) -> bool {
    // 只允许一个 ASCII 字母或数字。
    name.len() == 1
        // 读取唯一字节。
        && name.as_bytes()[0].is_ascii_alphanumeric()
}

// 判断规范化文本是否属于规律生成的功能键。
fn is_function_key(name: &str) -> bool {
    // 去掉固定前缀并解析有界编号。
    name.strip_prefix('f')
        // 解析十进制编号。
        .and_then(|value| value.parse::<u8>().ok())
        // 只允许 F1 到 F24。
        .is_some_and(|value| (1..=24).contains(&value))
}

// 判断规范化文本是否属于规律生成的数字区数字键。
fn is_numpad_digit(name: &str) -> bool {
    // 去掉固定前缀。
    let Some(value) = name.strip_prefix("numpad-") else {
        // 缺少前缀时不是数字区键。
        return false;
    };
    // 只允许单个 ASCII 数字。
    value.len() == 1
        // 核对唯一字节。
        && value.as_bytes()[0].is_ascii_digit()
}

// 判断名称是否属于完整公开键集。
fn is_supported_key(name: &str) -> bool {
    // 合并规律键与固定命名键。
    is_alphanumeric_key(name)
        // 接受功能键。
        || is_function_key(name)
        // 接受数字区数字键。
        || is_numpad_digit(name)
        // 接受固定 allowlist。
        || NAMED_KEY_NAMES.contains(&name)
}

// 把旧别名映射为 canonical provider-neutral 名称。
fn legacy_alias(name: &str) -> &str {
    // 穷举保留兼容的安全别名。
    match name {
        // 泛化控制键映射到左控制键。
        "ctrl" | "control" => "left-control",
        // 泛化 Alt 映射到左 Alt。
        "alt" => "left-alt",
        // 泛化 Shift 映射到左 Shift。
        "shift" => "left-shift",
        // 泛化 Windows 键映射到左 Windows 键。
        "win" | "windows" => "left-win",
        // 保留常用编辑键别名。
        "return" => "enter",
        // 保留 Escape 缩写。
        "esc" => "escape",
        // 保留 Delete 缩写。
        "del" => "delete",
        // 保留 Backspace 缩写。
        "back" => "backspace",
        // 其余名称保持规范化结果。
        _ => name,
    }
}

// 解析并规范化一个 provider-neutral 键名。
pub(crate) fn parse_keyboard_key(value: &str) -> AppResult<KeyboardKey> {
    // 统一大小写、空白和下划线拼写。
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    // 应用封闭旧别名映射。
    let canonical = legacy_alias(&normalized);
    // 未知键必须在任何目标发现和输入前失败。
    if !is_supported_key(canonical) {
        // 返回不回显平台值的固定错误。
        return Err(invalid_argument(
            "Keyboard key is not in the provider-neutral key-input-v1 allowlist.",
        ));
    }
    // 保存 canonical 名称。
    Ok(KeyboardKey {
        // 复制有界名称。
        name: canonical.to_owned(),
    })
}

// 验证 press/chord 共用的持续时间、重复和间隔。
fn validate_repeat_options(hold_ms: u32, repeat: u16, interval_ms: u32) -> AppResult<()> {
    // 持续时间必须保持有界。
    if hold_ms > MAXIMUM_KEY_HOLD_MS {
        // 返回固定持续时间错误。
        return Err(invalid_argument(format!(
            "Keyboard holdMs must be within 0..={MAXIMUM_KEY_HOLD_MS}."
        )));
    }
    // 重复次数必须非零且有界。
    if repeat == 0 || repeat > MAXIMUM_KEY_REPEAT {
        // 返回固定重复边界错误。
        return Err(invalid_argument(format!(
            "Keyboard repeat must be within 1..={MAXIMUM_KEY_REPEAT}."
        )));
    }
    // 重复间隔必须保持有界。
    if interval_ms > MAXIMUM_KEY_INTERVAL_MS {
        // 返回固定间隔错误。
        return Err(invalid_argument(format!(
            "Keyboard intervalMs must be within 0..={MAXIMUM_KEY_INTERVAL_MS}."
        )));
    }
    // 单次执行不接受无效果的重复间隔。
    if repeat == 1 && interval_ms != 0 {
        // 返回字段组合错误。
        return Err(invalid_argument(
            "Keyboard intervalMs requires repeat greater than one.",
        ));
    }
    // 全部选项有效。
    Ok(())
}

// 把 wire 单键步骤转换为领域步骤。
fn key_step(wire: KeyStepWire) -> AppResult<KeyboardStep> {
    // 先解析 provider-neutral 键名。
    let key = parse_keyboard_key(&wire.key)?;
    // 按阶段验证选项所有权。
    match wire.phase {
        // press 拥有成对生命周期和重复语义。
        KeyboardKeyPhase::Press => {
            // 验证持续时间与重复选项。
            validate_repeat_options(wire.hold_ms, wire.repeat, wire.interval_ms)?;
        }
        // 显式 down/up 不得伪装等待或重复。
        KeyboardKeyPhase::Down | KeyboardKeyPhase::Up => {
            // 非缺省选项会产生跨阶段歧义。
            if wire.hold_ms != 0 || wire.repeat != 1 || wire.interval_ms != 0 {
                // 返回固定字段组合错误。
                return Err(invalid_argument(
                    "Keyboard down/up only allow holdMs=0, repeat=1 and intervalMs=0.",
                ));
            }
        }
    }
    // 构造已验证领域步骤。
    Ok(KeyboardStep::Key {
        // 保存键名。
        key,
        // 保存阶段。
        phase: wire.phase,
        // 保存持续时间。
        hold_ms: wire.hold_ms,
        // 保存重复次数。
        repeat: wire.repeat,
        // 保存重复间隔。
        interval_ms: wire.interval_ms,
    })
}

// 把 wire 快捷键步骤转换为领域步骤。
fn chord_step(wire: ChordStepWire) -> AppResult<KeyboardStep> {
    // 快捷键至少两个键且保持有界。
    if wire.keys.len() < 2 || wire.keys.len() > MAXIMUM_CHORD_KEYS {
        // 返回固定按键数量错误。
        return Err(invalid_argument(format!(
            "Keyboard chord requires 2..={MAXIMUM_CHORD_KEYS} keys."
        )));
    }
    // 解析全部 provider-neutral 键名。
    let keys = wire
        // 按调用方顺序迭代。
        .keys
        // 转换每个名称。
        .iter()
        // 使用封闭解析器。
        .map(|key| parse_keyboard_key(key))
        // 首个未知键立即失败。
        .collect::<AppResult<Vec<_>>>()?;
    // 保存快捷键内唯一键集合。
    let unique = keys.iter().cloned().collect::<BTreeSet<_>>();
    // 重复键破坏唯一释放责任。
    if unique.len() != keys.len() {
        // 返回固定重复键错误。
        return Err(invalid_argument(
            "Keyboard chord cannot contain duplicate keys.",
        ));
    }
    // 验证持续时间与重复选项。
    validate_repeat_options(wire.hold_ms, wire.repeat, wire.interval_ms)?;
    // 构造已验证快捷键步骤。
    Ok(KeyboardStep::Chord {
        // 保留调用方组合顺序。
        keys,
        // 保存持续时间。
        hold_ms: wire.hold_ms,
        // 保存重复次数。
        repeat: wire.repeat,
        // 保存重复间隔。
        interval_ms: wire.interval_ms,
    })
}

// 把 wire 文本步骤转换为领域步骤。
fn text_step(wire: TextStepWire) -> AppResult<KeyboardStep> {
    // 空文本没有可验证领域效果。
    if wire.text.is_empty() {
        // 返回固定空文本错误。
        return Err(invalid_argument("Keyboard text must not be empty."));
    }
    // NUL 无法形成可靠可见 Unicode 输入。
    if wire.text.contains('\0') {
        // 返回固定 NUL 错误。
        return Err(invalid_argument("Keyboard text must not contain NUL."));
    }
    // 计算 Windows Unicode 路线实际使用的 UTF-16 单元数。
    let utf16_units = wire.text.encode_utf16().count();
    // 文本范围必须保持有界。
    if utf16_units > MAXIMUM_TEXT_UTF16_UNITS {
        // 返回固定文本边界错误。
        return Err(invalid_argument(format!(
            "Keyboard text exceeds {MAXIMUM_TEXT_UTF16_UNITS} UTF-16 units."
        )));
    }
    // 构造已验证文本步骤。
    Ok(KeyboardStep::Text {
        // 保存原 Unicode 文本。
        text: wire.text,
        // 保存工作预算证据。
        utf16_units,
    })
}

// 把公开 wire 步骤转换为领域步骤。
fn convert_step(wire: KeyboardStepWire) -> AppResult<KeyboardStep> {
    // 按封闭步骤类型转换。
    match wire {
        // 转换单键步骤。
        KeyboardStepWire::Key(value) => key_step(value),
        // 转换快捷键步骤。
        KeyboardStepWire::Chord(value) => chord_step(value),
        // 转换文本步骤。
        KeyboardStepWire::Text(value) => text_step(value),
    }
}

// 验证动作序列与请求内按键所有权。
fn validate_sequence(input: &KeyboardInput) -> AppResult<()> {
    // 步骤数组必须非空且保持有界。
    if input.steps.is_empty() || input.steps.len() > MAXIMUM_KEYBOARD_STEPS {
        // 返回固定步骤边界错误。
        return Err(invalid_argument(format!(
            "Keyboard input requires 1..={MAXIMUM_KEYBOARD_STEPS} steps."
        )));
    }
    // deadline 必须处于同步调用认证范围。
    if !(MINIMUM_KEYBOARD_TIMEOUT_MS..=MAXIMUM_KEYBOARD_TIMEOUT_MS)
        // 核对调用方值。
        .contains(&input.timeout_ms)
    {
        // 返回固定 deadline 边界错误。
        return Err(invalid_argument(format!(
            "Keyboard input timeoutMs must be within {MINIMUM_KEYBOARD_TIMEOUT_MS}..={MAXIMUM_KEYBOARD_TIMEOUT_MS}."
        )));
    }
    // 保存当前请求已经按下且必须释放的键集合。
    let mut held = BTreeSet::new();
    // 累计展开后的平台工作预算。
    let mut work_units = 0usize;
    // 按公开顺序验证每一步。
    for step in &input.steps {
        // 累加保守工作单元数。
        work_units = work_units.saturating_add(step.work_units());
        // 超出预算时立即拒绝。
        if work_units > MAXIMUM_KEYBOARD_WORK_UNITS {
            // 返回固定工作预算错误。
            return Err(invalid_argument(format!(
                "Keyboard input expands beyond {MAXIMUM_KEYBOARD_WORK_UNITS} work units."
            )));
        }
        // 执行步骤专属状态检查。
        match step {
            // 单键步骤可以改变请求内持有集合。
            KeyboardStep::Key { key, phase, .. } => match phase {
                // 成对 press 不得与显式持有的同键重叠。
                KeyboardKeyPhase::Press => {
                    // 同键已持有会破坏唯一释放责任。
                    if held.contains(key) {
                        // 返回固定状态冲突错误。
                        return Err(invalid_argument(
                            "Keyboard press cannot target a key already held by this request.",
                        ));
                    }
                }
                // down 取得当前请求释放责任。
                KeyboardKeyPhase::Down => {
                    // 重复按下会破坏唯一所有权。
                    if !held.insert(key.clone()) {
                        // 返回固定重复按下错误。
                        return Err(invalid_argument(
                            "A keyboard key cannot be pressed twice without an intervening release.",
                        ));
                    }
                }
                // up 必须对应当前请求中的 down。
                KeyboardKeyPhase::Up => {
                    // 跨请求释放没有可证明的所有权。
                    if !held.remove(key) {
                        // 返回固定无所有者释放错误。
                        return Err(invalid_argument(
                            "A keyboard key release must match an earlier down in the same request.",
                        ));
                    }
                }
            },
            // 快捷键宏拥有自己的完整生命周期。
            KeyboardStep::Chord { .. } => {
                // 禁止快捷键隐式共享显式持有状态。
                if !held.is_empty() {
                    // 返回固定状态冲突错误。
                    return Err(invalid_argument(
                        "Keyboard chord cannot execute while this request holds keys.",
                    ));
                }
            }
            // Unicode 文本不能继承修饰键状态。
            KeyboardStep::Text { .. } => {
                // 持有键会让 Unicode 语义依赖外部布局或快捷键。
                if !held.is_empty() {
                    // 返回固定状态冲突错误。
                    return Err(invalid_argument(
                        "Keyboard text cannot execute while this request holds keys.",
                    ));
                }
            }
        }
    }
    // 请求返回前不得遗留任何工具持有键。
    if !held.is_empty() {
        // 返回固定未闭合状态错误。
        return Err(invalid_argument(
            "Every keyboard down must be released in the same request.",
        ));
    }
    // 全部契约检查通过。
    Ok(())
}

// 严格解析正式动作序列或旧 key/chord 兼容形状。
pub(crate) fn parse_keyboard_input(value: &Value) -> AppResult<KeyboardInput> {
    // 必须先取得对象形状才能选择版本化分支。
    let object = value.as_object().ok_or_else(|| {
        // 返回不含原始输入的稳定错误。
        invalid_argument("Keyboard input must be an object.")
    })?;
    // `steps` 是正式动作序列的唯一判别字段。
    let input = if object.contains_key("steps") {
        // 严格反序列化正式形状。
        let wire: KeyboardSequenceWire = serde_json::from_value(value.clone()).map_err(|_| {
            // 不穿透 serde 可能携带的输入片段。
            invalid_argument("Keyboard input does not match the key-input-v1 sequence schema.")
        })?;
        // 转换全部公开步骤。
        let steps = wire
            // 取得步骤所有权。
            .steps
            // 按顺序迭代。
            .into_iter()
            // 转换为领域步骤。
            .map(convert_step)
            // 首个失败立即停止。
            .collect::<AppResult<Vec<_>>>()?;
        // 构造正式领域对象。
        KeyboardInput {
            // 保存动作序列。
            steps,
            // 保存 deadline。
            timeout_ms: wire.timeout_ms,
            // 标记不是兼容调用。
            legacy_key_compatibility: false,
        }
    } else {
        // 严格反序列化旧 key 形状。
        let wire: LegacyKeyboardWire = serde_json::from_value(value.clone()).map_err(|_| {
            // 不穿透 serde 可能携带的输入片段。
            invalid_argument("Keyboard input does not match the legacy key compatibility shape.")
        })?;
        // 旧 down/up 无法在短命请求中证明安全配平。
        if wire.phase != KeyboardKeyPhase::Press {
            // 引导调用方改用正式同请求步骤序列。
            return Err(invalid_argument(
                "Legacy keyboard phase only supports press; use balanced steps for down/up.",
            ));
        }
        // 按旧加号组合拆分并保留顺序。
        let parts = wire.key.split('+').map(str::trim).collect::<Vec<_>>();
        // 空键名或空组合成员必须失败。
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            // 返回固定旧形状错误。
            return Err(invalid_argument("Legacy keyboard key must not be empty."));
        }
        // 单键映射为正式 key press。
        let step = if parts.len() == 1 {
            // 转换单键兼容形状。
            key_step(KeyStepWire {
                // 保存唯一键名。
                key: parts[0].to_owned(),
                // 兼容形状只允许 press。
                phase: KeyboardKeyPhase::Press,
                // 传播旧持续时间。
                hold_ms: wire.hold_ms,
                // 兼容形状只执行一次。
                repeat: 1,
                // 兼容形状没有重复间隔。
                interval_ms: 0,
            })?
        } else {
            // 转换快捷键兼容形状。
            chord_step(ChordStepWire {
                // 复制有序组合成员。
                keys: parts.iter().map(|part| (*part).to_owned()).collect(),
                // 传播旧持续时间。
                hold_ms: wire.hold_ms,
                // 兼容形状只执行一次。
                repeat: 1,
                // 兼容形状没有重复间隔。
                interval_ms: 0,
            })?
        };
        // 构造兼容领域对象。
        KeyboardInput {
            // 保存单个兼容步骤。
            steps: vec![step],
            // 保存 deadline。
            timeout_ms: wire.timeout_ms,
            // 标记兼容调用。
            legacy_key_compatibility: true,
        }
    };
    // 在任何目标发现前验证完整状态机。
    validate_sequence(&input)?;
    // 返回已验证请求。
    Ok(input)
}

// 声明纯契约回归。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测私有契约。
    use super::*;

    // 验证完整键集代表、快捷键、重复与 Unicode 文本被接受。
    #[test]
    fn complete_keyboard_sequence_is_accepted() -> AppResult<()> {
        // 构造覆盖主要键类和全部步骤的正式请求。
        let input = parse_keyboard_input(&json!({
            // 使用有界 deadline。
            "timeoutMs": 10_000,
            // 保存公开步骤顺序。
            "steps": [
                // 显式持有左控制键。
                { "type": "key", "key": "left-control", "phase": "down" },
                // 在修饰键期间按下功能键。
                { "type": "key", "key": "f12", "phase": "press" },
                // 配平左控制键。
                { "type": "key", "key": "left-control", "phase": "up" },
                // 覆盖有序快捷键和重复。
                { "type": "chord", "keys": ["right-control", "right-shift", "a"], "repeat": 2, "intervalMs": 10 },
                // 覆盖数字区与长按。
                { "type": "key", "key": "numpad-enter", "phase": "press", "holdMs": 20 },
                // 覆盖非 ASCII Unicode 文本。
                { "type": "text", "text": "通用🙂" }
            ]
        }))?;
        // 核对步骤数量。
        assert_eq!(input.steps.len(), 6);
        // 核对正式形状没有兼容标记。
        assert!(!input.legacy_key_compatibility);
        // 返回成功。
        Ok(())
    }

    // 验证旧 CTRL 组合被安全映射到正式 chord。
    #[test]
    fn legacy_chord_maps_to_canonical_keys() -> AppResult<()> {
        // 解析旧兼容形状。
        let input = parse_keyboard_input(&json!({ "key": "CTRL+S", "holdMs": 5 }))?;
        // 核对兼容标记。
        assert!(input.legacy_key_compatibility);
        // 核对 canonical 左控制键和小写字母。
        match &input.steps[0] {
            // 解构快捷键步骤。
            KeyboardStep::Chord { keys, .. } => {
                // 核对第一个键。
                assert_eq!(keys[0].as_str(), "left-control");
                // 核对第二个键。
                assert_eq!(keys[1].as_str(), "s");
            }
            // 其他步骤表示兼容映射错误。
            _ => panic!("legacy chord must map to a chord step"),
        }
        // 返回成功。
        Ok(())
    }

    // 验证跨请求式未闭合按下被拒绝。
    #[test]
    fn unbalanced_key_down_is_rejected() {
        // 构造未释放修饰键。
        let error = match parse_keyboard_input(&json!({
            // 只有一个 down 步骤。
            "steps": [{ "type": "key", "key": "left-alt", "phase": "down" }]
        })) {
            // 意外成功表示按键所有权校验失效。
            Ok(_) => panic!("unbalanced key down must fail"),
            // 保留预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    // 验证无所有者释放被拒绝。
    #[test]
    fn release_without_owned_key_is_rejected() {
        // 构造孤立释放。
        let error = match parse_keyboard_input(&json!({
            // 释放当前请求从未按下的键。
            "steps": [{ "type": "key", "key": "right-win", "phase": "up" }]
        })) {
            // 意外成功表示释放所有权校验失效。
            Ok(_) => panic!("unowned key release must fail"),
            // 保留预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    // 验证未知键和原生键码字段失败闭合。
    #[test]
    fn unknown_key_and_native_field_are_rejected() {
        // 构造平台键码注入尝试。
        let error = match parse_keyboard_input(&json!({
            // 同时提供未知键和原生字段。
            "steps": [{ "type": "key", "key": "vk-255", "phase": "press", "scanCode": 1 }]
        })) {
            // 意外成功表示平台字段禁止规则失效。
            Ok(_) => panic!("native keyboard fields must fail"),
            // 保留预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
