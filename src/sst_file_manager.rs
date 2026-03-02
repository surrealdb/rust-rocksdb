use std::sync::Arc;

use crate::{ffi, Env, Error};

/// SstFileManager is used to track SST files in the database and control their
/// deletion rate.
///
/// All SstFileManager public functions are thread-safe.
///
/// SstFileManager can be used to:
/// * Set a limit on the total size of SST files. Once the limit is exceeded,
///   writes will fail with a specific status.
/// * Control the deletion rate of obsolete files to avoid saturating the
///   disk bandwidth with deletion operations.
/// * Track the total size of SST files and the total size of trash files.
///
/// See also: `rocksdb/include/rocksdb/sst_file_manager.h`
#[derive(Clone)]
pub struct SstFileManager(pub(crate) Arc<SstFileManagerWrapper>);

pub(crate) struct SstFileManagerWrapper {
    pub(crate) inner: *mut ffi::rocksdb_sst_file_manager_t,
}

impl Drop for SstFileManagerWrapper {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_sst_file_manager_destroy(self.inner);
        }
    }
}

impl SstFileManager {
    pub fn new(env: &Env) -> Result<Self, Error> {
        let manager = unsafe { ffi::rocksdb_sst_file_manager_create(env.0.inner) };
        if manager.is_null() {
            Err(Error::new("Could not create SstFileManager".to_owned()))
        } else {
            Ok(Self(Arc::new(SstFileManagerWrapper { inner: manager })))
        }
    }

    pub fn set_max_allowed_space_usage(&self, max_allowed_space: u64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_max_allowed_space_usage(
                self.0.inner,
                max_allowed_space,
            );
        }
    }

    pub fn set_compaction_buffer_size(&self, compaction_buffer_size: u64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_compaction_buffer_size(
                self.0.inner,
                compaction_buffer_size,
            );
        }
    }

    pub fn is_max_allowed_space_reached(&self) -> bool {
        unsafe { ffi::rocksdb_sst_file_manager_is_max_allowed_space_reached(self.0.inner) }
    }

    pub fn is_max_allowed_space_reached_including_compactions(&self) -> bool {
        unsafe {
            ffi::rocksdb_sst_file_manager_is_max_allowed_space_reached_including_compactions(
                self.0.inner,
            )
        }
    }

    pub fn get_total_size(&self) -> u64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_total_size(self.0.inner) }
    }

    pub fn get_delete_rate_bytes_per_second(&self) -> i64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_delete_rate_bytes_per_second(self.0.inner) }
    }

    pub fn set_delete_rate_bytes_per_second(&self, delete_rate: i64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_delete_rate_bytes_per_second(
                self.0.inner,
                delete_rate,
            );
        }
    }

    pub fn get_max_trash_db_ratio(&self) -> f64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_max_trash_db_ratio(self.0.inner) }
    }

    pub fn set_max_trash_db_ratio(&self, ratio: f64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_max_trash_db_ratio(self.0.inner, ratio);
        }
    }

    pub fn get_total_trash_size(&self) -> u64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_total_trash_size(self.0.inner) }
    }
}

unsafe impl Send for SstFileManagerWrapper {}
unsafe impl Sync for SstFileManagerWrapper {}
