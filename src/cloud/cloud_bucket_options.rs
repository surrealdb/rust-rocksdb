use std::ffi::CStr;

use crate::{ffi, ffi_util::CStrLike};

/// Cloud bucket options specifying the cloud storage bucket configuration.
pub struct CloudBucketOptions {
    pub(crate) inner: *mut ffi::rocksdb_cloud_bucket_options_t,
}

unsafe impl Send for CloudBucketOptions {}
unsafe impl Sync for CloudBucketOptions {}

impl Drop for CloudBucketOptions {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_bucket_options_destroy(self.inner);
        }
    }
}

impl Default for CloudBucketOptions {
    fn default() -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_bucket_options_create() };
        assert!(!inner.is_null(), "Could not create CloudBucketOptions");
        Self { inner }
    }
}

impl CloudBucketOptions {
    /// Configure bucket options from environment variables with the given prefix.
    ///
    /// Reads `{prefix}_BUCKET_NAME`, `{prefix}_REGION`, and `{prefix}_OBJECT_PATH`.
    pub fn read_from_env(mut self, env_prefix: &str) -> Self {
        if let Ok(val) = std::env::var(format!("{env_prefix}_BUCKET_NAME")) {
            self.set_bucket_name(&val);
        }
        if let Ok(val) = std::env::var(format!("{env_prefix}_REGION")) {
            self.set_region(&val);
        }
        if let Ok(val) = std::env::var(format!("{env_prefix}_OBJECT_PATH")) {
            self.set_object_path(&val);
        }
        self
    }

    pub fn get_bucket_name(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_bucket_name(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    pub fn set_bucket_name(&mut self, name: impl CStrLike) {
        let name = name.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_bucket_name(self.inner, name.as_ptr());
        }
    }

    pub fn get_prefix(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_prefix(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    pub fn set_prefix(&mut self, prefix: impl CStrLike) {
        let prefix = prefix.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_prefix(self.inner, prefix.as_ptr());
        }
    }

    pub fn get_region(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_bucket_options_get_region(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
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
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    pub fn set_object_path(&mut self, path: impl CStrLike) {
        let path = path.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_bucket_options_set_object_path(self.inner, path.as_ptr());
        }
    }

    pub fn is_valid(&self) -> bool {
        unsafe { ffi::rocksdb_cloud_bucket_options_is_valid(self.inner) != 0 }
    }
}
