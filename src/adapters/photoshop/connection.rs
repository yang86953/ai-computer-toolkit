use serde_json::Value;
use windows::{
    Win32::System::{
        Com::{
            CLSIDFromProgID, COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
            DISPATCH_METHOD, DISPPARAMS, IDispatch,
        },
        Ole::GetActiveObject,
        Variant::VARIANT,
    },
    core::{BSTR, GUID, Interface, PCWSTR, w},
};

use crate::domain::{AppControlError, AppResult};

pub(super) fn photoshop_installed() -> bool {
    unsafe { CLSIDFromProgID(w!("Photoshop.Application")) }.is_ok()
}

pub(super) fn run_script_json(script: String) -> AppResult<Value> {
    run_com_task(move || {
        let connection = PhotoshopConnection::attach()?.ok_or_else(|| {
            AppControlError::new(
                "APPLICATION_NOT_RUNNING",
                "No running structured image application is available.",
            )
        })?;
        let text = connection.do_javascript(&script)?;
        parse_script_json(&text)
    })
}

pub(super) fn parse_script_json(text: &str) -> AppResult<Value> {
    serde_json::from_str(text)
        .map_err(|error| AppControlError::new("OPERATION_FAILED", error.to_string()))
}

/// 同步执行 COM，确保返回后不再有遗留写线程继续修改文档。
///
/// COM 无法安全取消时，阻塞比“返回 TIMEOUT 后仍继续变更”更可预测。
pub(super) fn run_com_task<T, F>(operation: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T>,
{
    operation()
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> AppResult<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|error| {
                AppControlError::new("COM_INITIALIZATION_FAILED", error.to_string())
            })?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) struct PhotoshopConnection {
    _apartment: ComApartment,
    dispatch: IDispatch,
}

impl PhotoshopConnection {
    pub(super) fn attach() -> AppResult<Option<Self>> {
        let apartment = ComApartment::initialize()?;
        let clsid = unsafe { CLSIDFromProgID(w!("Photoshop.Application")) }.map_err(|error| {
            AppControlError::new("APPLICATION_NOT_INSTALLED", error.to_string())
        })?;
        let mut unknown = None;
        match unsafe { GetActiveObject(&clsid, None, &mut unknown) } {
            Ok(()) => {
                let dispatch = unknown
                    .ok_or_else(|| {
                        AppControlError::new("OPERATION_FAILED", "COM returned no active object.")
                    })?
                    .cast::<IDispatch>()
                    .map_err(|error| AppControlError::new("OPERATION_FAILED", error.to_string()))?;
                Ok(Some(Self {
                    _apartment: apartment,
                    dispatch,
                }))
            }
            Err(error) if error.code().0 as u32 == 0x8004_01E3 => Ok(None),
            Err(error) => Err(AppControlError::new("OPERATION_FAILED", error.to_string())),
        }
    }

    pub(super) fn do_javascript(&self, script: &str) -> AppResult<String> {
        let name = BSTR::from("DoJavaScript");
        let names = [PCWSTR(name.as_ptr())];
        let mut dispid = 0_i32;
        unsafe {
            self.dispatch
                .GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, 0, &mut dispid)
        }
        .map_err(|error| AppControlError::new("OPERATION_FAILED", error.to_string()))?;

        let mut args = [VARIANT::from(BSTR::from(script))];
        let params = DISPPARAMS {
            rgvarg: args.as_mut_ptr(),
            rgdispidNamedArgs: std::ptr::null_mut(),
            cArgs: 1,
            cNamedArgs: 0,
        };
        let mut result = VARIANT::default();
        unsafe {
            self.dispatch.Invoke(
                dispid,
                &GUID::zeroed(),
                0,
                DISPATCH_METHOD,
                &params,
                Some(&mut result),
                None,
                None,
            )
        }
        .map_err(|error| AppControlError::new("OPERATION_FAILED", error.to_string()))?;
        BSTR::try_from(&result)
            .map(|value| value.to_string())
            .map_err(|error| AppControlError::new("OPERATION_FAILED", error.to_string()))
    }
}
