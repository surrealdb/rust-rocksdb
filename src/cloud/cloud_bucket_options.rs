use std::ffi::CStr;

use crate::{ffi, ffi_util::CStrLike};

/// Cloud Bucket options.
pub struct CloudBucketOptions {
    pub(crate) inner: *mut ffi::rocksdb_cloud_bucket_options_t,
}

const DEFAULT_ENV_PREFIX: &str = "ROCKSDB_CLOUD";

unsafe impl Send for CloudBucketOptions {}
unsafe impl Sync for CloudBucketOptions {}

impl Drop for CloudBucketOptions {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_bucket_options_destroy(self.inner);
        }
    }
}

impl Clone for CloudBucketOptions {
    fn clone(&self) -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_bucket_options_create_copy(self.inner) };
        assert!(
            !inner.is_null(),
            "Could not copy RocksDB Cloud Bucket options"
        );

        Self { inner }
    }
}

impl CloudBucketOptions {
    pub fn read_from_env(&self, env_prefix: &str) -> Self {
        let mut result = self.clone();
        std::env::vars().for_each(|(key, value)| match key {
            _ if key == format!("{env_prefix}_BUCKET_NAME") => result.set_bucket_name(&value),
            _ if key == format!("{env_prefix}_REGION") => result.set_region(&value),
            _ if key == format!("{env_prefix}_OBJECT_PATH") => result.set_object_path(&value),
            _ => {}
        });

        result
    }
    pub fn get_bucket_name(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_bucket_name(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).to_string()
        }
    }
    pub fn set_bucket_name(&mut self, name: impl CStrLike) {
        let name = name.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_bucket_name(self.inner, name.as_ptr());
        }
    }

    pub fn get_region(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_region(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).to_string()
        }
    }

    pub fn set_region(&mut self, region: impl CStrLike) {
        let region = region.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_region(self.inner, region.as_ptr());
        }
    }

    pub fn get_object_path(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_object_path(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).to_string()
        }
    }

    pub fn set_object_path(&mut self, path: impl CStrLike) {
        let path = path.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_object_path(self.inner, path.as_ptr());
        }
    }

    pub fn is_valid(&self) -> bool {
        unsafe { ffi::rocksdb_cloud_bucket_options_is_valid(self.inner) }
    }
}

impl Default for CloudBucketOptions {
    fn default() -> Self {
        let opts = unsafe { ffi::rocksdb_cloud_bucket_options_create() };

        if opts.is_null() {
            panic!("Could not create RocksDB Cloud Bucket options");
        };

        Self { inner: opts }.read_from_env(DEFAULT_ENV_PREFIX)
    }
}
