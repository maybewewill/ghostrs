#![allow(unused)]
use libloading::{Library, Symbol};
use std::env;
use std::ffi::c_void;
use std::io;
use std::os::raw::{c_char, c_uint};
use std::path::Path;

#[derive(Debug)]
pub struct CdKey {
    pub public_value: u32,
    pub product: u32,
    pub hash: Vec<u8>,
}

pub struct BncsUtil {
    lib: Library,
}

impl BncsUtil {
    pub fn new() -> io::Result<Self> {
        let current_dir = env::current_dir()?;
        //println!("{:?}", current_dir);
        let dll_path = current_dir.join("libbncsutil.so");
        if !Path::new(&dll_path).exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "libbncsutil.so not found in current directory",
            ));
        }
        let lib = unsafe { Library::new(&dll_path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))? };
        Ok(BncsUtil { lib })
    }

    pub fn extract_mpq_number(&self, mpq_name: &str) -> io::Result<i32> {
        let func: Symbol<unsafe extern "C" fn(*const c_char) -> i32> = unsafe {
            self.lib
                .get(b"extractMPQNumber")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_mpq_name = std::ffi::CString::new(mpq_name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let result = unsafe { func(c_mpq_name.as_ptr()) };
        Ok(result)
    }

    pub fn get_exe_info(&self, file_name: &str, platform: u32) -> io::Result<(String, u32)> {
        let func: Symbol<
            unsafe extern "C" fn(
                *const c_char,
                *mut c_char,
                c_uint,
                *mut c_uint,
                c_uint,
            ) -> c_uint,
        > = unsafe {
            self.lib
                .get(b"getExeInfo")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_file_name = std::ffi::CString::new(file_name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut exe_info = vec![0u8; 1024];
        let mut version = 0u32;
        unsafe {
            func(
                c_file_name.as_ptr(),
                exe_info.as_mut_ptr() as *mut c_char,
                256,
                &mut version as *mut c_uint,
                platform,
            );
        }
        let exe_info_str = String::from_utf8_lossy(&exe_info)
            .trim_end_matches('\0')
            .replace("/110", "/10");
        Ok((exe_info_str, version))
    }

    pub fn check_revision_flat(
        &self,
        value_string: &str,
        file1: &str,
        file2: &str,
        file3: &str,
        mpq_number: i32,
    ) -> io::Result<u32> {
        let func: Symbol<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                i32,
                *mut c_uint,
            ) -> c_uint,
        > = unsafe {
            self.lib
                .get(b"checkRevisionFlat")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_value_string = std::ffi::CString::new(value_string)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let c_file1 = std::ffi::CString::new(file1)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let c_file2 = std::ffi::CString::new(file2)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let c_file3 = std::ffi::CString::new(file3)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut checksum = 0u32;
        unsafe {
            func(
                c_value_string.as_ptr(),
                c_file1.as_ptr(),
                c_file2.as_ptr(),
                c_file3.as_ptr(),
                mpq_number,
                &mut checksum as *mut c_uint,
            );
        }
        Ok(checksum)
    }

    pub fn kd_quick(
        &self,
        cd_key: &str,
        client_token: u32,
        server_token: u32,
    ) -> io::Result<CdKey> {
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
        > = unsafe {
            self.lib
                .get(b"kd_quick")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_cd_key = std::ffi::CString::new(cd_key)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut public_value = 0u32;
        let mut product = 0u32;
        let mut hash_buffer = vec![0u8; 256];
        unsafe {
            func(
                c_cd_key.as_ptr(),
                client_token,
                server_token,
                &mut public_value as *mut c_uint,
                &mut product as *mut c_uint,
                hash_buffer.as_mut_ptr() as *mut c_char,
                256,
            );
        }
        let hash = hash_buffer
            .into_iter()
            .take_while(|&b| b != 0)
            .collect::<Vec<u8>>();
        Ok(CdKey {
            public_value,
            product,
            hash,
        })
    }

    pub fn nls_init(&self, username: &str, password: &str) -> io::Result<usize> {
        let func: Symbol<
            unsafe extern "C" fn(*const c_char, c_uint, *const c_char, c_uint) -> *mut c_void,
        > = unsafe {
            self.lib
                .get(b"nls_init_l")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_username = std::ffi::CString::new(username)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let c_password = std::ffi::CString::new(password)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let result = unsafe {
            func(
                c_username.as_ptr(),
                username.len() as c_uint,
                c_password.as_ptr(),
                password.len() as c_uint,
            )
        };
        Ok(result as usize)
    }

    pub fn nls_get_a(&self, nls: usize) -> io::Result<Vec<u8>> {
        let func: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_char)> = unsafe {
            self.lib
                .get(b"nls_get_A")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let mut buffer = vec![0u8; 32];
        unsafe {
            func(nls as *mut c_void, buffer.as_mut_ptr() as *mut c_char);
        }
        Ok(buffer)
    }

    pub fn nls_get_m1(&self, nls: usize, b: &[u8], salt: &[u8]) -> io::Result<Vec<u8>> {
        let func: Symbol<
            unsafe extern "C" fn(*mut c_void, *mut c_char, *const c_char, *const c_char),
        > = unsafe {
            self.lib
                .get(b"nls_get_M1")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let mut buffer = vec![0u8; 20];
        unsafe {
            func(
                nls as *mut c_void,
                buffer.as_mut_ptr() as *mut c_char,
                b.as_ptr() as *const c_char,
                salt.as_ptr() as *const c_char,
            );
        }
        Ok(buffer)
    }

    pub fn hash_password(&self, password: &str) -> io::Result<Vec<u8>> {
        let func: Symbol<unsafe extern "C" fn(*const c_char, *mut c_char)> = unsafe {
            self.lib
                .get(b"hashPassword")
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };
        let c_password = std::ffi::CString::new(password)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut buffer = vec![0u8; 20];
        unsafe {
            func(c_password.as_ptr(), buffer.as_mut_ptr() as *mut c_char);
        }
        Ok(buffer)
    }
}