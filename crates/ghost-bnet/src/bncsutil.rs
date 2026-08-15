use std::ffi::CString;
use std::os::raw::{c_char, c_uint};
use std::path::Path;
use std::sync::OnceLock;

pub mod mpq_num;
pub mod xsha1;
pub mod cdkey;

use libloading::{Library, Symbol};

pub struct BncsUtil {
    lib: Library,
}

static BNCSUTIL: OnceLock<Option<BncsUtil>> = OnceLock::new();

impl BncsUtil {
    pub fn global() -> Option<&'static BncsUtil> {
        BNCSUTIL.get_or_init(|| {
            let candidates = [
                "bncsutil.dll",
                "libbncsutil.so",
                "libbncsutil.dylib",
                "./bncsutil.dll",
                "./libbncsutil.so",
                "/usr/lib/libbncsutil.so",
                "/usr/local/lib/libbncsutil.so",
            ];

            for path in candidates {
                if Path::new(path).exists() {
                    if let Ok(lib) = unsafe { Library::new(path) } {
                        tracing::info!(path, "loaded bncsutil library successfully");
                        return Some(BncsUtil { lib });
                    }
                }
            }

            // Also try system search
            #[cfg(windows)]
            if let Ok(lib) = unsafe { Library::new("bncsutil.dll") } {
                return Some(BncsUtil { lib });
            }

            #[cfg(unix)]
            if let Ok(lib) = unsafe { Library::new("libbncsutil.so") } {
                return Some(BncsUtil { lib });
            }

            tracing::warn!("bncsutil library not found; CD-key verification will use fallback");
            None
        }).as_ref()
    }

    pub fn extract_mpq_number(&self, mpq_name: &str) -> Option<i32> {
        let func: Symbol<unsafe extern "C" fn(*const c_char) -> i32> =
            unsafe { self.lib.get(b"extractMPQNumber").ok()? };
        let c_name = CString::new(mpq_name).ok()?;
        Some(unsafe { func(c_name.as_ptr()) })
    }

    pub fn get_exe_info(&self, file_name: &str, platform: u32) -> Option<(String, u32)> {
        let func: Symbol<
            unsafe extern "C" fn(
                *const c_char,
                *mut c_char,
                c_uint,
                *mut c_uint,
                c_uint,
            ) -> c_uint,
        > = unsafe { self.lib.get(b"getExeInfo").ok()? };

        let c_file = CString::new(file_name).ok()?;
        let mut exe_info = vec![0u8; 1024];
        let mut version = 0u32;

        unsafe {
            func(
                c_file.as_ptr(),
                exe_info.as_mut_ptr() as *mut c_char,
                256,
                &mut version as *mut c_uint,
                platform,
            );
        }

        let exe_info_str = String::from_utf8_lossy(&exe_info)
            .trim_end_matches('\0')
            .replace("/110", "/10");
        Some((exe_info_str, version))
    }

    pub fn check_revision_flat(
        &self,
        value_string: &str,
        file1: &str,
        file2: &str,
        file3: &str,
        mpq_number: i32,
    ) -> Option<u32> {
        let func: Symbol<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                i32,
                *mut c_uint,
            ) -> c_uint,
        > = unsafe { self.lib.get(b"checkRevisionFlat").ok()? };

        let c_value = CString::new(value_string).ok()?;
        let c_f1 = CString::new(file1).ok()?;
        let c_f2 = CString::new(file2).ok()?;
        let c_f3 = CString::new(file3).ok()?;
        let mut checksum = 0u32;

        unsafe {
            func(
                c_value.as_ptr(),
                c_f1.as_ptr(),
                c_f2.as_ptr(),
                c_f3.as_ptr(),
                mpq_number,
                &mut checksum as *mut c_uint,
            );
        }

        Some(checksum)
    }

    pub fn kd_quick(
        &self,
        cd_key: &str,
        client_token: u32,
        server_token: u32,
    ) -> Option<(u32, u32, [u8; 20])> {
        let func: Symbol<
            unsafe extern "C" fn(
                *const c_char,
                c_uint,
                c_uint,
                *mut c_uint,
                *mut c_uint,
                *mut c_char,
                c_uint,
            ) -> c_uint,
        > = unsafe { self.lib.get(b"kd_quick").ok()? };

        let c_cd_key = CString::new(cd_key).ok()?;
        let mut public_value = 0u32;
        let mut product = 0u32;
        let mut hash_buf = [0u8; 256];

        let res = unsafe {
            func(
                c_cd_key.as_ptr(),
                client_token,
                server_token,
                &mut public_value as *mut c_uint,
                &mut product as *mut c_uint,
                hash_buf.as_mut_ptr() as *mut c_char,
                hash_buf.len() as c_uint,
            )
        };

        if res == 0 {
            // Error in kd_quick
            return None;
        }

        let mut hash = [0u8; 20];
        hash.copy_from_slice(&hash_buf[..20]);
        Some((public_value, product, hash))
    }

    pub fn hash_password(&self, password: &str) -> Option<[u8; 20]> {
        let func: Symbol<unsafe extern "C" fn(*const c_char, *mut c_char)> =
            unsafe { self.lib.get(b"hashPassword").ok()? };
        let c_password = CString::new(password).ok()?;
        let mut buffer = [0u8; 20];
        unsafe {
            func(c_password.as_ptr(), buffer.as_mut_ptr() as *mut c_char);
        }
        Some(buffer)
    }

    pub fn nls_init(&self, username: &str, password: &str) -> Option<usize> {
        let func: Symbol<
            unsafe extern "C" fn(*const c_char, c_uint, *const c_char, c_uint) -> *mut std::ffi::c_void,
        > = unsafe { self.lib.get(b"nls_init_l").ok()? };
        let c_username = CString::new(username).ok()?;
        let c_password = CString::new(password).ok()?;
        let result = unsafe {
            func(
                c_username.as_ptr(),
                username.len() as c_uint,
                c_password.as_ptr(),
                password.len() as c_uint,
            )
        };
        if result.is_null() {
            None
        } else {
            Some(result as usize)
        }
    }

    pub fn nls_get_a(&self, nls: usize) -> Option<[u8; 32]> {
        let func: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char)> =
            unsafe { self.lib.get(b"nls_get_A").ok()? };
        let mut buffer = [0u8; 32];
        unsafe {
            func(nls as *mut std::ffi::c_void, buffer.as_mut_ptr() as *mut c_char);
        }
        Some(buffer)
    }

    pub fn nls_get_m1(&self, nls: usize, b: &[u8], salt: &[u8]) -> Option<[u8; 20]> {
        let func: Symbol<
            unsafe extern "C" fn(*mut std::ffi::c_void, *mut c_char, *const c_char, *const c_char),
        > = unsafe { self.lib.get(b"nls_get_M1").ok()? };
        let mut buffer = [0u8; 20];
        unsafe {
            func(
                nls as *mut std::ffi::c_void,
                buffer.as_mut_ptr() as *mut c_char,
                b.as_ptr() as *const c_char,
                salt.as_ptr() as *const c_char,
            );
        }
        Some(buffer)
    }
}
