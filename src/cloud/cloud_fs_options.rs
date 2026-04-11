use std::ffi::CStr;

use crate::{
    cloud::{CloudBucketOptions, CloudCredentials},
    ffi,
    ffi_util::CStrLike,
};

/// Controls when WAL records are published to Kafka.
///
/// Requires `USE_KAFKA` at build time for `PerAppend` and `PerSync` modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalKafkaSyncMode {
    /// No Kafka WAL sync (default).
    None = 0,
    /// Publish to Kafka on every `Append()`.
    PerAppend = 1,
    /// Publish to Kafka on every `Sync()`/`fsync`.
    PerSync = 2,
}

impl WalKafkaSyncMode {
    fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::PerAppend,
            2 => Self::PerSync,
            _ => Self::None,
        }
    }
}

/// WAL sources for read replica catch-up. Combine with bitwise OR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadReplicaWALSource(u32);

impl ReadReplicaWALSource {
    /// Scan local WAL directory (default).
    pub const LOCAL: Self = Self(0x1);
    /// Download WAL from cloud object storage (S3/GCS).
    pub const CLOUD: Self = Self(0x2);
    /// Consume WAL from Kafka.
    pub const KAFKA: Self = Self(0x4);

    /// Create from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Return the raw bits.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for ReadReplicaWALSource {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ReadReplicaWALSource {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for ReadReplicaWALSource {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// Cloud file system options controlling how RocksDB interacts with cloud storage.
///
/// Persistent cache path and size are stored as Rust fields and passed to the
/// DB open calls rather than being set on the FFI options object.
pub struct CloudFileSystemOptions {
    pub(crate) inner: *mut ffi::rocksdb_cloud_fs_options_t,
    pub(crate) persistent_cache_path: Option<String>,
    pub(crate) persistent_cache_size_gb: Option<u64>,
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

impl Default for CloudFileSystemOptions {
    fn default() -> Self {
        let inner = unsafe { ffi::rocksdb_cloud_fs_options_create() };
        assert!(!inner.is_null(), "Could not create CloudFileSystemOptions");
        Self {
            inner,
            persistent_cache_path: None,
            persistent_cache_size_gb: None,
        }
    }
}

macro_rules! cloud_fs_bool_option {
    ($set:ident, $get:ident, $ffi_set:ident, $ffi_get:ident) => {
        pub fn $set(&mut self, val: bool) {
            unsafe {
                ffi::$ffi_set(self.inner, val as libc::c_uchar);
            }
        }

        pub fn $get(&self) -> bool {
            unsafe { ffi::$ffi_get(self.inner) != 0 }
        }
    };
}

impl CloudFileSystemOptions {
    /// Set the source bucket for the cloud file system.
    pub fn set_src_bucket(&mut self, bucket: &CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_src_bucket(self.inner, bucket.inner);
        }
    }

    /// Set the destination bucket for the cloud file system.
    pub fn set_dest_bucket(&mut self, bucket: &CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_dest_bucket(self.inner, bucket.inner);
        }
    }

