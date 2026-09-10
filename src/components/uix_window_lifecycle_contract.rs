//! 冻结 UIX 协作式窗口生命周期版本二输入。

use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_CLIENT_DIMENSION: u32 = 65_535;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 表示 UIX resize 唯一认证的客户区坐标空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) enum UixWindowCoordinateSpace {
    /// 使用 UIX 跨平台布局与客户区的 logical px。
    #[serde(rename = "client-logical-px")]
    ClientLogicalPx,
}

impl UixWindowCoordinateSpace {
    /// 返回稳定公开坐标空间名称。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClientLogicalPx => "client-logical-px",
        }
    }
}

/// 表示 Linux UIX Provider 能完整承接的窗口动作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum UixWindowLifecycleAction {
    Restore {},
    Minimize {},
    Maximize {},
    Resize {
        #[serde(rename = "coordinateSpace")]
        coordinate_space: UixWindowCoordinateSpace,
        width: u32,
        height: u32,
    },
}

impl UixWindowLifecycleAction {
    /// 返回 provider-neutral 动作名。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Restore {} => "restore",
            Self::Minimize {} => "minimize",
            Self::Maximize {} => "maximize",
            Self::Resize { .. } => "resize",
        }
    }

    /// 返回 UIX Agent hello 与 perform 使用的动作名。
    pub(crate) const fn provider_action(self) -> &'static str {
        match self {
            Self::Restore {} => "restore_window",
            Self::Minimize {} => "minimize_window",
            Self::Maximize {} => "maximize_window",
            Self::Resize { .. } => "resize_window",
        }
    }

    /// 生成仅供认证 Agent Adapter 使用的动作对象。
    pub(crate) fn provider_value(self) -> Value {
        match self {
            Self::Restore {} => json!({ "kind": "restore_window" }),
            Self::Minimize {} => json!({ "kind": "minimize_window" }),
            Self::Maximize {} => json!({ "kind": "maximize_window" }),
            Self::Resize { width, height, .. } => {
                json!({ "kind": "resize_window", "width": width, "height": height })
            }
        }
    }

    /// 生成不含 provider 身份的公开生命周期动作值。
    pub(crate) fn public_value(self) -> Value {
        match self {
            Self::Restore {} => json!({ "type": "restore" }),
            Self::Minimize {} => json!({ "type": "minimize" }),
            Self::Maximize {} => json!({ "type": "maximize" }),
            Self::Resize {
                coordinate_space,
                width,
                height,
            } => json!({
                "type": "resize",
                "coordinateSpace": coordinate_space.as_str(),
                "width": width,
                "height": height,
            }),
        }
    }

    /// 返回 resize 的公开 logical 客户区请求。
    pub(crate) const fn requested_size(self) -> Option<(u32, u32)> {
        match self {
            Self::Resize { width, height, .. } => Some((width, height)),
            _ => None,
        }
    }

    /// 返回 resize 显式坐标空间。
    pub(crate) const fn coordinate_space(self) -> Option<UixWindowCoordinateSpace> {
        match self {
            Self::Resize {
                coordinate_space, ..
            } => Some(coordinate_space),
            _ => None,
        }
    }

    /// 复用生命周期动作唯一的尺寸与坐标约束。
    pub(crate) fn validate(self) -> Result<(), &'static str> {
        match self {
            Self::Resize { width, height, .. }
                if !(1..=MAXIMUM_CLIENT_DIMENSION).contains(&width)
                    || !(1..=MAXIMUM_CLIENT_DIMENSION).contains(&height) =>
            {
                Err("UIX window lifecycle resize is outside its bounded contract.")
            }
            _ => Ok(()),
        }
    }
}

/// 保存通过严格验证的版本二生命周期请求。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowLifecycleInput {
    pub(crate) action: UixWindowLifecycleAction,
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u32,
}

impl UixWindowLifecycleInput {
    /// 严格解析输入且不回显原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX window lifecycle input violates schema://window/lifecycle/v2.")?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.action.validate().is_err()
        {
            return Err("UIX window lifecycle input is outside its bounded contract.");
        }
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn lifecycle_v2_is_logical_bounded_and_excludes_wayland_move() {
        let input = UixWindowLifecycleInput::parse(&json!({
            "action": {
                "type": "resize",
                "coordinateSpace": "client-logical-px",
                "width": 800,
                "height": 600
            }
        }))
        .expect("valid logical resize must parse");
        assert_eq!(input.action.provider_action(), "resize_window");
        assert_eq!(input.action.requested_size(), Some((800, 600)));
        assert_eq!(input.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(
            UixWindowLifecycleInput::parse(&json!({
                "action": { "type": "move", "x": 1, "y": 2 }
            }))
            .is_err()
        );
        assert!(
            UixWindowLifecycleInput::parse(&json!({
                "action": {
                    "type": "resize",
                    "coordinateSpace": "screen-physical-px",
                    "width": 800,
                    "height": 600
                }
            }))
            .is_err()
        );
    }
}
