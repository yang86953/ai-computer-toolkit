//! 定义 provider-neutral 窗口状态与几何生命周期输入契约。

// 导入严格 JSON 反序列化。
use serde::Deserialize;
// 导入公开 JSON 值。
use serde_json::Value;

// 导入统一结果与错误类型。
use crate::domain::{AppControlError, AppResult};

// 固定同步窗口生命周期请求最短 deadline。
pub(crate) const MINIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS: u32 = 1;
// 固定同步窗口生命周期请求最长 deadline。
pub(crate) const MAXIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS: u32 = 30_000;
// 固定同步窗口生命周期请求缺省 deadline。
pub(crate) const DEFAULT_WINDOW_LIFECYCLE_TIMEOUT_MS: u32 = 2_000;
// 固定公开窗口尺寸的协议上界。
pub(crate) const MAXIMUM_WINDOW_DIMENSION_PX: u32 = 65_535;

// 表示窗口外框几何使用的公开坐标空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) enum WindowLifecycleCoordinateSpace {
    // 表示带符号虚拟桌面物理像素。
    #[serde(rename = "screen-physical-px")]
    ScreenPhysicalPx,
}

// 提供稳定公开坐标空间文本。
impl WindowLifecycleCoordinateSpace {
    // 返回 schema 使用的 provider-neutral 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        // 当前版本只认证虚拟桌面物理像素。
        match self {
            // 映射虚拟桌面物理像素。
            Self::ScreenPhysicalPx => "screen-physical-px",
        }
    }
}

// 表示一次窗口状态或几何变更。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowLifecycleOperation {
    // 恢复为非最小化且非最大化状态。
    Restore,
    // 最小化精确窗口。
    Minimize,
    // 最大化精确窗口。
    Maximize,
    // 移动窗口外框左上角。
    Move {
        // 保存公开坐标空间。
        coordinate_space: WindowLifecycleCoordinateSpace,
        // 保存带符号虚拟桌面横坐标。
        x: i32,
        // 保存带符号虚拟桌面纵坐标。
        y: i32,
    },
    // 调整窗口外框尺寸。
    Resize {
        // 保存公开坐标空间与物理像素单位。
        coordinate_space: WindowLifecycleCoordinateSpace,
        // 保存外框宽度。
        width: u32,
        // 保存外框高度。
        height: u32,
    },
}

// 提供稳定动作与坐标语义。
impl WindowLifecycleOperation {
    // 返回公开动作名称。
    pub(crate) const fn action(self) -> &'static str {
        // 穷举五种窗口生命周期动作。
        match self {
            // 映射恢复动作。
            Self::Restore => "restore",
            // 映射最小化动作。
            Self::Minimize => "minimize",
            // 映射最大化动作。
            Self::Maximize => "maximize",
            // 映射移动动作。
            Self::Move { .. } => "move",
            // 映射缩放动作。
            Self::Resize { .. } => "resize",
        }
    }

    // 返回几何动作显式声明的坐标空间。
    pub(crate) const fn coordinate_space(self) -> Option<WindowLifecycleCoordinateSpace> {
        // 状态动作没有调用方几何输入。
        match self {
            // 恢复不携带几何输入。
            Self::Restore => None,
            // 最小化不携带几何输入。
            Self::Minimize => None,
            // 最大化不携带几何输入。
            Self::Maximize => None,
            // 移动传播显式坐标空间。
            Self::Move {
                coordinate_space, ..
            } => Some(coordinate_space),
            // 缩放传播显式坐标空间。
            Self::Resize {
                coordinate_space, ..
            } => Some(coordinate_space),
        }
    }
}

// 保存通过验证的窗口生命周期请求。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleInput {
    // 保存唯一窗口动作。
    pub(crate) operation: WindowLifecycleOperation,
    // 保存单调 deadline。
    pub(crate) timeout_ms: u32,
}

// 保存公开窗口生命周期 JSON 形状。
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
enum WindowLifecycleWire {
    // 恢复只接受可选 deadline。
    Restore {
        // 保存可选 deadline。
        #[serde(default = "default_window_lifecycle_timeout_ms", rename = "timeoutMs")]
        timeout_ms: u32,
    },
    // 最小化只接受可选 deadline。
    Minimize {
        // 保存可选 deadline。
        #[serde(default = "default_window_lifecycle_timeout_ms", rename = "timeoutMs")]
        timeout_ms: u32,
    },
    // 最大化只接受可选 deadline。
    Maximize {
        // 保存可选 deadline。
        #[serde(default = "default_window_lifecycle_timeout_ms", rename = "timeoutMs")]
        timeout_ms: u32,
    },
    // 移动要求完整物理像素位置。
    Move {
        // 保存显式坐标空间。
        #[serde(rename = "coordinateSpace")]
        coordinate_space: WindowLifecycleCoordinateSpace,
        // 保存带符号横坐标。
        x: i32,
        // 保存带符号纵坐标。
        y: i32,
        // 保存可选 deadline。
        #[serde(default = "default_window_lifecycle_timeout_ms", rename = "timeoutMs")]
        timeout_ms: u32,
    },
    // 缩放要求完整物理像素尺寸。
    Resize {
        // 保存显式坐标空间与尺寸单位。
        #[serde(rename = "coordinateSpace")]
        coordinate_space: WindowLifecycleCoordinateSpace,
        // 保存外框宽度。
        width: u32,
        // 保存外框高度。
        height: u32,
        // 保存可选 deadline。
        #[serde(default = "default_window_lifecycle_timeout_ms", rename = "timeoutMs")]
        timeout_ms: u32,
    },
}

