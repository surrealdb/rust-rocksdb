use std::sync::Arc;

use crate::{
    cloud::CloudFileSystemOptions,
    env::EnvWrapper,
    ffi, CStrLike, Env, Error,
};

/// Cloud file system that provides cloud-backed storage for RocksDB.
#[derive(Clone)]
pub struct CloudFileSystem(pub(crate) Arc<CloudFileSystemWrapper>);

pub(crate) struct CloudFileSystemWrapper {
    pub(crate) inner: *mut ffi::rocksdb_cloud_fs_t,
    pub(crate) persistent_cache_path: String,
    pub(crate) persistent_cache_size_gb: u64,
}

unsafe impl Send for CloudFileSystemWrapper {}
unsafe impl Sync for CloudFileSystemWrapper {}

impl Drop for CloudFileSystemWrapper {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_fs_destroy(self.inner);
        }
    }
}

impl CloudFileSystem {
    /// Create a new cloud file system from the given options.
    ///
    /// Requires a cloud storage provider to be linked (e.g., the `aws` feature).
    pub fn new(opts: &CloudFileSystemOptions) -> Result<Self, Error> {
        let cloud_fs = unsafe { ffi_try!(ffi::rocksdb_cloud_fs_create(opts.inner)) };

        Ok(Self(Arc::new(CloudFileSystemWrapper {
            inner: cloud_fs,
            persistent_cache_path: opts
                .persistent_cache_path
                .clone()
                .unwrap_or_default(),
            persistent_cache_size_gb: opts.persistent_cache_size_gb.unwrap_or(0),
        })))
    }

    /// Create a cloud-backed Env from this file system.
    pub fn create_cloud_env(&self) -> Result<Env, Error> {
        let env = unsafe { ffi::rocksdb_cloud_env_create(self.0.inner) };
        if env.is_null() {
            Err(Error::new("Could not create cloud env".to_owned()))
        } else {
            Ok(Env(Arc::new(EnvWrapper { inner: env })))
        }
    }

    pub(crate) fn persistent_cache_path_c(&self) -> std::ffi::CString {
        self.0
            .persistent_cache_path
            .as_str()
            .into_c_string()
            .unwrap()
    }

    pub(crate) fn persistent_cache_size_gb(&self) -> u64 {
        self.0.persistent_cache_size_gb
    }
}
