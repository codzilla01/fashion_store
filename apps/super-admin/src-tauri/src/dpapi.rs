//! The Windows DPAPI wrapper of the signing key.
//!
//! The vault of the developer workstation is bound to the Windows account
//! that created it. The key is wrapped with `CryptProtectData` in the scope
//! of the current user, so copying `vault.bin` to another machine or to
//! another Windows account does not open it: the copy is worthless without
//! the master key of that account.
//!
//! The customer application never uses this module. It carries the public
//! key only, so the POS installer can never be turned into a signer.

/// The entropy that binds a protected blob to this application.
///
/// DPAPI mixes this value into the protection, so a blob of another program
/// cannot be unprotected with the same account.
pub const ENTROPY: &[u8] = b"naseej-super-admin-vault-v2";

/// Wrap bytes with DPAPI in the scope of the current Windows user.
#[cfg(windows)]
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    windows::protect_windows(data)
}

/// Unwrap bytes that were wrapped by [`protect`].
#[cfg(windows)]
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    windows::unprotect_windows(data)
}

/// Wrap bytes with DPAPI in the scope of the current Windows user.
#[cfg(not(windows))]
pub fn protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("VAULT_DPAPI_UNAVAILABLE".to_string())
}

/// Unwrap bytes that were wrapped by [`protect`].
#[cfg(not(windows))]
pub fn unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("VAULT_DPAPI_UNAVAILABLE".to_string())
}

#[cfg(windows)]
mod windows {
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

    /// Build a blob that points at the given bytes.
    fn blob(data: &[u8]) -> DataBlob {
        DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        }
    }

    /// Take the bytes out of a blob that Windows allocated.
    fn take(owned: DataBlob) -> Vec<u8> {
        let bytes =
            unsafe { slice::from_raw_parts(owned.pb_data, owned.cb_data as usize).to_vec() };
        unsafe {
            LocalFree(owned.pb_data as *mut c_void);
        }
        bytes
    }

    /// Protect bytes with the account of the current user.
    pub(super) fn protect_windows(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(super::ENTROPY);
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                null(),
                &entropy,
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("VAULT_DPAPI_PROTECT_FAILED".to_string());
        }
        Ok(take(output))
    }

    /// Unprotect bytes with the account of the current user.
    pub(super) fn unprotect_windows(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let entropy = blob(super::ENTROPY);
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let mut description: *mut u16 = null_mut();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                &mut description,
                &entropy,
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if !description.is_null() {
            unsafe {
                LocalFree(description as *mut c_void);
            }
        }
        if ok == 0 {
            return Err("VAULT_DPAPI_UNPROTECT_FAILED".to_string());
        }
        Ok(take(output))
    }
}