// 返回 serde 使用的缺省 deadline。
const fn default_window_lifecycle_timeout_ms() -> u32 {
    // 复用公开固定常量。
    DEFAULT_WINDOW_LIFECYCLE_TIMEOUT_MS
}

// 构造不回显原始 JSON 的稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回统一公开错误 envelope。
    AppControlError::new("INVALID_ARGUMENT", message)
}

// 验证同步 deadline 与公开协议尺寸边界。
fn validate_window_lifecycle_input(input: &WindowLifecycleInput) -> AppResult<()> {
    // deadline 必须处于认证同步范围。
    if !(MINIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS..=MAXIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS)
        // 核对调用方值。
        .contains(&input.timeout_ms)
    {
        // 返回固定 deadline 边界错误。
        return Err(invalid_argument(format!(
            "Window lifecycle timeoutMs must be within {MINIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS}..={MAXIMUM_WINDOW_LIFECYCLE_TIMEOUT_MS}."
        )));
    }
    // 只有缩放具有协议级尺寸边界。
    if let WindowLifecycleOperation::Resize { width, height, .. } = input.operation {
        // 零尺寸与超出稳定协议预算的尺寸都在发现前拒绝。
        if width == 0
            // 核对高度下界。
            || height == 0
            // 核对宽度上界。
            || width > MAXIMUM_WINDOW_DIMENSION_PX
            // 核对高度上界。
            || height > MAXIMUM_WINDOW_DIMENSION_PX
        {
            // 返回固定尺寸边界错误。
            return Err(invalid_argument(format!(
                "Window resize width and height must be within 1..={MAXIMUM_WINDOW_DIMENSION_PX} physical pixels."
            )));
        }
    }
    // 全部纯契约检查通过。
    Ok(())
}

