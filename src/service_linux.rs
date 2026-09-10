//! Linux `ComputerControlSystem` 组合根。

use serde_json::{Value, json};

use crate::{
    adapters::AdapterRegistry,
    catalog,
    domain::{AppControlError, AppResult, CommandRequest, Verb, error_json},
    methods,
    modules::{
        application_discovery, application_session_discovery, build_info, capability_assessment,
        capability_metadata,
    },
    policy,
};

// Linux 与 Windows 共享公开 Workflow 输入、绑定、条件和聚合语义。
#[path = "service/sequence.rs"]
mod sequence;
#[path = "service/sequence_binding_errors.rs"]
mod sequence_binding_errors;
#[path = "service/sequence_bindings.rs"]
mod sequence_bindings;
#[path = "service/sequence_postconditions.rs"]
mod sequence_postconditions;
#[path = "service/sequence_preconditions.rs"]
mod sequence_preconditions;
#[path = "service/sequence_templates.rs"]
mod sequence_templates;

pub use sequence::*;
pub use sequence_bindings::{SequenceBinding, SequenceBindingDestination};
pub use sequence_postconditions::SequencePostcondition;
pub use sequence_preconditions::SequencePrecondition;
pub use sequence_templates::*;

/// Linux 组合根只选择、持有并协调平台 provider。
pub struct AppControlService {
    registry: AdapterRegistry,
}

impl Default for AppControlService {
    fn default() -> Self {
        Self::new()
    }
}

impl AppControlService {
    pub fn new() -> Self {
        Self {
            registry: AdapterRegistry::adaptive(),
        }
    }

    pub fn build_info(&self) -> Value {
        build_info::render()
    }

    pub fn capability_surface(&self) -> Value {
        capability_metadata::surface()
    }

