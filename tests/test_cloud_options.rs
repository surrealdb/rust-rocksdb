#![cfg(feature = "cloud")]

use rocksdb::{
    AwsAccessType, CloudBucketOptions, CloudCheckpointOptions, CloudCredentials,
    CloudFileSystemOptions, WalKafkaSyncMode,
};

#[test]
fn test_cloud_bucket_options_default() {
    let opts = CloudBucketOptions::default();
    assert_eq!(opts.get_bucket_name(), "");
    assert_eq!(opts.get_prefix(), "");
    assert_eq!(opts.get_region(), "");
    assert_eq!(opts.get_object_path(), "");
}

#[test]
fn test_cloud_bucket_options_set_get() {
    let mut opts = CloudBucketOptions::default();
    opts.set_bucket_name("my-bucket");
    opts.set_prefix("prefix");
    opts.set_region("us-east-1");
    opts.set_object_path("/path/to/db");
    assert_eq!(opts.get_bucket_name(), "my-bucket");
    assert_eq!(opts.get_prefix(), "prefix");
    assert_eq!(opts.get_region(), "us-east-1");
    assert_eq!(opts.get_object_path(), "/path/to/db");
}

#[test]
fn test_cloud_bucket_options_is_valid() {
    let opts = CloudBucketOptions::default();
    assert!(!opts.is_valid());

    let mut opts = CloudBucketOptions::default();
    opts.set_bucket_name("my-bucket");
    assert!(opts.is_valid());
}

#[test]
fn test_cloud_credentials_default() {
    let creds = CloudCredentials::default();
    drop(creds);
}

#[test]
fn test_cloud_credentials_access_type() {
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Simple);
    assert_eq!(creds.get_type(), AwsAccessType::Simple);

    creds.set_type(AwsAccessType::Environment);
    assert_eq!(creds.get_type(), AwsAccessType::Environment);

    creds.set_type(AwsAccessType::Anonymous);
    assert_eq!(creds.get_type(), AwsAccessType::Anonymous);
}

#[test]
fn test_cloud_fs_options_default() {
    let opts = CloudFileSystemOptions::default();
    let _ = opts.get_keep_local_sst_files();
    drop(opts);
}

#[test]
fn test_cloud_fs_options_boolean_setters() {
    let mut opts = CloudFileSystemOptions::default();
    opts.set_keep_local_sst_files(true);
    assert!(opts.get_keep_local_sst_files());
    opts.set_keep_local_sst_files(false);
    assert!(!opts.get_keep_local_sst_files());
}

#[test]
fn test_cloud_fs_options_more_booleans() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_server_side_encryption(true);
    assert!(opts.get_server_side_encryption());

    opts.set_create_bucket_if_missing(true);
    assert!(opts.get_create_bucket_if_missing());

    opts.set_skip_dbid_verification(true);
    assert!(opts.get_skip_dbid_verification());

    opts.set_resync_on_open(true);
    assert!(opts.get_resync_on_open());

    opts.set_roll_cloud_manifest_on_open(true);
    assert!(opts.get_roll_cloud_manifest_on_open());
}

#[test]
fn test_cloud_fs_options_numeric() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_request_timeout_ms(5000);
    assert_eq!(opts.get_request_timeout_ms(), 5000);

    opts.set_warm_connection_pool_size(8);
    assert_eq!(opts.get_warm_connection_pool_size(), 8);

    opts.set_purger_periodicity_millis(60000);
    assert_eq!(opts.get_purger_periodicity_millis(), 60000);
}

#[test]
fn test_cloud_fs_options_string() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_encryption_key_id("my-key-id");
    assert_eq!(opts.get_encryption_key_id(), "my-key-id");

    opts.set_cookie_on_open("cookie-value");
    assert_eq!(opts.get_cookie_on_open(), "cookie-value");

    opts.set_new_cookie_on_open("new-cookie");
    assert_eq!(opts.get_new_cookie_on_open(), "new-cookie");
}

#[test]
fn test_cloud_checkpoint_options() {
    let mut opts = CloudCheckpointOptions::default();
    opts.set_thread_count(4);
    assert_eq!(opts.get_thread_count(), 4);
    opts.set_flush_memtable(true);
    assert!(opts.get_flush_memtable());
}

