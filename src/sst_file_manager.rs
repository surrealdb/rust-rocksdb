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
    /// Creates a new SstFileManager with the given environment.
    ///
    /// # Arguments
    ///
    /// * `env` - The environment to use for file operations
    ///
    /// # Errors
    ///
    /// Returns an error if the SstFileManager cannot be created.
    pub fn new(env: &Env) -> Result<Self, Error> {
        let manager = unsafe { ffi::rocksdb_sst_file_manager_create(env.0.inner) };
        if manager.is_null() {
            Err(Error::new("Could not create SstFileManager".to_owned()))
        } else {
            Ok(Self(Arc::new(SstFileManagerWrapper { inner: manager })))
        }
    }

    /// Sets the maximum allowed space usage for SST files.
    ///
    /// If the total size of SST files exceeds this limit, writes will fail.
    /// Setting this to 0 disables the limit.
    ///
    /// # Arguments
    ///
    /// * `max_allowed_space` - Maximum allowed space in bytes (0 = unlimited)
    pub fn set_max_allowed_space_usage(&self, max_allowed_space: u64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_max_allowed_space_usage(
                self.0.inner,
                max_allowed_space,
            );
        }
    }

    /// Sets the size of the compaction buffer.
    ///
    /// The compaction buffer is used to reserve space for compaction output.
    /// If set, the SstFileManager will consider this space as used when
    /// checking against the maximum allowed space.
    ///
    /// # Arguments
    ///
    /// * `compaction_buffer_size` - Size of the compaction buffer in bytes
    pub fn set_compaction_buffer_size(&self, compaction_buffer_size: u64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_compaction_buffer_size(
                self.0.inner,
                compaction_buffer_size,
            );
        }
    }

    /// Returns true if the total size of SST files exceeded the maximum allowed space.
    pub fn is_max_allowed_space_reached(&self) -> bool {
        unsafe { ffi::rocksdb_sst_file_manager_is_max_allowed_space_reached(self.0.inner) }
    }

    /// Returns true if the total size of SST files plus compaction buffer size
    /// exceeded the maximum allowed space.
    pub fn is_max_allowed_space_reached_including_compactions(&self) -> bool {
        unsafe {
            ffi::rocksdb_sst_file_manager_is_max_allowed_space_reached_including_compactions(
                self.0.inner,
            )
        }
    }

    /// Returns the total size of all tracked SST files in bytes.
    pub fn get_total_size(&self) -> u64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_total_size(self.0.inner) }
    }

    /// Returns the current delete rate in bytes per second.
    ///
    /// Returns 0 if there is no rate limiting.
    pub fn get_delete_rate_bytes_per_second(&self) -> i64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_delete_rate_bytes_per_second(self.0.inner) }
    }

    /// Sets the delete rate limit in bytes per second.
    ///
    /// This controls how fast obsolete files are deleted. Setting this to 0
    /// disables rate limiting and files will be deleted immediately.
    ///
    /// # Arguments
    ///
    /// * `delete_rate` - Delete rate in bytes per second (0 = unlimited)
    pub fn set_delete_rate_bytes_per_second(&self, delete_rate: i64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_delete_rate_bytes_per_second(
                self.0.inner,
                delete_rate,
            );
        }
    }

    /// Returns the maximum trash to DB size ratio.
    ///
    /// If the trash size exceeds this ratio of the total DB size, files will be
    /// deleted immediately regardless of the delete rate limit.
    pub fn get_max_trash_db_ratio(&self) -> f64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_max_trash_db_ratio(self.0.inner) }
    }

    /// Sets the maximum trash to DB size ratio.
    ///
    /// When the size of pending-deletion files (trash) exceeds this ratio
    /// relative to the database size, files will be deleted immediately
    /// regardless of the delete rate limit.
    ///
    /// # Arguments
    ///
    /// * `ratio` - Maximum trash to DB size ratio (e.g., 0.25 = 25%)
    pub fn set_max_trash_db_ratio(&self, ratio: f64) {
        unsafe {
            ffi::rocksdb_sst_file_manager_set_max_trash_db_ratio(self.0.inner, ratio);
        }
    }

    /// Returns the total size of trash files (files pending deletion) in bytes.
    pub fn get_total_trash_size(&self) -> u64 {
        unsafe { ffi::rocksdb_sst_file_manager_get_total_trash_size(self.0.inner) }
    }
}

unsafe impl Send for SstFileManagerWrapper {}
unsafe impl Sync for SstFileManagerWrapper {}
