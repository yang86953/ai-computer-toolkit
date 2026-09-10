use std::path::Path;

use serde::Deserialize;

use crate::{
    // 导入统一单文件覆盖门禁。
    components::output_guard::{OutputGuardError, guard_file_output},
    // 导入结构化图像领域请求与错误。
    domain::{AppControlError, AppResult, CommandRequest},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CanvasInput {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) resolution: f64,
    pub(super) name: String,
}

impl CanvasInput {
    pub(super) fn validate(&self) -> AppResult<()> {
        if !(64..=20_000).contains(&self.width)
            || !(64..=20_000).contains(&self.height)
            || !(36.0..=1_200.0).contains(&self.resolution)
            || self.name.trim().is_empty()
            || self.name.chars().count() > 128
        {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "Canvas width/height, resolution, or name is outside the certified range.",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct LayersInput {
    operations: Vec<LayerOperation>,
}

impl LayersInput {
    pub(super) fn validate(&self) -> AppResult<()> {
        if self.operations.is_empty() || self.operations.len() > 128 {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "image.layers.apply@1 requires 1..=128 operations.",
            ));
        }
        self.operations
            .iter()
            .try_for_each(LayerOperation::validate)
    }

    pub(super) fn script(&self) -> AppResult<String> {
        let mut snippets = String::new();
        for operation in &self.operations {
            snippets.push_str(&operation.script()?);
        }
        Ok(snippets)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
enum LayerOperation {
    #[serde(rename = "layer.remove")]
    Remove {
        #[serde(rename = "layerId")]
        layer_id: i64,
    },
    #[serde(rename = "layer.addRect")]
    AddRect {
        name: String,
        bounds: Bounds,
        fill: String,
        #[serde(default = "default_opacity")]
        opacity: f64,
        #[serde(default)]
        rotation: f64,
    },
    #[serde(rename = "layer.addPolygon")]
    AddPolygon {
        name: String,
        points: Vec<Point>,
        fill: String,
        #[serde(default = "default_opacity")]
        opacity: f64,
    },
    #[serde(rename = "layer.addText")]
    AddText {
        name: String,
        text: String,
        position: Point,
        #[serde(rename = "fontSizePt")]
        font_size_pt: f64,
        color: String,
        #[serde(default)]
        font: Option<String>,
        #[serde(default)]
        justification: TextJustification,
        #[serde(default)]
        tracking: i32,
        #[serde(default)]
        rotation: f64,
        #[serde(default = "default_opacity")]
        opacity: f64,
    },
}

#[derive(Debug, Deserialize)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextJustification {
    #[default]
    Left,
    Center,
    Right,
}

impl LayerOperation {
    fn validate(&self) -> AppResult<()> {
        let valid_name = |name: &str| !name.trim().is_empty() && name.chars().count() <= 128;
        let valid_number = |number: f64| number.is_finite() && number.abs() <= 100_000.0;
        let valid_opacity = |opacity: f64| opacity.is_finite() && (0.0..=100.0).contains(&opacity);
        let valid = match self {
            Self::Remove { layer_id } => *layer_id > 0,
            Self::AddRect {
                name,
                bounds,
                fill,
                opacity,
                rotation,
            } => {
                valid_name(name)
                    && valid_number(bounds.x)
                    && valid_number(bounds.y)
                    && valid_number(bounds.width)
                    && valid_number(bounds.height)
                    && bounds.width > 0.0
                    && bounds.height > 0.0
                    && valid_color(fill)
                    && valid_opacity(*opacity)
                    && valid_number(*rotation)
            }
            Self::AddPolygon {
                name,
                points,
                fill,
                opacity,
            } => {
                valid_name(name)
                    && (3..=64).contains(&points.len())
                    && points
                        .iter()
                        .all(|point| valid_number(point.x) && valid_number(point.y))
                    && valid_color(fill)
                    && valid_opacity(*opacity)
            }
            Self::AddText {
                name,
                text,
                position,
                font_size_pt,
                color,
                font,
                tracking,
                rotation,
                opacity,
                ..
            } => {
                valid_name(name)
                    && !text.is_empty()
                    && text.chars().count() <= 2_000
                    && valid_number(position.x)
                    && valid_number(position.y)
                    && font_size_pt.is_finite()
                    && (1.0..=2_000.0).contains(font_size_pt)
                    && valid_color(color)
                    && !font
                        .as_ref()
                        .is_some_and(|value| value.is_empty() || value.chars().count() > 128)
                    && (-10_000..=10_000).contains(tracking)
                    && valid_number(*rotation)
                    && valid_opacity(*opacity)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "A layer operation contains invalid identity, geometry, typography, color, or opacity.",
            ))
        }
    }

    fn script(&self) -> AppResult<String> {
        match self {
            Self::Remove { layer_id } => Ok(format!("removeLayer({layer_id});\n")),
            Self::AddRect {
                name,
                bounds,
                fill,
                opacity,
                rotation,
            } => Ok(format!(
                "addRect({}, {}, {}, {}, {}, {}, {}, {});\n",
                js_string(name)?,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                js_string(fill)?,
                opacity,
                rotation,
            )),
            Self::AddPolygon {
                name,
                points,
                fill,
                opacity,
            } => {
                let points = points
                    .iter()
                    .map(|point| format!("[{},{}]", point.x, point.y))
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(format!(
                    "addPolygon({}, [{}], {}, {});\n",
                    js_string(name)?,
                    points,
                    js_string(fill)?,
                    opacity,
                ))
            }
            Self::AddText {
                name,
                text,
                position,
                font_size_pt,
                color,
                font,
                justification,
                tracking,
                rotation,
                opacity,
            } => {
                let justification = match justification {
                    TextJustification::Left => "left",
                    TextJustification::Center => "center",
                    TextJustification::Right => "right",
                };
                Ok(format!(
                    "addText({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {});\n",
                    js_string(name)?,
                    js_string(text)?,
                    position.x,
                    position.y,
                    font_size_pt,
                    js_string(color)?,
                    font.as_deref()
                        .map(js_string)
                        .transpose()?
                        .unwrap_or_else(|| "null".to_owned()),
                    js_string(justification)?,
                    tracking,
                    rotation,
                    opacity,
                ))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SaveInput {
    pub(super) path: String,
    #[serde(default)]
    pub(super) overwrite: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExportInput {
    pub(super) path: String,
    pub(super) format: String,
    #[serde(default)]
    pub(super) overwrite: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CloseInput {
    pub(super) save_changes: bool,
}

pub(super) fn parse_input<T: for<'de> Deserialize<'de>>(request: &CommandRequest) -> AppResult<T> {
    let input = request
        .args
        .get("input")
        .cloned()
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "args.input is required."))?;
    serde_json::from_value(input)
        .map_err(|error| AppControlError::new("INVALID_ARGUMENT", error.to_string()))
}

pub(super) fn validate_output_path(path: &str, extension: &str, overwrite: bool) -> AppResult<()> {
    let path = Path::new(path);
    if !path.is_absolute()
        || path.extension().and_then(|value| value.to_str()) != Some(extension)
        || !path.parent().is_some_and(Path::is_dir)
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            format!("Output must be an absolute .{extension} path with an existing parent."),
        ));
    }
    // 在 COM dispatch 前统一检查非跟随目标状态与覆盖许可。
    guard_file_output(path, overwrite).map_err(structured_output_guard_error)?;
    Ok(())
}

// 把 Component 错误映射为结构化图像领域错误。
fn structured_output_guard_error(error: OutputGuardError) -> AppControlError {
    // 保持覆盖、类型和检查失败可以区分。
    match error {
        // 既有 PSD 或 PNG 缺少覆盖许可。
        OutputGuardError::ConfirmationRequired => AppControlError::new(
            // 使用公开稳定覆盖错误码。
            "OVERWRITE_CONFIRMATION_REQUIRED",
            // 保持 provider 既有调用提示。
            "The output exists; set input.overwrite=true to replace it.",
        ),
        // 目录、链接或特殊目标不得传给应用 writer。
        OutputGuardError::InvalidTargetType => AppControlError::new(
            // 使用参数错误表示目标类型不满足契约。
            "INVALID_ARGUMENT",
            // 不公开具体输出路径。
            "The existing output is not a replaceable regular file.",
        ),
        // 目标状态无法可靠检查时必须失败闭合。
        OutputGuardError::InspectionFailed => AppControlError::new(
            // 保持公开封闭 envelope 内的操作失败码。
            "OPERATION_FAILED",
            // 不泄漏原生文件系统错误。
            "The output path could not be inspected safely.",
        ),
    }
}

pub(super) fn verify_non_empty_file(path: &str) -> AppResult<()> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| AppControlError::new("OUTPUT_VERIFICATION_FAILED", error.to_string()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(AppControlError::new(
            "OUTPUT_VERIFICATION_FAILED",
            "The provider returned success but the output file is empty or not a file.",
        ));
    }
    Ok(())
}

pub(super) fn js_string(value: &str) -> AppResult<String> {
    serde_json::to_string(value)
        .map_err(|error| AppControlError::new("INVALID_ARGUMENT", error.to_string()))
}

const fn default_opacity() -> f64 {
    100.0
}

// 编译不触达 COM 的结构化输出路径测试。
#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
