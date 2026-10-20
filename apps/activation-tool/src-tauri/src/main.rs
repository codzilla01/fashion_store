use chrono::Utc;
use naseej_license_format::{
    ActivationRequest, DeviceInfo, SignedLicense, create_request, device_info, embedded_public_key,
    validate_license_for_device,
};
use serde::Serialize;
use std::{env, fs, path::PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationStatus {
    license_id: String,
    customer_id: String,
    plan_code: Option<String>,
    expires_at_utc: String,
    features: Vec<String>,
    valid: bool,
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            create_activation_request,
            get_activation_status,
            import_activation_license,
            export_license_backup,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ActivationTool");
}

#[tauri::command]
fn get_device_info() -> Result<DeviceInfo, String> {
    device_info().map_err(|error| error.to_string())
}

#[tauri::command]
fn create_activation_request() -> Result<ActivationRequest, String> {
    create_request(env!("CARGO_PKG_VERSION")).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_activation_status() -> Result<Option<ActivationStatus>, String> {
    let path = shared_license_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let protected = fs::read(&path).map_err(|_| "LICENSE_FILE_READ_FAILED".to_string())?;
    let clear = dpapi_unprotect(&protected)?;
    status_from_bytes(&clear).map(Some)
}

#[tauri::command]
fn import_activation_license(license_json: String) -> Result<ActivationStatus, String> {
    if license_json.len() > 64 * 1024 {
        return Err("LICENSE_FILE_TOO_LARGE".to_string());
    }
    let envelope: SignedLicense =
        serde_json::from_str(&license_json).map_err(|_| "LICENSE_FORMAT_INVALID".to_string())?;
    let public_key = embedded_public_key().map_err(|error| error.to_string())?;
    let fingerprint = device_info()
        .map_err(|error| error.to_string())?
        .fingerprint;
    validate_license_for_device(&envelope, &public_key, &fingerprint, Utc::now())
        .map_err(|error| error.to_string())?;

    let bytes =
        serde_json::to_vec(&envelope).map_err(|_| "LICENSE_SERIALIZE_FAILED".to_string())?;
    let protected = dpapi_protect(&bytes)?;
    let path = shared_license_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "LICENSE_PATH_INVALID".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "LICENSE_DIR_CREATE_FAILED".to_string())?;
    let temp = parent.join(format!("license.state.{}.part", std::process::id()));
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)
            .map_err(|_| "LICENSE_TEMP_CREATE_FAILED".to_string())?;
        file.write_all(&protected)
            .map_err(|_| "LICENSE_TEMP_WRITE_FAILED".to_string())?;
        file.sync_all()
            .map_err(|_| "LICENSE_TEMP_SYNC_FAILED".to_string())?;
    }
    fs::rename(&temp, &path).map_err(|_| {
        let _ = fs::remove_file(&temp);
        "LICENSE_ATOMIC_REPLACE_FAILED".to_string()
    })?;
    status_from_bytes(&bytes)
}

#[tauri::command]
fn export_license_backup() -> Result<String, String> {
    let protected =
        fs::read(shared_license_path()?).map_err(|_| "LICENSE_FILE_NOT_FOUND".to_string())?;
    let clear = dpapi_unprotect(&protected)?;
    let envelope: SignedLicense =
        serde_json::from_slice(&clear).map_err(|_| "LICENSE_STATE_INVALID".to_string())?;
    serde_json::to_string_pretty(&envelope).map_err(|_| "LICENSE_SERIALIZE_FAILED".to_string())
}

fn status_from_bytes(bytes: &[u8]) -> Result<ActivationStatus, String> {
    let envelope: SignedLicense =
        serde_json::from_slice(bytes).map_err(|_| "LICENSE_FORMAT_INVALID".to_string())?;
    let public_key = embedded_public_key().map_err(|error| error.to_string())?;
    let fingerprint = device_info()
        .map_err(|error| error.to_string())?
        .fingerprint;
    validate_license_for_device(&envelope, &public_key, &fingerprint, Utc::now())
        .map_err(|error| error.to_string())?;
    Ok(ActivationStatus {
        license_id: envelope.payload.license_id,
        customer_id: envelope.payload.customer_id,
        plan_code: envelope.payload.plan_code,
        expires_at_utc: envelope.payload.expires_at_utc,
        features: envelope.payload.features.into_iter().collect(),
        valid: true,
    })
}

fn shared_license_path() -> Result<PathBuf, String> {
    let local = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA_UNAVAILABLE".to_string())?;
    Ok(local
        .join("NaseejPOS")
        .join("Licensing")
        .join("license.state.dpapi"))
}

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    win_dpapi::protect(data)
}
#[cfg(not(windows))]
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("WINDOWS_DPAPI_REQUIRED".to_string())
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    win_dpapi::unprotect(data)
}
#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("WINDOWS_DPAPI_REQUIRED".to_string())
}

#[cfg(windows)]
mod win_dpapi {
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
        slice,
    };
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;
    #[link(name = "Crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                null(),
                null(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI_PROTECT_FAILED".to_string());
        }
        let result =
            unsafe { slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe {
            LocalFree(output.pb_data as *mut c_void);
        }
        Ok(result)
    }
    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let mut description: *mut u16 = null_mut();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                &mut description,
                null(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI_UNPROTECT_FAILED".to_string());
        }
        let result =
            unsafe { slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe {
            LocalFree(output.pb_data as *mut c_void);
        }
        if !description.is_null() {
            unsafe {
                LocalFree(description as *mut c_void);
            }
        }
        Ok(result)
    }
}
