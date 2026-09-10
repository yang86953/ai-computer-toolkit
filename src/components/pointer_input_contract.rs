//! 定义 provider-neutral 指针输入与请求内按钮所有权契约。

// 导入有序集合以验证按钮状态机。
use std::collections::BTreeSet;

// 导入严格 JSON 反序列化。
use serde::Deserialize;
// 导入公开 JSON 值。
use serde_json::Value;

// 导入统一结果与错误类型。
use crate::domain::{AppControlError, AppResult};

// 固定一次请求允许的最大公开步骤数。
pub(crate) const MAXIMUM_POINTER_STEPS: usize = 64;
// 固定展开后的最大平台工作单元数。
pub(crate) const MAXIMUM_POINTER_WORK_UNITS: usize = 1_024;
// 固定同步指针请求最短 deadline。
pub(crate) const MINIMUM_POINTER_TIMEOUT_MS: u32 = 1;
// 固定同步指针请求最长 deadline。
pub(crate) const MAXIMUM_POINTER_TIMEOUT_MS: u32 = 30_000;
// 固定同步指针请求缺省 deadline。
pub(crate) const DEFAULT_POINTER_TIMEOUT_MS: u32 = 2_000;
// 固定单双击之间允许的最长间隔。
pub(crate) const MAXIMUM_CLICK_INTERVAL_MS: u32 = 500;
// 固定拖拽允许的最长持续时间。
pub(crate) const MAXIMUM_DRAG_DURATION_MS: u32 = 5_000;
// 固定一次拖拽允许的最大采样数。
pub(crate) const MAXIMUM_DRAG_SAMPLES: u16 = 240;
// 固定单步滚轮刻度绝对值上限。
pub(crate) const MAXIMUM_SCROLL_TICKS: i32 = 120;

// 表示公开坐标空间的封闭集合。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PointerCoordinateSpace {
    // 表示带符号虚拟桌面物理像素。
    ScreenPhysicalPx,
    // 表示目标窗口客户区物理像素。
    WindowClientPhysicalPx,
}

// 提供稳定公开坐标空间文本。
impl PointerCoordinateSpace {
    // 返回 schema 使用的 provider-neutral 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举两种坐标空间。
        match self {
            // 映射虚拟桌面物理像素。
            Self::ScreenPhysicalPx => "screen-physical-px",
            // 映射窗口客户区物理像素。
            Self::WindowClientPhysicalPx => "window-client-physical-px",
        }
    }
}

// 保存一个带符号二维点。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PointerPoint {
    // 保存横坐标。
    pub(crate) x: i32,
    // 保存纵坐标。
    pub(crate) y: i32,
}

// 对两个公开点执行确定性整数插值。
pub(crate) fn interpolate_pointer_point(
    // 接收起点。
    start: PointerPoint,
    // 接收终点。
    end: PointerPoint,
    // 接收当前采样索引。
    index: u16,
    // 接收总采样数。
    samples: u16,
) -> PointerPoint {
    // 使用 i64 计算横向差值避免溢出。
    let x = i64::from(start.x)
        // 按当前采样比例增加差值。
        + (i64::from(end.x) - i64::from(start.x)) * i64::from(index)
            / i64::from(samples);
    // 使用 i64 计算纵向差值避免溢出。
    let y = i64::from(start.y)
        // 按当前采样比例增加差值。
        + (i64::from(end.y) - i64::from(start.y)) * i64::from(index)
            / i64::from(samples);
    // 构造保证落在两个 i32 端点之间的结果。
    PointerPoint {
        // 安全收窄横坐标。
        x: i32::try_from(x).unwrap_or(end.x),
        // 安全收窄纵坐标。
        y: i32::try_from(y).unwrap_or(end.y),
    }
}

// 表示公开按钮的封闭集合。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PointerButton {
    // 表示主按钮。
    Left,
    // 表示次按钮。
    Right,
    // 表示中按钮。
    Middle,
}

// 提供稳定公开按钮文本。
impl PointerButton {
    // 返回 schema 使用的按钮名称。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举三种按钮。
        match self {
            // 映射左键。
            Self::Left => "left",
            // 映射右键。
            Self::Right => "right",
            // 映射中键。
            Self::Middle => "middle",
        }
    }
}