    pub fn method_capabilities(&self, method_id: Option<&str>) -> AppResult<Value> {
        let entries = methods::all()
            .iter()
            .filter(|method| method_id.is_none_or(|id| id == method.id))
            .map(|method| {
                let portal_screenshot = method.id == "xdg-desktop-portal-screenshot";
                json!({
                    "id": method.id,
                    "executionScope": method.execution_scope,
                    "availability": method.availability,
                    "summary": method.summary,
                    "cppStatus": "reference-only-on-linux",
                    "safetyBoundary": if portal_screenshot {
                        "wayland-only-visible-consent-gated-no-fallback"
                    } else {
                        "no-certified-linux-provider-no-fallback"
                    },
                })
            })
            .collect::<Vec<_>>();
        if method_id.is_some() && entries.is_empty() {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "The requested method is not in the public directory.",
            ));
        }
        Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "implementation": "rust",
            "data": {
                "readOnly": true,
                "policy": "capability-first-no-silent-fallback",
                "methods": entries,
            },
        }))
    }

    pub fn descriptor_capabilities(
        &self,
        app_id: &str,
        operation_id: Option<&str>,
    ) -> AppResult<Value> {
        let app = catalog::app(app_id).ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "The requested app is not in the catalog.",
            )
        })?;
        let mut descriptor = match operation_id {
            Some(operation_id) => {
                serde_json::to_value(catalog::operation(app_id, operation_id).ok_or_else(|| {
                    AppControlError::new(
                        "INVALID_ARGUMENT",
                        "The requested operation is not in the catalog.",
                    )
                })?)
            }
            None => serde_json::to_value(app),
        }
        .map_err(|error| AppControlError::new("SERIALIZATION_FAILED", error.to_string()))?;
        if let Some(object) = descriptor.as_object_mut() {
            if operation_id.is_some() {
                object.insert("cppStatus".to_owned(), json!("reference-only-on-linux"));
                object.insert("cppExecutionEnabled".to_owned(), json!(false));
            } else {
                object.insert(
                    "cppDirectoryStatus".to_owned(),
                    json!("reference-only-on-linux"),
                );
            }
        }
        Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "implementation": "rust",
            "data": { "readOnly": true, "descriptor": descriptor },
        }))
    }

    pub fn catalog(&self, app_id: Option<&str>) -> AppResult<Value> {
        let entries = match app_id {
            Some(id) => vec![catalog::app(id).ok_or_else(|| {
                AppControlError::new("INVALID_ARGUMENT", "Unknown application catalog entry.")
            })?],
            None => catalog::apps().iter().collect(),
        };
        Ok(json!({ "ok": true, "policy": "background-preferred", "apps": entries }))
    }

    pub fn describe(&self, app_id: &str, operation_id: Option<&str>) -> AppResult<Value> {
        let app = catalog::app(app_id).ok_or_else(|| {
            AppControlError::new("INVALID_ARGUMENT", "Unknown application catalog entry.")
        })?;
        let descriptor = match operation_id {
            Some(id) => serde_json::to_value(catalog::operation(app_id, id).ok_or_else(|| {
                AppControlError::new("INVALID_ARGUMENT", "Unknown operation catalog entry.")
            })?),
            None => serde_json::to_value(app),
        }
        .map_err(|error| AppControlError::new("SERIALIZATION_FAILED", error.to_string()))?;
        Ok(json!({ "ok": true, "descriptor": descriptor }))
    }

    pub fn methods(&self, method_id: Option<&str>) -> AppResult<Value> {
        let entries = match method_id {
            Some(id) => vec![methods::find(id).ok_or_else(|| {
                AppControlError::new("INVALID_ARGUMENT", "Unknown control method.")
            })?],
            None => methods::all().iter().collect(),
        };
        Ok(json!({ "ok": true, "methods": entries }))
    }

    pub fn doctor(&self, app_id: Option<&str>) -> Value {
        let ids = app_id.map_or_else(|| vec!["process"], |id| vec![id]);
        let results = ids
            .into_iter()
            .map(
                |id| match self.execute(CommandRequest::read(Verb::Status, id)) {
                    Ok(result) => result,
                    Err(error) => error_json(&error),
                },
            )
            .collect::<Vec<_>>();
        json!({
            "ok": results.iter().all(|result| result["ok"] == true),
            "policy": "background-preferred",
            "platform": "linux",
            "results": results,
        })
    }

    pub fn discover_app(
        &self,
        maximum_applications: usize,
        maximum_processes: usize,
        maximum_windows: usize,
    ) -> AppResult<Value> {
        application_discovery::discover(maximum_applications, maximum_processes, maximum_windows)
    }

    /// 显式执行与 application.open@2 同步发布启动状态的版本三清单。
    pub fn discover_app_v3(
        &self,
        maximum_applications: usize,
        maximum_processes: usize,
        maximum_windows: usize,
    ) -> AppResult<Value> {
        application_discovery::discover_v3(maximum_applications, maximum_processes, maximum_windows)
    }

    /// 显式执行 Linux 版本二 application session 只读聚合。
    pub fn discover_application_sessions(
        &self,
        maximum_applications: usize,
        maximum_processes: usize,
    ) -> AppResult<Value> {
        application_session_discovery::discover(maximum_applications, maximum_processes)
    }

    /// 显式执行 Linux 版本三 UIX-aware application session 只读聚合。
    pub fn discover_uix_application_sessions(
        &self,
        maximum_applications: usize,
        maximum_processes: usize,
        maximum_windows: usize,
    ) -> AppResult<Value> {
        application_session_discovery::discover_v3(
            maximum_applications,
            maximum_processes,
            maximum_windows,
        )
    }

    /// 显式执行 Linux 版本四 launch-aware UIX application session 只读聚合。
    pub fn discover_launch_aware_application_sessions(
        &self,
        maximum_applications: usize,
        maximum_processes: usize,
        maximum_windows: usize,
    ) -> AppResult<Value> {
        application_session_discovery::discover_v4(
            maximum_applications,
            maximum_processes,
            maximum_windows,
        )
    }

    pub fn assess_capability(&self, capability: &str, session_id: &str) -> AppResult<Value> {
        capability_assessment::assess(capability, session_id)
    }

    pub fn execute(&self, request: CommandRequest) -> AppResult<Value> {
        self.execute_with_dispatch_hook(request, || Ok(()))
    }

    /// 在 Linux registry 和统一高风险门禁后、provider 调用前发布一次 dispatch 事实。
    pub(crate) fn execute_with_dispatch_hook<F>(
        &self,
        request: CommandRequest,
        dispatch_hook: F,
    ) -> AppResult<Value>
    where
        F: FnOnce() -> AppResult<()>,
    {
        let adapter = self.registry.get(&request.app)?;
        // process mutation 与全部认证 Linux App capability 复用统一 catalog/Policy。
        let direct_policy =
            request.verb == Verb::Run && matches!(request.app.as_str(), "process" | "app");
        let plan = if direct_policy {
            policy::validate(&request)?;
            policy::execution_plan(&request)?
        } else {
            None
        };
        // 只有 registry、确认、前景同意和隔离门禁都通过后才能发布 accepted。
        dispatch_hook()?;
        let mut result = match request.verb {
            Verb::Status => adapter.status(),
            Verb::Sessions => adapter.sessions(&request),
            Verb::Inspect => adapter.inspect(&request),
            Verb::Run => adapter.run(&request),
        }?;
        if let Some(plan) = plan {
            plan.attest_result(&mut result)?;
        }
        Ok(result)
    }
}
