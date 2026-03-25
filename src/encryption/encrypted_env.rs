// Copyright (c) 2025-present, SurrealDB Ltd.  All rights reserved.

use std::ffi::{CStr, CString};
use std::sync::Arc;

use libc::{c_char, c_int, c_void, size_t};

use crate::env::EnvWrapper;
use crate::{ffi, Env, Error};

use super::key_manager::KeyManager;

struct KeyManagerState {
    manager: Box<dyn KeyManager>,
}

/// Create an encrypted [`Env`] that delegates key management to the provided
/// [`KeyManager`] implementation.
///
/// The returned `Env` can be passed to [`Options::set_env()`](crate::Options::set_env)
/// to transparently encrypt all database files using OpenSSL AES-CTR.
///
/// # Example
///
/// ```no_run
/// use rocksdb::{DB, Options};
/// use rocksdb::encryption::{
///     create_encrypted_env, EncryptionMethod, FileEncryptionInfo, KeyManager,
/// };
///
/// struct MyKeyManager;
///
/// impl KeyManager for MyKeyManager {
///     fn get_file(&self, _fname: &str) -> Result<FileEncryptionInfo, rocksdb::Error> {
///         Ok(FileEncryptionInfo {
///             method: EncryptionMethod::Aes256Ctr,
///             key: vec![0u8; 32],
///             iv: vec![0u8; 16],
///         })
///     }
///     fn new_file(&self, _fname: &str) -> Result<FileEncryptionInfo, rocksdb::Error> {
///         Ok(FileEncryptionInfo {
///             method: EncryptionMethod::Aes256Ctr,
///             key: vec![0u8; 32],
///             iv: vec![0u8; 16],
///         })
///     }
///     fn delete_file(&self, _fname: &str) -> Result<(), rocksdb::Error> { Ok(()) }
///     fn link_file(&self, _src: &str, _dst: &str) -> Result<(), rocksdb::Error> { Ok(()) }
/// }
///
/// let env = create_encrypted_env(MyKeyManager).unwrap();
/// let mut opts = Options::default();
/// opts.create_if_missing(true);
/// opts.set_env(&env);
/// let db = DB::open(&opts, "/tmp/encrypted_db").unwrap();
/// ```
pub fn create_encrypted_env(key_manager: impl KeyManager + 'static) -> Result<Env, Error> {
    let state = Box::new(KeyManagerState {
        manager: Box::new(key_manager),
    });
    let state_ptr = Box::into_raw(state) as *mut c_void;

    unsafe {
        let c_key_manager = ffi::rocksdb_encryption_key_manager_create(
            state_ptr,
            Some(key_manager_destructor),
            Some(key_manager_get_file),
            Some(key_manager_new_file),
            Some(key_manager_delete_file),
            Some(key_manager_link_file),
        );

        if c_key_manager.is_null() {
            drop(Box::from_raw(state_ptr as *mut KeyManagerState));
            return Err(Error::new(
                "Failed to create encryption key manager".to_owned(),
            ));
        }

        let base_env = ffi::rocksdb_create_default_env();
        let encrypted_env = ffi::rocksdb_create_key_managed_encrypted_env(base_env, c_key_manager);

        if encrypted_env.is_null() {
            ffi::rocksdb_encryption_key_manager_destroy(c_key_manager);
            return Err(Error::new("Failed to create encrypted env".to_owned()));
        }

        Ok(Env(Arc::new(EnvWrapper {
            inner: encrypted_env,
        })))
    }
}

unsafe extern "C" fn key_manager_destructor(state: *mut c_void) {
    drop(Box::from_raw(state as *mut KeyManagerState));
}

unsafe extern "C" fn key_manager_get_file(
    state: *mut c_void,
    fname: *const c_char,
    method: *mut c_int,
    key: *mut *mut c_char,
    key_len: *mut size_t,
    iv: *mut *mut c_char,
    iv_len: *mut size_t,
    errptr: *mut *mut c_char,
) {
    let state = &*(state as *const KeyManagerState);
    let fname = CStr::from_ptr(fname).to_string_lossy();

    match state.manager.get_file(&fname) {
        Ok(info) => write_file_info(info, method, key, key_len, iv, iv_len),
        Err(e) => set_error(errptr, e),
    }
}

unsafe extern "C" fn key_manager_new_file(
    state: *mut c_void,
    fname: *const c_char,
    method: *mut c_int,
    key: *mut *mut c_char,
    key_len: *mut size_t,
    iv: *mut *mut c_char,
    iv_len: *mut size_t,
    errptr: *mut *mut c_char,
) {
    let state = &*(state as *const KeyManagerState);
    let fname = CStr::from_ptr(fname).to_string_lossy();

    match state.manager.new_file(&fname) {
        Ok(info) => write_file_info(info, method, key, key_len, iv, iv_len),
        Err(e) => set_error(errptr, e),
    }
}

unsafe extern "C" fn key_manager_delete_file(
    state: *mut c_void,
    fname: *const c_char,
    errptr: *mut *mut c_char,
) {
    let state = &*(state as *const KeyManagerState);
    let fname = CStr::from_ptr(fname).to_string_lossy();

    if let Err(e) = state.manager.delete_file(&fname) {
        set_error(errptr, e);
    }
}

unsafe extern "C" fn key_manager_link_file(
    state: *mut c_void,
    src: *const c_char,
    dst: *const c_char,
    errptr: *mut *mut c_char,
) {
    let state = &*(state as *const KeyManagerState);
    let src = CStr::from_ptr(src).to_string_lossy();
    let dst = CStr::from_ptr(dst).to_string_lossy();

    if let Err(e) = state.manager.link_file(&src, &dst) {
        set_error(errptr, e);
    }
}

unsafe fn write_file_info(
    info: super::FileEncryptionInfo,
    method: *mut c_int,
    key: *mut *mut c_char,
    key_len: *mut size_t,
    iv: *mut *mut c_char,
    iv_len: *mut size_t,
) {
    *method = info.method as c_int;
    *key = libc::malloc(info.key.len()) as *mut c_char;
    if (*key).is_null() {
        *key_len = 0;
    } else {
        *key_len = info.key.len();
        std::ptr::copy_nonoverlapping(info.key.as_ptr(), *key as *mut u8, info.key.len());
    }
    *iv = libc::malloc(info.iv.len()) as *mut c_char;
    if (*iv).is_null() {
        *iv_len = 0;
    } else {
        *iv_len = info.iv.len();
        std::ptr::copy_nonoverlapping(info.iv.as_ptr(), *iv as *mut u8, info.iv.len());
    }
}

unsafe fn set_error(errptr: *mut *mut c_char, e: Error) {
    let msg = e.into_string();
    let cs = CString::new(msg).unwrap_or_else(|_| CString::new("unknown error").unwrap());
    *errptr = cs.into_raw();
}