#[test]
fn test_cloud_fs_options_buckets() {
    let mut opts = CloudFileSystemOptions::default();
    let mut src = CloudBucketOptions::default();
    src.set_bucket_name("src-bucket");
    opts.set_src_bucket(&src);

    let mut dest = CloudBucketOptions::default();
    dest.set_bucket_name("dest-bucket");
    opts.set_dest_bucket(&dest);

    drop(opts);
}

#[test]
fn test_cloud_fs_options_fallback_buckets() {
    let mut opts = CloudFileSystemOptions::default();
    assert_eq!(opts.num_fallback_buckets(), 0);

    let mut fb1 = CloudBucketOptions::default();
    fb1.set_bucket_name("fallback-1");
    opts.add_fallback_bucket(&fb1);
    assert_eq!(opts.num_fallback_buckets(), 1);

    let mut fb2 = CloudBucketOptions::default();
    fb2.set_bucket_name("fallback-2");
    opts.add_fallback_bucket(&fb2);
    assert_eq!(opts.num_fallback_buckets(), 2);

    opts.clear_fallback_buckets();
    assert_eq!(opts.num_fallback_buckets(), 0);
}

#[test]
fn test_cloud_fs_options_replication_buckets() {
    let mut opts = CloudFileSystemOptions::default();
    assert_eq!(opts.num_replication_buckets(), 0);

    let mut rb = CloudBucketOptions::default();
    rb.set_bucket_name("replica-bucket");
    opts.add_replication_bucket(&rb);
    assert_eq!(opts.num_replication_buckets(), 1);

    opts.clear_replication_buckets();
    assert_eq!(opts.num_replication_buckets(), 0);
}

#[test]
fn test_cloud_fs_options_credentials() {
    let mut opts = CloudFileSystemOptions::default();
    let creds = CloudCredentials::default();
    opts.set_credentials(&creds);
    drop(opts);
}

#[test]
fn test_cloud_fs_options_wal_booleans() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_keep_local_log_files(false);
    assert!(!opts.get_keep_local_log_files());
    opts.set_keep_local_log_files(true);
    assert!(opts.get_keep_local_log_files());

    opts.set_background_wal_sync_to_cloud(true);
    assert!(opts.get_background_wal_sync_to_cloud());
    opts.set_background_wal_sync_to_cloud(false);
    assert!(!opts.get_background_wal_sync_to_cloud());
}

#[test]
fn test_cloud_fs_options_wal_kafka_mode() {
    let mut opts = CloudFileSystemOptions::default();
    assert_eq!(opts.get_kafka_wal_sync_mode(), WalKafkaSyncMode::None);

    opts.set_kafka_wal_sync_mode(WalKafkaSyncMode::PerAppend);
    assert_eq!(opts.get_kafka_wal_sync_mode(), WalKafkaSyncMode::PerAppend);

    opts.set_kafka_wal_sync_mode(WalKafkaSyncMode::PerSync);
    assert_eq!(opts.get_kafka_wal_sync_mode(), WalKafkaSyncMode::PerSync);

    opts.set_kafka_wal_sync_mode(WalKafkaSyncMode::None);
    assert_eq!(opts.get_kafka_wal_sync_mode(), WalKafkaSyncMode::None);
}

#[test]
fn test_cloud_fs_options_wal_strings() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_kafka_bootstrap_servers("broker1:9092,broker2:9092");
    assert_eq!(
        opts.get_kafka_bootstrap_servers(),
        "broker1:9092,broker2:9092"
    );

    opts.set_kafka_topic_prefix("my-wal-prefix");
    assert_eq!(opts.get_kafka_topic_prefix(), "my-wal-prefix");
}

#[test]
fn test_cloud_fs_options_wal_interval() {
    let mut opts = CloudFileSystemOptions::default();

    opts.set_background_wal_sync_interval_ms(2000);
    assert_eq!(opts.get_background_wal_sync_interval_ms(), 2000);

    opts.set_background_wal_sync_interval_ms(10000);
    assert_eq!(opts.get_background_wal_sync_interval_ms(), 10000);
}