// 表示显式按钮阶段。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PointerButtonPhase {
    // 表示按下并由当前请求取得释放责任。
    Down,
    // 表示释放当前请求持有的按钮。
    Up,
}

// 表示滚轮方向轴。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PointerScrollAxis {
    // 表示垂直滚轮。
    Vertical,
    // 表示水平滚轮。
    Horizontal,
}

// 返回缺省单击次数。
const fn default_click_count() -> u8 {
    // 单击是默认点击语义。
    1
}

// 返回缺省双击间隔。
const fn default_click_interval_ms() -> u32 {
    // 使用短于系统常见双击阈值的稳定间隔。
    100
}

// 返回缺省拖拽持续时间。
const fn default_drag_duration_ms() -> u32 {
    // 使用可观察但保持有界的缺省持续时间。
    250
}

// 返回缺省拖拽采样数。
const fn default_drag_samples() -> u16 {
    // 使用足够平滑且输出固定的采样预算。
    12
}

// 表示一次请求中的 provider-neutral 指针步骤。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum PointerStep {
    // 表示移动到精确点。
    Move {
        // 保存公开坐标点。
        point: PointerPoint,
    },
    // 表示在精确点按下或释放按钮。
    Button {
        // 保存按钮类别。
        button: PointerButton,
        // 保存按钮阶段。
        phase: PointerButtonPhase,
        // 保存公开坐标点。
        point: PointerPoint,
    },
    // 表示在精确点执行单击或双击。
    Click {
        // 保存按钮类别。
        button: PointerButton,
        // 保存单击或双击次数。
        #[serde(default = "default_click_count")]
        count: u8,
        // 保存两次点击之间的等待。
        #[serde(default = "default_click_interval_ms", rename = "intervalMs")]
        interval_ms: u32,
        // 保存公开坐标点。
        point: PointerPoint,
    },
    // 表示在精确点滚动垂直或水平滚轮。
    Scroll {
        // 保存滚轮方向轴。
        axis: PointerScrollAxis,
        // 保存带方向的 provider-neutral 刻度数。
        ticks: i32,
        // 保存公开坐标点。
        point: PointerPoint,
    },
    // 表示从起点到终点的有界拖拽。
    Drag {
        // 保存拖拽按钮。
        button: PointerButton,
        // 保存拖拽起点。
        start: PointerPoint,
        // 保存拖拽终点。
        end: PointerPoint,
        // 保存拖拽持续时间。
        #[serde(default = "default_drag_duration_ms", rename = "durationMs")]
        duration_ms: u32,
        // 保存插值采样数。
        #[serde(default = "default_drag_samples")]
        samples: u16,
    },
}

// 提供步骤的稳定公开类型文本。
impl PointerStep {
    // 返回公开类型名称。
    pub(crate) const fn kind(&self) -> &'static str {
        // 穷举全部五种步骤。
        match self {
            // 映射移动步骤。
            Self::Move { .. } => "move",
            // 映射按钮步骤。
            Self::Button { .. } => "button",
            // 映射点击步骤。
            Self::Click { .. } => "click",
            // 映射滚轮步骤。
            Self::Scroll { .. } => "scroll",
            // 映射拖拽步骤。
            Self::Drag { .. } => "drag",
        }
    }

    // 估算展开后平台调用的工作单元数。
    const fn work_units(&self) -> usize {
        // 为每种步骤计算保守上界。
        match self {
            // 移动只包含一次光标变更。
            Self::Move { .. } => 1,
            // 按钮步骤包含移动与一次按钮事件。
            Self::Button { .. } => 2,
            // 点击包含移动以及每次点击的按下和释放。
            Self::Click { count, .. } => 1 + (*count as usize * 2),
            // 滚轮包含移动与一次滚轮事件。
            Self::Scroll { .. } => 2,
            // 拖拽包含起点移动、按下、采样移动和释放。
            Self::Drag { samples, .. } => 3 + *samples as usize,
        }
    }
}

// 保存通过验证的指针请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PointerInput {
    // 保存统一坐标空间。
    pub(crate) coordinate_space: PointerCoordinateSpace,
    // 保存有界动作序列。
    pub(crate) steps: Vec<PointerStep>,
    // 保存单调 deadline。
    pub(crate) timeout_ms: u32,
    // 标记是否来自旧单击兼容形状。
    pub(crate) legacy_click_compatibility: bool,
}

