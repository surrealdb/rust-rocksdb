use crate::ffi;

use super::cloud_bucket_options::CloudBucketOptions;

/// Cloud System options.
///
/// # Examples
///
/// ```
/// use rocksdb::{CloudFileSystemOptions, CloudFileSystem};
///
/// let mut opts = CloudFileSystemOptions::new("db-path");
/// opts.set_persistent_cache_path("db-path");
/// opts.set_persistent_cache_size(1);
///
/// let cloud_fs = CloudFileSystem::new(opts);
/// ```
pub struct CloudFileSystemOptions {
    pub(crate) inner: *mut ffi::rocksdb_cloud_fs_options_t,
    pub(crate) persistent_cache_path: Option<String>,
    pub(crate) persistent_cache_size_gb: Option<usize>,
}

unsafe impl Send for CloudFileSystemOptions {}
unsafe impl Sync for CloudFileSystemOptions {}

impl Drop for CloudFileSystemOptions {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_destroy(self.inner);
        }
    }
}

impl Clone for CloudFileSystemOptions {
    fn clone(&self) -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_fs_options_create_copy(self.inner) };
        assert!(
            !inner.is_null(),
            "Could not copy RocksDB Cloud FileSystem options"
        );

        Self {
            inner,
            persistent_cache_path: self.persistent_cache_path.clone(),
            persistent_cache_size_gb: self.persistent_cache_size_gb.clone(),
        }
    }
}

impl CloudFileSystemOptions {
    /// Set the source bucket for the cloud file system.
    pub fn set_src_bucket(&mut self, bucket: CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_src_bucket(self.inner, bucket.inner);
        }
    }

    /// Set the destination bucket for the cloud file system.
    pub fn set_dst_bucket(&mut self, bucket: CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_dest_bucket(self.inner, bucket.inner);
        }
    }

    pub fn set_persistent_cache_path(&mut self, path: &str) {
        self.persistent_cache_path = Some(path.to_owned());
    }

    // Set the size of the persistent cache in gigabytes.
    pub fn set_persistent_cache_size_gb(&mut self, size: usize) {
        self.persistent_cache_size_gb = Some(size);
    }
}

impl Default for CloudFileSystemOptions {
    fn default() -> Self {
        unsafe {
            let opts = ffi::rocksdb_cloud_fs_options_create();
            assert!(!opts.is_null(), "Could not create RocksDB options");

            Self {
                inner: opts,
                persistent_cache_path: None,
                persistent_cache_size_gb: None,
            }
        }
    }
}
