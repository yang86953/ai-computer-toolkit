//! UIX 精确窗口截图输入契约 Component。

use std::{path::Path, time::Duration};

use serde_json::Value;

use crate::domain::{AppControlError, AppResult};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MINIMUM_TIMEOUT: Duration = Duration::from_secs(1);
const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(30);
const MAXIMUM_PATH_BYTES: usize = 32_767;

/// 完成严格字段校验后的截图请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UixWindowScreenshotInput {
    path: String,
    timeout: Duration,
    overwrite: bool,
}

impl UixWindowScreenshotInput {
    pub(crate) fn parse(value: &Value) -> AppResult<Self> {
        let object = value.as_object().ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX window screenshot input must be an object.",
            )
        })?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "path" | "timeoutMs" | "overwrite"))
        {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX window screenshot accepts path, timeoutMs and overwrite only.",
            ));
        }
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| (5..=MAXIMUM_PATH_BYTES).contains(&path.len()))
            .ok_or_else(|| {
                AppControlError::new(
                    "INVALID_OUTPUT_PATH",
                    "UIX window screenshot requires a bounded UTF-8 .png path.",
                )
            })?;
        if Path::new(path).extension().and_then(|value| value.to_str()) != Some("png") {
            return Err(AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "UIX window screenshot output must use the lowercase .png extension.",
            ));
        }
        let timeout = match object.get("timeoutMs") {
            None => DEFAULT_TIMEOUT,
            Some(value) => value.as_u64().map(Duration::from_millis).ok_or_else(|| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "UIX window screenshot timeoutMs must be an integer.",
                )
            })?,
        };
        if !(MINIMUM_TIMEOUT..=MAXIMUM_TIMEOUT).contains(&timeout) {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX window screenshot timeoutMs must be 1000..30000ms.",
            ));
        }
        let overwrite = match object.get("overwrite") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "UIX window screenshot overwrite must be a boolean.",
                )
            })?,
        };
        Ok(Self {
            path: path.to_owned(),
            timeout,
            overwrite,
        })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn overwrite(&self) -> bool {
        self.overwrite
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn strict_input_defaults_and_bounds_are_stable() {
        let Ok(input) = UixWindowScreenshotInput::parse(&json!({ "path": "result.png" })) else {
            panic!("有效截图输入必须成功解析");
        };
        assert_eq!(input.timeout(), Duration::from_secs(30));
        assert!(!input.overwrite());
        assert!(UixWindowScreenshotInput::parse(&json!({ "path": "result.PNG" })).is_err());
        assert!(
            UixWindowScreenshotInput::parse(&json!({
                "path": "result.png",
                "unknown": true
            }))
            .is_err()
        );
    }
}