// 保存正式动作序列 JSON 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PointerSequenceWire {
    // 保存统一坐标空间。
    coordinate_space: PointerCoordinateSpace,
    // 保存公开步骤数组。
    steps: Vec<PointerStep>,
    // 保存可选 deadline。
    #[serde(default = "default_pointer_timeout_ms")]
    timeout_ms: u32,
}

// 保存旧 `{x,y}` 左键单击兼容形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyPointerClickWire {
    // 保存屏幕横坐标。
    x: i32,
    // 保存屏幕纵坐标。
    y: i32,
    // 保存可选 deadline。
    #[serde(default = "default_pointer_timeout_ms")]
    timeout_ms: u32,
}

// 返回 serde 使用的缺省 deadline。
const fn default_pointer_timeout_ms() -> u32 {
    // 复用公开固定常量。
    DEFAULT_POINTER_TIMEOUT_MS
}

// 构造不回显原始 JSON 的稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回统一公开错误 envelope。
    AppControlError::new("INVALID_ARGUMENT", message)
}

// 验证动作序列与请求内按钮所有权。
fn validate_sequence(input: &PointerInput) -> AppResult<()> {
    // 步骤数组必须非空且保持有界。
    if input.steps.is_empty() || input.steps.len() > MAXIMUM_POINTER_STEPS {
        // 返回固定步骤边界错误。
        return Err(invalid_argument(format!(
            "Pointer input requires 1..={MAXIMUM_POINTER_STEPS} steps."
        )));
    }
    // deadline 必须处于同步调用认证范围。
    if !(MINIMUM_POINTER_TIMEOUT_MS..=MAXIMUM_POINTER_TIMEOUT_MS)
        // 核对调用方值。
        .contains(&input.timeout_ms)
    {
        // 返回固定 deadline 边界错误。
        return Err(invalid_argument(format!(
            "Pointer input timeoutMs must be within {MINIMUM_POINTER_TIMEOUT_MS}..={MAXIMUM_POINTER_TIMEOUT_MS}."
        )));
    }
    // 保存当前请求已经按下且必须释放的按钮集合。
    let mut held = BTreeSet::new();
    // 累计展开后的平台工作预算。
    let mut work_units = 0usize;
    // 按公开顺序验证每一步。
    for step in &input.steps {
        // 累加保守工作单元数。
        work_units = work_units.saturating_add(step.work_units());
        // 超出预算时立即拒绝。
        if work_units > MAXIMUM_POINTER_WORK_UNITS {
            // 返回固定工作预算错误。
            return Err(invalid_argument(format!(
                "Pointer input expands beyond {MAXIMUM_POINTER_WORK_UNITS} work units."
            )));
        }
        // 执行步骤专属状态检查。
        match step {
            // 移动允许发生在按住按钮期间。
            PointerStep::Move { .. } => {}
            // 按钮步骤改变请求内持有集合。
            PointerStep::Button { button, phase, .. } => match phase {
                // 按下取得当前请求的释放责任。
                PointerButtonPhase::Down => {
                    // 重复按下会破坏唯一所有权。
                    if !held.insert(*button) {
                        // 返回固定重复按下错误。
                        return Err(invalid_argument(
                            "A pointer button cannot be pressed twice without an intervening release.",
                        ));
                    }
                }
                // 释放必须对应当前请求中的按下。
                PointerButtonPhase::Up => {
                    // 跨请求释放没有可证明的所有权。
                    if !held.remove(button) {
                        // 返回固定无所有者释放错误。
                        return Err(invalid_argument(
                            "A pointer button release must match an earlier press in the same request.",
                        ));
                    }
                }
            },
            // 点击宏不得嵌入未闭合按钮状态。
            PointerStep::Click {
                count, interval_ms, ..
            } => {
                // 只允许单击或双击。
                if !matches!(*count, 1 | 2) {
                    // 返回固定点击次数错误。
                    return Err(invalid_argument("Pointer click count must be 1 or 2."));
                }
                // 点击间隔保持在可取消等待边界内。
                if *interval_ms > MAXIMUM_CLICK_INTERVAL_MS {
                    // 返回固定点击间隔错误。
                    return Err(invalid_argument(format!(
                        "Pointer click intervalMs must be within 0..={MAXIMUM_CLICK_INTERVAL_MS}."
                    )));
                }
                // 禁止与显式持有按钮混用。
                if !held.is_empty() {
                    // 返回固定状态冲突错误。
                    return Err(invalid_argument(
                        "Pointer click cannot execute while this request holds a button.",
                    ));
                }
            }
            // 滚轮允许在显式按钮持有期间执行。
            PointerStep::Scroll { ticks, .. } => {
                // 零刻度没有领域效果，过大刻度破坏有界性。
                if *ticks == 0 || ticks.unsigned_abs() > MAXIMUM_SCROLL_TICKS as u32 {
                    // 返回固定滚轮边界错误。
                    return Err(invalid_argument(format!(
                        "Pointer scroll ticks must be non-zero and within -{MAXIMUM_SCROLL_TICKS}..={MAXIMUM_SCROLL_TICKS}."
                    )));
                }
            }
            // 拖拽宏自有完整按钮生命周期。
            PointerStep::Drag {
                duration_ms,
                samples,
                ..
            } => {
                // 持续时间保持在同步范围内。
                if *duration_ms > MAXIMUM_DRAG_DURATION_MS {
                    // 返回固定拖拽持续时间错误。
                    return Err(invalid_argument(format!(
                        "Pointer drag durationMs must be within 0..={MAXIMUM_DRAG_DURATION_MS}."
                    )));
                }
                // 采样必须非零且有界。
                if *samples == 0 || *samples > MAXIMUM_DRAG_SAMPLES {
                    // 返回固定拖拽采样错误。
                    return Err(invalid_argument(format!(
                        "Pointer drag samples must be within 1..={MAXIMUM_DRAG_SAMPLES}."
                    )));
                }
                // 禁止宏隐式共享已有按钮所有权。
                if !held.is_empty() {
                    // 返回固定状态冲突错误。
                    return Err(invalid_argument(
                        "Pointer drag cannot execute while this request holds a button.",
                    ));
                }
            }
        }
    }
    // 请求返回前不得遗留任何工具持有按钮。
    if !held.is_empty() {
        // 返回固定未闭合状态错误。
        return Err(invalid_argument(
            "Every pointer button press must be released in the same request.",
        ));
    }
    // 全部契约检查通过。
    Ok(())
}