// 严格解析单个窗口状态或几何动作。
pub(crate) fn parse_window_lifecycle_input(value: &Value) -> AppResult<WindowLifecycleInput> {
    // 严格反序列化封闭动作集合。
    let wire: WindowLifecycleWire = serde_json::from_value(value.clone()).map_err(|_| {
        // 不穿透 serde 可能携带的输入片段。
        invalid_argument("Window lifecycle input does not match window-lifecycle-input-v1.")
    })?;
    // 把 wire 形状映射为领域动作。
    let input = match wire {
        // 映射恢复动作。
        WindowLifecycleWire::Restore { timeout_ms } => WindowLifecycleInput {
            // 保存恢复动作。
            operation: WindowLifecycleOperation::Restore,
            // 保存 deadline。
            timeout_ms,
        },
        // 映射最小化动作。
        WindowLifecycleWire::Minimize { timeout_ms } => WindowLifecycleInput {
            // 保存最小化动作。
            operation: WindowLifecycleOperation::Minimize,
            // 保存 deadline。
            timeout_ms,
        },
        // 映射最大化动作。
        WindowLifecycleWire::Maximize { timeout_ms } => WindowLifecycleInput {
            // 保存最大化动作。
            operation: WindowLifecycleOperation::Maximize,
            // 保存 deadline。
            timeout_ms,
        },
        // 映射移动动作。
        WindowLifecycleWire::Move {
            // 接收显式坐标空间。
            coordinate_space,
            // 接收横坐标。
            x,
            // 接收纵坐标。
            y,
            // 接收 deadline。
            timeout_ms,
        } => WindowLifecycleInput {
            // 构造移动动作。
            operation: WindowLifecycleOperation::Move {
                // 保存坐标空间。
                coordinate_space,
                // 保存横坐标。
                x,
                // 保存纵坐标。
                y,
            },
            // 保存 deadline。
            timeout_ms,
        },
        // 映射缩放动作。
        WindowLifecycleWire::Resize {
            // 接收显式坐标空间。
            coordinate_space,
            // 接收宽度。
            width,
            // 接收高度。
            height,
            // 接收 deadline。
            timeout_ms,
        } => WindowLifecycleInput {
            // 构造缩放动作。
            operation: WindowLifecycleOperation::Resize {
                // 保存坐标空间。
                coordinate_space,
                // 保存宽度。
                width,
                // 保存高度。
                height,
            },
            // 保存 deadline。
            timeout_ms,
        },
    };
    // 在任何窗口发现或平台调用前验证完整请求。
    validate_window_lifecycle_input(&input)?;
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

    // 验证五种通用窗口动作都能被严格解析。
    #[test]
    fn all_window_lifecycle_actions_are_accepted() -> AppResult<()> {
        // 构造五种合法输入。
        let values = [
            // 使用缺省 deadline 恢复窗口。
            json!({ "action": "restore" }),
            // 最小化窗口。
            json!({ "action": "minimize", "timeoutMs": 1 }),
            // 最大化窗口。
            json!({ "action": "maximize", "timeoutMs": 30_000 }),
            // 使用负坐标跨越虚拟桌面原点。
            json!({ "action": "move", "coordinateSpace": "screen-physical-px", "x": -1920, "y": 20 }),
            // 使用物理像素调整外框尺寸。
            json!({ "action": "resize", "coordinateSpace": "screen-physical-px", "width": 800, "height": 600 }),
        ];
        // 按公开顺序收集动作名称。
        let actions = values
            // 遍历所有输入。
            .iter()
            // 严格解析每个输入。
            .map(parse_window_lifecycle_input)
            // 把领域动作映射为稳定文本。
            .map(|result| result.map(|input| input.operation.action()))
            // 收集并传播错误。
            .collect::<AppResult<Vec<_>>>()?;
        // 核对完整动作集合。
        assert_eq!(
            actions,
            ["restore", "minimize", "maximize", "move", "resize"]
        );
        // 核对缺省 deadline。
        assert_eq!(
            parse_window_lifecycle_input(&values[0])?.timeout_ms,
            DEFAULT_WINDOW_LIFECYCLE_TIMEOUT_MS
        );
        // 完成测试。
        Ok(())
    }

    // 验证移动保留带符号虚拟桌面坐标。
    #[test]
    fn move_preserves_signed_physical_coordinates() -> AppResult<()> {
        // 解析负坐标移动。
        let input = parse_window_lifecycle_input(&json!({
            "action": "move",
            "coordinateSpace": "screen-physical-px",
            "x": -10,
            "y": -20
        }))?;
        // 核对完整移动载荷。
        assert_eq!(
            input.operation,
            WindowLifecycleOperation::Move {
                // 核对坐标空间。
                coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
                // 核对横坐标。
                x: -10,
                // 核对纵坐标。
                y: -20,
            }
        );
        // 核对公开坐标名称。
        assert_eq!(
            input
                .operation
                .coordinate_space()
                .map(|space| space.as_str()),
            Some("screen-physical-px")
        );
        // 完成测试。
        Ok(())
    }

    // 验证零尺寸和超出协议预算的尺寸失败闭合。
    #[test]
    fn invalid_resize_dimensions_are_rejected() {
        // 构造两个非法尺寸。
        let values = [
            // 拒绝零宽度。
            json!({ "action": "resize", "coordinateSpace": "screen-physical-px", "width": 0, "height": 600 }),
            // 拒绝超出协议上界的高度。
            json!({ "action": "resize", "coordinateSpace": "screen-physical-px", "width": 800, "height": 65_536 }),
        ];
        // 逐项验证稳定参数错误。
        for value in values {
            // 解析非法输入并显式区分结果。
            let error = match parse_window_lifecycle_input(&value) {
                // 成功表示尺寸门禁失效。
                Ok(_) => panic!("invalid resize dimensions must fail"),
                // 保存预期错误。
                Err(error) => error,
            };
            // 核对统一参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }

    // 验证未知平台字段和未认证坐标空间不会被静默接受。
    #[test]
    fn native_fields_and_unknown_coordinate_spaces_are_rejected() {
        // 构造带平台字段的状态动作。
        let native = json!({ "action": "restore", "hwnd": 42 });
        // 构造未认证逻辑坐标空间。
        let logical = json!({
            "action": "move",
            "coordinateSpace": "logical-dip",
            "x": 1,
            "y": 2
        });
        // 两类输入都必须失败闭合。
        for value in [native, logical] {
            // 解析非法输入并显式区分结果。
            let error = match parse_window_lifecycle_input(&value) {
                // 成功表示封闭字段门禁失效。
                Ok(_) => panic!("native or unknown coordinate input must fail"),
                // 保存预期错误。
                Err(error) => error,
            };
            // 核对统一参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }

    // 验证 deadline 在任何目标发现前保持严格有界。
    #[test]
    fn timeout_outside_synchronous_bounds_is_rejected() {
        // 构造超出上界的恢复请求。
        let value = json!({ "action": "restore", "timeoutMs": 30_001 });
        // 解析非法输入并显式区分结果。
        let error = match parse_window_lifecycle_input(&value) {
            // 成功表示 deadline 门禁失效。
            Ok(_) => panic!("out-of-range lifecycle timeout must fail"),
            // 保存预期错误。
            Err(error) => error,
        };
        // 核对统一参数错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
