use std::sync::Arc;

use crate::{env::EnvWrapper, ffi, CloudFileSystemOptions, Env, Error};

/// Cloud FileSystem.
#[derive(Clone)]
pub struct CloudFileSystem(pub(crate) Arc<CloudFileSystemWrapper>);

pub(crate) struct CloudFileSystemWrapper {
    pub(crate) inner: *mut ffi::rocksdb_cloud_fs_t,
    pub(crate) opts: CloudFileSystemOptions,
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
    pub fn new(opts: &CloudFileSystemOptions) -> Result<Self, Error> {
        let cloud_fs = Self::create_cloud_fs(&opts);
        if let Ok(cloud_fs) = cloud_fs {
            Ok(Self(Arc::new(CloudFileSystemWrapper {
                inner: cloud_fs,
                opts: opts.clone(),
            })))
        } else {
            Err(Error::new("Could not create cloud file system".to_owned()))
        }
    }

    fn create_cloud_fs(
        opts: &CloudFileSystemOptions,
    ) -> Result<*mut ffi::rocksdb_cloud_fs_t, Error> {
        unsafe {
            let cloud_fs = ffi_try!(ffi::rocksdb_cloud_fs_create(opts.inner));
            Ok(cloud_fs)
        }
    }

    pub fn create_cloud_env(&self) -> Result<Env, Error> {
        let env = unsafe { ffi::rocksdb_cloud_env_create(self.0.inner) };

        if env.is_null() {
            Err(Error::new("Could not create cloud env".to_owned()))
        } else {
            Ok(Env(Arc::new(EnvWrapper { inner: env })))
        }
    }

    pub fn opts(&self) -> &CloudFileSystemOptions {
        &self.0.opts
    }
}