// 严格解析正式动作序列或旧单击兼容形状。
pub(crate) fn parse_pointer_input(value: &Value) -> AppResult<PointerInput> {
    // 必须先取得对象形状才能选择版本化分支。
    let object = value.as_object().ok_or_else(|| {
        // 返回不含原始输入的稳定错误。
        invalid_argument("Pointer input must be an object.")
    })?;
    // `steps` 是正式动作序列的唯一判别字段。
    let input = if object.contains_key("steps") {
        // 严格反序列化正式形状。
        let wire: PointerSequenceWire = serde_json::from_value(value.clone()).map_err(|_| {
            // 不穿透 serde 可能携带的输入片段。
            invalid_argument("Pointer input does not match the pointer-input-v1 sequence schema.")
        })?;
        // 构造正式领域对象。
        PointerInput {
            // 传播坐标空间。
            coordinate_space: wire.coordinate_space,
            // 传播动作序列。
            steps: wire.steps,
            // 传播 deadline。
            timeout_ms: wire.timeout_ms,
            // 标记不是兼容调用。
            legacy_click_compatibility: false,
        }
    } else {
        // 严格反序列化旧 `{x,y}` 形状。
        let wire: LegacyPointerClickWire = serde_json::from_value(value.clone()).map_err(|_| {
            // 不穿透 serde 可能携带的输入片段。
            invalid_argument(
                "Pointer input must use the sequence schema or the legacy {x,y} click shape.",
            )
        })?;
        // 构造等价的单步左键单击。
        PointerInput {
            // 旧坐标固定解释为虚拟桌面物理像素。
            coordinate_space: PointerCoordinateSpace::ScreenPhysicalPx,
            // 映射到正式点击步骤。
            steps: vec![PointerStep::Click {
                // 旧路径固定使用左键。
                button: PointerButton::Left,
                // 旧路径固定执行单击。
                count: 1,
                // 单击不使用间隔但保持规范默认值。
                interval_ms: default_click_interval_ms(),
                // 传播旧屏幕点。
                point: PointerPoint {
                    // 传播横坐标。
                    x: wire.x,
                    // 传播纵坐标。
                    y: wire.y,
                },
            }],
            // 传播兼容 deadline。
            timeout_ms: wire.timeout_ms,
            // 标记旧兼容调用。
            legacy_click_compatibility: true,
        }
    };
    // 在任何窗口发现或输入前验证完整状态机。
    validate_sequence(&input)?;
    // 返回认证领域对象。
    Ok(input)
}

