use crate::ffi;

/// Options for checkpointing a cloud database to another cloud location.
pub struct CloudCheckpointOptions {
    pub(crate) inner: *mut ffi::rocksdb_cloud_checkpoint_options_t,
}

unsafe impl Send for CloudCheckpointOptions {}
unsafe impl Sync for CloudCheckpointOptions {}

impl Drop for CloudCheckpointOptions {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_checkpoint_options_destroy(self.inner);
        }
    }
}

impl Default for CloudCheckpointOptions {
    fn default() -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_checkpoint_options_create() };
        assert!(!inner.is_null(), "Could not create CloudCheckpointOptions");
        Self { inner }
    }
}

impl CloudCheckpointOptions {
    pub fn set_thread_count(&mut self, count: i32) {
        unsafe {
            ffi::rocksdb_cloud_checkpoint_options_set_thread_count(
                self.inner,
                count as libc::c_int,
            );
        }
    }

    pub fn get_thread_count(&self) -> i32 {
        unsafe {
            ffi::rocksdb_cloud_checkpoint_options_get_thread_count(self.inner) as i32
        }
    }

    pub fn set_flush_memtable(&mut self, flush: bool) {
        unsafe {
            ffi::rocksdb_cloud_checkpoint_options_set_flush_memtable(
                self.inner,
                flush as libc::c_uchar,
            );
        }
    }

    pub fn get_flush_memtable(&self) -> bool {
        unsafe {
            ffi::rocksdb_cloud_checkpoint_options_get_flush_memtable(self.inner) != 0
        }
    }
}