    /// Set the credentials for the cloud file system.
    pub fn set_credentials(&mut self, creds: &CloudCredentials) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_credentials(self.inner, creds.inner);
        }
    }

    pub fn set_persistent_cache_path(&mut self, path: impl Into<String>) {
        self.persistent_cache_path = Some(path.into());
    }

    pub fn set_persistent_cache_size_gb(&mut self, size: u64) {
        self.persistent_cache_size_gb = Some(size);
    }

    // Boolean options
    cloud_fs_bool_option!(
        set_keep_local_sst_files,
        get_keep_local_sst_files,
        rocksdb_cloud_fs_options_set_keep_local_sst_files,
        rocksdb_cloud_fs_options_get_keep_local_sst_files
    );

    cloud_fs_bool_option!(
        set_validate_filesize,
        get_validate_filesize,
        rocksdb_cloud_fs_options_set_validate_filesize,
        rocksdb_cloud_fs_options_get_validate_filesize
    );

    cloud_fs_bool_option!(
        set_server_side_encryption,
        get_server_side_encryption,
        rocksdb_cloud_fs_options_set_server_side_encryption,
        rocksdb_cloud_fs_options_get_server_side_encryption
    );

    cloud_fs_bool_option!(
        set_create_bucket_if_missing,
        get_create_bucket_if_missing,
        rocksdb_cloud_fs_options_set_create_bucket_if_missing,
        rocksdb_cloud_fs_options_get_create_bucket_if_missing
    );

    cloud_fs_bool_option!(
        set_run_purger,
        get_run_purger,
        rocksdb_cloud_fs_options_set_run_purger,
        rocksdb_cloud_fs_options_get_run_purger
    );

    cloud_fs_bool_option!(
        set_resync_on_open,
        get_resync_on_open,
        rocksdb_cloud_fs_options_set_resync_on_open,
        rocksdb_cloud_fs_options_get_resync_on_open
    );

    cloud_fs_bool_option!(
        set_skip_dbid_verification,
        get_skip_dbid_verification,
        rocksdb_cloud_fs_options_set_skip_dbid_verification,
        rocksdb_cloud_fs_options_get_skip_dbid_verification
    );

    cloud_fs_bool_option!(
        set_use_aws_transfer_manager,
        get_use_aws_transfer_manager,
        rocksdb_cloud_fs_options_set_use_aws_transfer_manager,
        rocksdb_cloud_fs_options_get_use_aws_transfer_manager
    );

    cloud_fs_bool_option!(
        set_skip_cloud_files_in_getchildren,
        get_skip_cloud_files_in_getchildren,
        rocksdb_cloud_fs_options_set_skip_cloud_files_in_getchildren,
        rocksdb_cloud_fs_options_get_skip_cloud_files_in_getchildren
    );

    cloud_fs_bool_option!(
        set_skip_cloud_listing_on_open,
        get_skip_cloud_listing_on_open,
        rocksdb_cloud_fs_options_set_skip_cloud_listing_on_open,
        rocksdb_cloud_fs_options_get_skip_cloud_listing_on_open
    );

    /// Set the number of TLS connections to pre-establish with the cloud
    /// storage endpoint during initialization. Set to 0 to disable.
    pub fn set_warm_connection_pool_size(&mut self, val: i32) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_warm_connection_pool_size(
                self.inner,
                val as libc::c_int,
            );
        }
    }

    /// Returns the configured warm connection pool size.
    pub fn get_warm_connection_pool_size(&self) -> i32 {
        unsafe { ffi::rocksdb_cloud_fs_options_get_warm_connection_pool_size(self.inner) as i32 }
    }

    cloud_fs_bool_option!(
        set_use_direct_io_for_cloud_download,
        get_use_direct_io_for_cloud_download,
        rocksdb_cloud_fs_options_set_use_direct_io_for_cloud_download,
        rocksdb_cloud_fs_options_get_use_direct_io_for_cloud_download
    );

    cloud_fs_bool_option!(
        set_roll_cloud_manifest_on_open,
        get_roll_cloud_manifest_on_open,
        rocksdb_cloud_fs_options_set_roll_cloud_manifest_on_open,
        rocksdb_cloud_fs_options_get_roll_cloud_manifest_on_open
    );

    cloud_fs_bool_option!(
        set_delete_cloud_invisible_files_on_open,
        get_delete_cloud_invisible_files_on_open,
        rocksdb_cloud_fs_options_set_delete_cloud_invisible_files_on_open,
        rocksdb_cloud_fs_options_get_delete_cloud_invisible_files_on_open
    );

    // Numeric options

    pub fn set_purger_periodicity_millis(&mut self, val: u64) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_purger_periodicity_millis(self.inner, val);
        }
    }

    pub fn get_purger_periodicity_millis(&self) -> u64 {
        unsafe { ffi::rocksdb_cloud_fs_options_get_purger_periodicity_millis(self.inner) }
    }

    pub fn set_request_timeout_ms(&mut self, val: u64) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_request_timeout_ms(self.inner, val);
        }
    }

    pub fn get_request_timeout_ms(&self) -> u64 {
        unsafe { ffi::rocksdb_cloud_fs_options_get_request_timeout_ms(self.inner) }
    }

    pub fn set_number_objects_listed_in_one_iteration(&mut self, val: i32) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_number_objects_listed_in_one_iteration(
                self.inner,
                val as libc::c_int,
            );
        }
    }

    pub fn get_number_objects_listed_in_one_iteration(&self) -> i32 {
        unsafe {
            ffi::rocksdb_cloud_fs_options_get_number_objects_listed_in_one_iteration(self.inner)
                as i32
        }
    }

    pub fn set_constant_sst_file_size_in_sst_file_manager(&mut self, val: i64) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_constant_sst_file_size_in_sst_file_manager(
                self.inner, val,
            );
        }
    }

    pub fn get_constant_sst_file_size_in_sst_file_manager(&self) -> i64 {
        unsafe {
            ffi::rocksdb_cloud_fs_options_get_constant_sst_file_size_in_sst_file_manager(self.inner)
        }
    }

    pub fn set_cloud_file_deletion_delay_secs(&mut self, val: u64) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_cloud_file_deletion_delay_secs(self.inner, val);
        }
    }

    pub fn get_cloud_file_deletion_delay_secs(&self) -> u64 {
        unsafe { ffi::rocksdb_cloud_fs_options_get_cloud_file_deletion_delay_secs(self.inner) }
    }

    // String options

    pub fn set_encryption_key_id(&mut self, val: impl CStrLike) {
        let val = val.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_encryption_key_id(self.inner, val.as_ptr());
        }
    }

    pub fn get_encryption_key_id(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_fs_options_get_encryption_key_id(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    pub fn set_cookie_on_open(&mut self, val: impl CStrLike) {
        let val = val.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_cookie_on_open(self.inner, val.as_ptr());
        }
    }

    pub fn get_cookie_on_open(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_fs_options_get_cookie_on_open(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    pub fn set_new_cookie_on_open(&mut self, val: impl CStrLike) {
        let val = val.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_new_cookie_on_open(self.inner, val.as_ptr());
        }
    }

    pub fn get_new_cookie_on_open(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_fs_options_get_new_cookie_on_open(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    // WAL options

    // keep_local_log_files: if true, WAL (log) files are written to the local
    // filesystem. When false and a Kafka or cloud WAL sync mode is enabled,
    // WAL files are not stored locally.
    cloud_fs_bool_option!(
        set_keep_local_log_files,
        get_keep_local_log_files,
        rocksdb_cloud_fs_options_set_keep_local_log_files,
        rocksdb_cloud_fs_options_get_keep_local_log_files
    );

    // background_wal_sync_to_cloud: if true, WAL files are periodically
    // uploaded to cloud object storage (S3/GCS) in the background via
    // CloudScheduler.
    cloud_fs_bool_option!(
        set_background_wal_sync_to_cloud,
        get_background_wal_sync_to_cloud,
        rocksdb_cloud_fs_options_set_background_wal_sync_to_cloud,
        rocksdb_cloud_fs_options_get_background_wal_sync_to_cloud
    );

    /// Set the Kafka WAL sync mode controlling when WAL records are published
    /// to Kafka. Requires `USE_KAFKA` at build time for non-`None` modes.
    pub fn set_kafka_wal_sync_mode(&mut self, mode: WalKafkaSyncMode) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_kafka_wal_sync_mode(
                self.inner,
                mode as libc::c_uchar,
            );
        }
    }

    /// Returns the configured Kafka WAL sync mode.
    pub fn get_kafka_wal_sync_mode(&self) -> WalKafkaSyncMode {
        let val = unsafe { ffi::rocksdb_cloud_fs_options_get_kafka_wal_sync_mode(self.inner) };
        WalKafkaSyncMode::from_u8(val)
    }

    /// Set the Kafka bootstrap servers (e.g. `"broker1:9092,broker2:9092"`).
    /// Required when `kafka_wal_sync_mode` is not `None`.
    pub fn set_kafka_bootstrap_servers(&mut self, val: impl CStrLike) {
        let val = val.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_kafka_bootstrap_servers(self.inner, val.as_ptr());
        }
    }

    /// Returns the configured Kafka bootstrap servers.
    pub fn get_kafka_bootstrap_servers(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_fs_options_get_kafka_bootstrap_servers(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    /// Set the prefix for the Kafka topic name. The full topic is
    /// `"<prefix>.<dest_bucket_name>"`.
    pub fn set_kafka_topic_prefix(&mut self, val: impl CStrLike) {
        let val = val.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_kafka_topic_prefix(self.inner, val.as_ptr());
        }
    }

    /// Returns the configured Kafka topic prefix.
    pub fn get_kafka_topic_prefix(&self) -> String {
        unsafe {
            let ptr = ffi::rocksdb_cloud_fs_options_get_kafka_topic_prefix(self.inner);
            String::from_utf8_lossy(CStr::from_ptr(ptr).to_bytes()).into_owned()
        }
    }

    /// Set the interval in milliseconds between background WAL uploads when
    /// `background_wal_sync_to_cloud` is enabled.
    pub fn set_background_wal_sync_interval_ms(&mut self, val: u64) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_background_wal_sync_interval_ms(self.inner, val);
        }
    }

    /// Returns the configured background WAL sync interval in milliseconds.
    pub fn get_background_wal_sync_interval_ms(&self) -> u64 {
        unsafe { ffi::rocksdb_cloud_fs_options_get_background_wal_sync_interval_ms(self.inner) }
    }

    // When true and `background_wal_sync_to_cloud` is enabled, only new
    // bytes since the last upload are written as separate delta objects
    // rather than re-uploading the entire WAL file. Recovery reassembles
    // deltas in order.
    cloud_fs_bool_option!(
        set_use_wal_delta_upload,
        get_use_wal_delta_upload,
        rocksdb_cloud_fs_options_set_use_wal_delta_upload,
        rocksdb_cloud_fs_options_get_use_wal_delta_upload
    );

    // Fallback bucket options

    /// Add a fallback bucket to search when an SST file is not found in the
    /// dest or src buckets. Fallbacks are tried in the order they are added.
    pub fn add_fallback_bucket(&mut self, bucket: &CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_add_fallback_bucket(self.inner, bucket.inner);
        }
    }

    /// Returns the number of fallback buckets configured.
    pub fn num_fallback_buckets(&self) -> usize {
        unsafe { ffi::rocksdb_cloud_fs_options_get_num_fallback_buckets(self.inner) as usize }
    }

    /// Remove all fallback buckets.
    pub fn clear_fallback_buckets(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_clear_fallback_buckets(self.inner);
        }
    }

    /// Add a replication bucket for cross-region SST/MANIFEST/CLOUDMANIFEST
    /// replication. SSTs are replicated asynchronously; metadata files are
    /// gated on SST completion.
    pub fn add_replication_bucket(&mut self, bucket: &CloudBucketOptions) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_add_replication_bucket(self.inner, bucket.inner);
        }
    }

    /// Returns the number of replication buckets configured.
    pub fn num_replication_buckets(&self) -> usize {
        unsafe { ffi::rocksdb_cloud_fs_options_get_num_replication_buckets(self.inner) as usize }
    }

    /// Remove all replication buckets.
    pub fn clear_replication_buckets(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_clear_replication_buckets(self.inner);
        }
    }

    /// Set a rate limiter for cloud upload operations (SST, MANIFEST, etc.).
    /// Pass rate_bytes_per_sec <= 0 to disable throttling.
    pub fn set_cloud_upload_rate_limiter(
        &mut self,
        rate_bytes_per_sec: i64,
        refill_period_us: i64,
        fairness: i32,
    ) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_cloud_upload_rate_limiter(
                self.inner,
                rate_bytes_per_sec,
                refill_period_us,
                fairness,
            );
        }
    }

    /// Set a rate limiter for cloud download operations (SST, MANIFEST, range reads).
    /// Pass rate_bytes_per_sec <= 0 to disable throttling.
    pub fn set_cloud_download_rate_limiter(
        &mut self,
        rate_bytes_per_sec: i64,
        refill_period_us: i64,
        fairness: i32,
    ) {
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_cloud_download_rate_limiter(
                self.inner,
                rate_bytes_per_sec,
                refill_period_us,
                fairness,
            );
        }
    }

    /// Set a custom S3-compatible endpoint URL (e.g. `http://localhost:9200` for MinIO).
    ///
    /// When set, the AWS SDK connects to this endpoint instead of the default
    /// AWS S3 endpoint for the configured region.
    pub fn set_endpoint_override(&mut self, endpoint: impl CStrLike) -> &mut Self {
        let endpoint = endpoint.into_c_string().unwrap();
        unsafe {
            ffi::rocksdb_cloud_fs_options_set_endpoint_override(self.inner, endpoint.as_ptr());
        }
        self
    }
}