// 声明纯契约回归测试。
#[cfg(test)]
mod tests {
    // 导入被测契约。
    use super::*;
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 验证全部通用指针原语可以组成一个闭合请求。
    #[test]
    fn complete_pointer_primitive_sequence_is_accepted() -> AppResult<()> {
        // 构造包含移动、按钮、点击、滚轮和拖拽的正式输入。
        let input = parse_pointer_input(&json!({
            "coordinateSpace": "window-client-physical-px",
            "steps": [
                { "type": "move", "point": { "x": 10, "y": 10 } },
                { "type": "button", "button": "left", "phase": "down", "point": { "x": 10, "y": 10 } },
                { "type": "move", "point": { "x": 30, "y": 30 } },
                { "type": "scroll", "axis": "horizontal", "ticks": -2, "point": { "x": 30, "y": 30 } },
                { "type": "button", "button": "left", "phase": "up", "point": { "x": 30, "y": 30 } },
                { "type": "click", "button": "right", "count": 2, "intervalMs": 80, "point": { "x": 40, "y": 40 } },
                { "type": "drag", "button": "middle", "start": { "x": 50, "y": 50 }, "end": { "x": 80, "y": 80 }, "durationMs": 100, "samples": 4 }
            ],
            "timeoutMs": 3000
        }))?;
        // 核对正式坐标空间。
        assert_eq!(
            input.coordinate_space,
            PointerCoordinateSpace::WindowClientPhysicalPx
        );
        // 核对完整步骤数。
        assert_eq!(input.steps.len(), 7);
        // 核对不是旧兼容调用。
        assert!(!input.legacy_click_compatibility);
        // 完成测试。
        Ok(())
    }

    // 验证旧屏幕点继续映射为单次左键点击。
    #[test]
    fn legacy_click_shape_maps_to_bounded_sequence() -> AppResult<()> {
        // 解析旧形状。
        let input = parse_pointer_input(&json!({ "x": -20, "y": 30 }))?;
        // 核对兼容标记。
        assert!(input.legacy_click_compatibility);
        // 核对屏幕坐标语义。
        assert_eq!(
            input.coordinate_space,
            PointerCoordinateSpace::ScreenPhysicalPx
        );
        // 核对唯一点击步骤。
        assert!(matches!(
            input.steps.as_slice(),
            [PointerStep::Click {
                button: PointerButton::Left,
                count: 1,
                ..
            }]
        ));
        // 完成测试。
        Ok(())
    }

    // 验证未配对按下在任何 provider 调用前失败。
    #[test]
    fn unbalanced_button_press_is_rejected() {
        // 构造会遗留按钮的请求并显式区分结果。
        let result = parse_pointer_input(&json!({
            "coordinateSpace": "screen-physical-px",
            "steps": [
                { "type": "button", "button": "left", "phase": "down", "point": { "x": 1, "y": 1 } }
            ]
        }));
        // 提取预期错误。
        let error = match result {
            // 成功表示按钮平衡门禁失效。
            Ok(_) => panic!("unbalanced pointer button must fail"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    // 验证跨请求式释放被拒绝。
    #[test]
    fn release_without_owned_press_is_rejected() {
        // 构造无所有者释放并显式区分结果。
        let result = parse_pointer_input(&json!({
            "coordinateSpace": "screen-physical-px",
            "steps": [
                { "type": "button", "button": "right", "phase": "up", "point": { "x": 1, "y": 1 } }
            ]
        }));
        // 提取预期错误。
        let error = match result {
            // 成功表示按钮所有权门禁失效。
            Ok(_) => panic!("unowned pointer release must fail"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    // 验证未知字段不会被静默忽略。
    #[test]
    fn unknown_pointer_field_is_rejected() {
        // 构造带未登记字段的旧形状。
        let result = parse_pointer_input(&json!({ "x": 1, "y": 2, "nativeFlags": 42 }));
        // 提取预期错误。
        let error = match result {
            // 成功表示封闭字段门禁失效。
            Ok(_) => panic!("native pointer fields must fail"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
