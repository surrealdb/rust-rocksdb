# Cloud Storage Guide

This guide covers all cloud storage functionality added to `rust-rocksdb` by
SurrealDB, including setup, configuration, usage examples, encryption, and
the full option reference.

---

## Table of contents

- [Feature flags](#feature-flags)
- [Quick start](#quick-start)
- [Credentials](#credentials)
- [Bucket configuration](#bucket-configuration)
- [Cloud file system options](#cloud-file-system-options)
- [Cloud file system](#cloud-file-system)
- [Opening a cloud database](#opening-a-cloud-database)
  - [CloudDB](#clouddb)
  - [CloudTransactionDB](#cloudtransactiondb)
  - [CloudOptimisticTransactionDB](#cloudoptimistictransactiondb)
- [Column families](#column-families)
- [Read-only replicas](#read-only-replicas)
  - [Basic replica pattern](#basic-replica-pattern)
  - [Replica with column families](#replica-with-column-families)
  - [How it works](#how-it-works)
  - [Limitations](#limitations)
- [Cloud checkpoints](#cloud-checkpoints)
- [Incremental backups](#incremental-backups)
  - [Basic usage](#basic-usage)
  - [Backing up to S3](#backing-up-to-s3)
  - [Automated backup loop](#automated-backup-loop)
  - [Restoring from backup](#restoring-from-backup)
  - [Combining backups with encryption](#combining-backups-with-encryption)
- [Zero-copy branching](#zero-copy-branching)
  - [Fork points](#fork-points)
  - [Fallback buckets](#fallback-buckets)
- [Cross-region replication](#cross-region-replication)
- [Bandwidth throttling](#bandwidth-throttling)
- [Encryption at rest](#encryption-at-rest)
- [SST file manager](#sst-file-manager)
- [Resuming after errors](#resuming-after-errors)
- [Full option reference](#full-option-reference)

---

## Feature flags

Enable cloud support by adding the appropriate feature flags in `Cargo.toml`:

```toml
[dependencies]
surrealdb-rocksdb = { version = "0.24", features = ["aws"] }
```

| Feature      | Description                                         | Implies  |
|--------------|-----------------------------------------------------|----------|
| `cloud`      | Core cloud types and `CloudFileSystem` abstraction  | —        |
| `aws`        | Amazon S3 storage backend (links AWS C++ SDK)       | `cloud`  |
| `gcs`        | Google Cloud Storage backend (links `google-cloud-cpp`) | `cloud` |
| `encryption` | Data-at-rest encryption via OpenSSL AES-CTR         | —        |

The `aws` and `gcs` features each imply `cloud`, so you only need to specify
the backend you want. You can enable both simultaneously for multi-cloud
deployments.

### System dependencies

- **`aws`** — requires the AWS SDK for C++ (`aws-cpp-sdk-s3`,
  `aws-cpp-sdk-core`, `aws-cpp-sdk-transfer`) installed and discoverable by
  the linker.
- **`gcs`** — requires `google-cloud-cpp` (specifically `google_cloud_cpp_storage`)
  installed and discoverable via `pkg-config`.
- **`encryption`** — requires OpenSSL development libraries (`libssl-dev` /
  `openssl`), discovered via `pkg-config`.

---

## Quick start

A minimal example that opens a database backed by S3:

```rust
use rocksdb::{
    Options,
    CloudCredentials, AwsAccessType,
    CloudBucketOptions, CloudFileSystemOptions,
    CloudFileSystem, CloudDB,
};

fn main() -> Result<(), rocksdb::Error> {
    // 1. Configure credentials
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    // 2. Configure the destination bucket
    let mut bucket = CloudBucketOptions::default();
    bucket.set_bucket_name("my-rocksdb-bucket");
    bucket.set_region("us-east-1");
    bucket.set_object_path("db/production");

    // 3. Configure the cloud file system
    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&bucket);
    cloud_opts.set_create_bucket_if_missing(true);
    cloud_opts.set_keep_local_sst_files(false);

    // 4. Create the cloud file system
    let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

    // 5. Open the database
    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    db_opts.set_env(&cloud_fs.create_cloud_env()?);

    let db = CloudDB::open(&db_opts, &cloud_fs, "/tmp/local_db")?;

    // Use the database normally
    db.put(b"hello", b"world")?;
    let val = db.get(b"hello")?;
    assert_eq!(val.as_deref(), Some(b"world".as_ref()));

    // Flush and close to ensure all data reaches cloud storage
    db.close()?;

    Ok(())
}
```

---

## Credentials

`CloudCredentials` configures how the cloud storage backend authenticates
with the provider.

```rust
use rocksdb::{CloudCredentials, AwsAccessType};

// Use environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
let mut creds = CloudCredentials::default();
creds.set_type(AwsAccessType::Environment);

// Use explicit access key and secret
let mut creds = CloudCredentials::default();
creds.initialize_simple("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");

// Use an AWS config/credentials file
let mut creds = CloudCredentials::default();
creds.initialize_config("/home/user/.aws/credentials");

// Validate credentials
assert!(creds.has_valid().unwrap());
```

### AwsAccessType variants

| Variant       | Description                                          |
|---------------|------------------------------------------------------|
| `Simple`      | Explicit access key ID and secret key                |
| `Config`      | Read from an AWS config/credentials file             |
| `Instance`    | Use EC2 instance metadata (instance profile)         |
| `TaskRole`    | Use ECS task role credentials                        |
| `Environment` | Read from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` environment variables |
| `Anonymous`   | No authentication (public buckets only)              |
| `Undefined`   | Not yet configured                                   |

---

## Bucket configuration

`CloudBucketOptions` specifies the cloud storage location for a database.

```rust
use rocksdb::CloudBucketOptions;

let mut bucket = CloudBucketOptions::default();
bucket.set_bucket_name("my-bucket");
bucket.set_region("eu-west-1");
bucket.set_prefix("rocksdb/");           // optional key prefix
bucket.set_object_path("databases/mydb"); // logical path within the bucket

// Read getters
assert_eq!(bucket.get_bucket_name(), "my-bucket");
assert_eq!(bucket.get_region(), "eu-west-1");
assert!(bucket.is_valid());
```

### Reading from environment variables

`CloudBucketOptions` can be populated from environment variables with a given
prefix. For a prefix of `ROCKSDB_SRC`, it reads `ROCKSDB_SRC_BUCKET_NAME`,
`ROCKSDB_SRC_REGION`, and `ROCKSDB_SRC_OBJECT_PATH`.

```rust
use rocksdb::CloudBucketOptions;

let bucket = CloudBucketOptions::default()
    .read_from_env("ROCKSDB_DEST");
```

### Source and destination buckets

The cloud file system supports separate source and destination buckets:

- **Destination bucket** — where new SST files, manifests, and WAL are
  written. This is the primary storage location.
- **Source bucket** — an optional read-only bucket from which the database
  can read pre-existing files. Useful for cloning or migrating databases.

```rust
use rocksdb::{CloudBucketOptions, CloudFileSystemOptions};

let mut src = CloudBucketOptions::default();
src.set_bucket_name("source-bucket");
src.set_region("us-east-1");
src.set_object_path("db/snapshot-2024");

let mut dest = CloudBucketOptions::default();
dest.set_bucket_name("dest-bucket");
dest.set_region("us-east-1");
dest.set_object_path("db/active");

let mut cloud_opts = CloudFileSystemOptions::default();
cloud_opts.set_src_bucket(&src);
cloud_opts.set_dest_bucket(&dest);
```

---

## Cloud file system options

`CloudFileSystemOptions` controls how RocksDB interacts with cloud storage.
All options have sensible defaults. Configure only what you need.

```rust
use rocksdb::CloudFileSystemOptions;

let mut opts = CloudFileSystemOptions::default();

// Keep SST files locally after upload (avoids re-download on read)
opts.set_keep_local_sst_files(true);

// Automatically create the bucket if it doesn't exist
opts.set_create_bucket_if_missing(true);

// Enable the background purger to clean up obsolete cloud files
opts.set_run_purger(true);
opts.set_purger_periodicity_millis(60_000);

// Set cloud request timeout
opts.set_request_timeout_ms(30_000);

// Enable server-side encryption (SSE-S3 / SSE-KMS)
opts.set_server_side_encryption(true);
opts.set_encryption_key_id("alias/my-kms-key");

// Configure persistent cache (local SSD cache for cloud reads)
opts.set_persistent_cache_path("/mnt/ssd/cache");
opts.set_persistent_cache_size_gb(50);

// Cold start optimizations
opts.set_skip_cloud_listing_on_open(true);
opts.set_warm_connection_pool_size(4);
```

See the [full option reference](#full-option-reference) for every available
getter and setter.

---

## Cloud file system

`CloudFileSystem` is created from `CloudFileSystemOptions` and provides the
cloud-backed `Env` used to open databases.

```rust
use rocksdb::{CloudFileSystem, CloudFileSystemOptions, Options};

let cloud_opts = CloudFileSystemOptions::default();
// ... configure options ...

let cloud_fs = CloudFileSystem::new(&cloud_opts)?;
let env = cloud_fs.create_cloud_env()?;

let mut db_opts = Options::default();
db_opts.set_env(&env);
```

`CloudFileSystem` is `Clone`, `Send`, and `Sync` — it can be shared across
threads and used to open multiple databases.

---

## Opening a cloud database

Three database types support cloud storage. All provide the same read/write
API as their non-cloud counterparts, with additional cloud-specific methods.

### CloudDB

The simplest cloud database. Supports the full `DBCommon` API (get, put,
delete, merge, iterators, snapshots, column families).

```rust
use rocksdb::{Options, CloudDB, CloudFileSystem, CloudFileSystemOptions};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

// Open
let db = CloudDB::open(&opts, &cloud_fs, "/tmp/cloud_db")?;

// Read-only open
let db = CloudDB::open_read_only(&opts, &cloud_fs, "/tmp/cloud_db")?;

// Standard operations
db.put(b"key", b"value")?;
let value = db.get(b"key")?;
db.delete(b"key")?;

// Create a savepoint (sync local state to cloud)
db.savepoint()?;

// Flush and close
db.close()?;
```

### CloudTransactionDB

Pessimistic transactions over a cloud-backed database. Ideal for workloads
requiring strict serializable isolation.

```rust
use rocksdb::{
    Options, TransactionDBOptions, CloudFileSystem,
    CloudFileSystemOptions, CloudTransactionDB,
};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

let txn_db_opts = TransactionDBOptions::default();

let db = CloudTransactionDB::open(
    &opts,
    &txn_db_opts,
    &cloud_fs,
    "/tmp/cloud_txn_db",
)?;

// Begin a transaction
let txn = db.transaction();
txn.put(b"account:1", b"1000")?;
txn.put(b"account:2", b"500")?;
txn.commit()?;

// Transaction with custom options
use rocksdb::{WriteOptions, TransactionOptions};
let mut write_opts = WriteOptions::default();
let mut txn_opts = TransactionOptions::default();
txn_opts.set_lock_timeout(5000); // 5 second lock timeout
let txn = db.transaction_opt(&write_opts, &txn_opts);
txn.put(b"key", b"value")?;
txn.commit()?;

// Direct writes (bypass transaction)
db.put(b"direct_key", b"direct_value")?;

// Iterators
use rocksdb::IteratorMode;
for item in db.iterator(IteratorMode::Start) {
    let (key, value) = item.unwrap();
    println!("{:?} => {:?}", key, value);
}

// Snapshots
let snapshot = db.snapshot();

// Flush to ensure data reaches cloud
db.flush()?;
```

### CloudOptimisticTransactionDB

Optimistic transactions over a cloud-backed database. Better throughput than
pessimistic transactions when conflicts are rare.

```rust
use rocksdb::{
    Options, CloudFileSystem, CloudFileSystemOptions,
    CloudOptimisticTransactionDB,
};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

let db = CloudOptimisticTransactionDB::open(
    &opts,
    &cloud_fs,
    "/tmp/cloud_otxn_db",
)?;

// Begin a transaction
let txn = db.transaction();
txn.put(b"key", b"value")?;
txn.commit()?;   // validates no conflicts at commit time

// With custom options
use rocksdb::{WriteOptions, OptimisticTransactionOptions};
let txn = db.transaction_opt(
    &WriteOptions::default(),
    &OptimisticTransactionOptions::default(),
);

// Write batches
use rocksdb::WriteBatchWithTransaction;
let mut batch = WriteBatchWithTransaction::<true>::default();
batch.put(b"k1", b"v1");
batch.put(b"k2", b"v2");
db.write(batch)?;

// Range delete on column families
// db.delete_range_cf(&cf, b"start", b"end")?;

db.close()?;
```

---

## Column families

All three cloud database types support column families.

```rust
use rocksdb::{
    Options, ColumnFamilyDescriptor, CloudDB,
    CloudFileSystem, CloudFileSystemOptions,
};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.create_missing_column_families(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

// Open with column family names (uses default options per CF)
let db = CloudDB::open_cf(
    &opts,
    &cloud_fs,
    "/tmp/cloud_cf_db",
    ["cf1", "cf2"],
)?;

// Open with column family descriptors (custom options per CF)
let mut cf_opts = Options::default();
cf_opts.set_max_write_buffer_number(4);

let db = CloudDB::open_cf_descriptors(
    &opts,
    &cloud_fs,
    "/tmp/cloud_cf_db",
    vec![
        ColumnFamilyDescriptor::new("cf1", Options::default()),
        ColumnFamilyDescriptor::new("cf2", cf_opts),
    ],
)?;

// List column families
let cfs = CloudDB::<rocksdb::SingleThreaded>::list_column_families(
    &opts,
    "/tmp/cloud_cf_db",
)?;
println!("Column families: {:?}", cfs);
```

Column family usage is identical for `CloudTransactionDB` and
`CloudOptimisticTransactionDB`:

```rust
use rocksdb::{
    Options, TransactionDBOptions, CloudFileSystem,
    CloudFileSystemOptions, CloudTransactionDB,
};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.create_missing_column_families(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

let db = CloudTransactionDB::open_cf(
    &opts,
    &TransactionDBOptions::default(),
    &cloud_fs,
    "/tmp/cloud_txn_cf_db",
    ["cf1"],
)?;

// Access column family handle
let cf = db.cf_handle("cf1").unwrap();
db.put_cf(&cf, b"key", b"value")?;

let txn = db.transaction();
txn.put_cf(&cf, b"txn_key", b"txn_value")?;
txn.commit()?;
```

---

## Read-only replicas

A read-only replica points at the same S3 bucket and object path as a
primary writer. Each time the replica opens, it downloads the latest
CLOUDMANIFEST and MANIFEST from S3 and replays any SST changes since the
last open. The replica never writes to cloud storage.

### Basic replica pattern

The primary database is opened normally. The replica uses
`CloudDB::open_read_only` with `resync_on_open` enabled so that every open
fetches the latest metadata from S3. To see new data written by the
primary, close and re-open the replica.

```rust
use rocksdb::{
    Options, CloudCredentials, AwsAccessType,
    CloudBucketOptions, CloudFileSystemOptions,
    CloudFileSystem, CloudDB,
};
use std::time::Duration;

fn open_replica(
    bucket_name: &str,
    region: &str,
    object_path: &str,
    local_path: &str,
) -> Result<CloudDB, rocksdb::Error> {
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    // Same bucket and object path as the primary writer
    let mut bucket = CloudBucketOptions::default();
    bucket.set_bucket_name(bucket_name);
    bucket.set_region(region);
    bucket.set_object_path(object_path);

    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&bucket);

    // Fetch fresh CLOUDMANIFEST / MANIFEST from S3 on every open
    cloud_opts.set_resync_on_open(true);
    // Do not roll a new epoch — the replica is read-only
    cloud_opts.set_roll_cloud_manifest_on_open(false);
    // Cache SST files locally so re-opens only download new files
    cloud_opts.set_keep_local_sst_files(true);
    // The replica must not delete cloud files
    cloud_opts.set_run_purger(false);
    cloud_opts.set_delete_cloud_invisible_files_on_open(false);
    // Bucket already exists
    cloud_opts.set_create_bucket_if_missing(false);
    // Skip DBID check — the replica's local DBID may not match
    cloud_opts.set_skip_dbid_verification(true);

    let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

    let mut db_opts = Options::default();
    db_opts.create_if_missing(false);
    db_opts.set_env(&cloud_fs.create_cloud_env()?);

    CloudDB::open_read_only(&db_opts, &cloud_fs, local_path)
}

// Periodically re-open to pick up the primary's latest writes
fn replica_loop() -> Result<(), rocksdb::Error> {
    loop {
        let db = open_replica(
            "my-rocksdb-bucket",
            "us-east-1",
            "db/production",
            "/tmp/replica_local",
        )?;

        if let Some(val) = db.get(b"some_key")? {
            println!("value: {:?}", val);
        }

        // Drop and re-open to catch up with the primary
        drop(db);
        std::thread::sleep(Duration::from_secs(5));
    }
}
```

### Replica with column families

Read-only replicas support column families through
`CloudDB::open_cf_descriptors`. The column families must match those
created by the primary.

```rust
use rocksdb::{
    Options, ColumnFamilyDescriptor, CloudCredentials, AwsAccessType,
    CloudBucketOptions, CloudFileSystemOptions, CloudFileSystem, CloudDB,
};

fn open_replica_with_cfs() -> Result<(), rocksdb::Error> {
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    let mut bucket = CloudBucketOptions::default();
    bucket.set_bucket_name("my-rocksdb-bucket");
    bucket.set_region("us-east-1");
    bucket.set_object_path("db/production");

    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&bucket);
    cloud_opts.set_resync_on_open(true);
    cloud_opts.set_roll_cloud_manifest_on_open(false);
    cloud_opts.set_keep_local_sst_files(true);
    cloud_opts.set_run_purger(false);
    cloud_opts.set_delete_cloud_invisible_files_on_open(false);
    cloud_opts.set_create_bucket_if_missing(false);
    cloud_opts.set_skip_dbid_verification(true);

    let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

    let mut db_opts = Options::default();
    db_opts.create_if_missing(false);
    db_opts.create_missing_column_families(false);
    db_opts.set_env(&cloud_fs.create_cloud_env()?);

    let db = CloudDB::open_cf_descriptors(
        &db_opts,
        &cloud_fs,
        "/tmp/replica_cf",
        vec![
            ColumnFamilyDescriptor::new("cf1", Options::default()),
            ColumnFamilyDescriptor::new("cf2", Options::default()),
        ],
    )?;

    let cf = db.cf_handle("cf1").unwrap();
    if let Some(val) = db.get_cf(&cf, b"key")? {
        println!("cf1 value: {:?}", val);
    }

    Ok(())
}
```

### How it works

When the primary writes data, SST files and MANIFEST updates are
automatically uploaded to S3 by the cloud file system. On the replica:

1. **`resync_on_open`** forces the cloud file system to download the
   latest CLOUDMANIFEST and MANIFEST from S3 during
   `SanitizeLocalDirectory`, even if local copies already exist.
2. **`keep_local_sst_files`** caches SST files on the replica's local
   disk. On subsequent opens only newly-created SSTs are downloaded,
   making re-opens fast.
3. **`roll_cloud_manifest_on_open(false)`** prevents the replica from
   creating a new cloud manifest epoch, which would conflict with the
   primary.
4. The replica opens with `DB::OpenForReadOnly` under the hood, so all
   write operations return an error.

### WAL recovery

When the primary is configured with `background_wal_sync_to_cloud` or
`kafka_wal_sync_mode` (or both), WAL files are shipped to S3 and/or
Kafka. On open, the cloud file system automatically recovers WAL data
from these sources before RocksDB replays it. This allows both the
primary (after a crash) and replicas to see data that was written but
not yet flushed to SSTs.

To enable WAL recovery on a replica, set
`background_wal_sync_to_cloud(true)` and/or the same Kafka WAL options
as the primary. The replica's `CloudFileSystemOptions` must match the
primary's WAL configuration so that recovery knows where to fetch WAL
data from.

### Limitations

- **Point-in-time snapshots.** Without WAL recovery enabled, each open is
  a frozen snapshot of the primary's state at the time of the last flush
  or compaction. When WAL recovery is enabled (via S3 and/or Kafka), the
  replica can also see unflushed writes that were shipped to cloud/Kafka
  before the replica opened.
- **No live tailing.** Unlike `DB::open_as_secondary` (which supports
  `try_catch_up_with_primary` for local-disk secondaries), the cloud
  read-only open does not support incremental catch-up. Each refresh
  requires a full close/re-open cycle.
- **Single primary writer.** Only one database instance may write to a
  given cloud bucket and object path at a time. Multiple read-only
  replicas may read from the same path concurrently.

---

## Cloud checkpoints

Cloud checkpoints copy the current database state to another cloud location.
This is useful for creating backups or cloning databases.

```rust
use rocksdb::{
    CloudBucketOptions, CloudCheckpointOptions, CloudDB,
    CloudFileSystem, CloudFileSystemOptions, Options,
};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;
let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

let db = CloudDB::open(&opts, &cloud_fs, "/tmp/cloud_db")?;

// Configure the checkpoint destination
let mut dest = CloudBucketOptions::default();
dest.set_bucket_name("backup-bucket");
dest.set_region("us-west-2");
dest.set_object_path("backups/2024-01-15");

let mut cp_opts = CloudCheckpointOptions::default();
cp_opts.set_thread_count(4);       // parallel upload threads
cp_opts.set_flush_memtable(true);  // flush before checkpoint

db.checkpoint_to_cloud(&dest, &cp_opts)?;
```

---

## Incremental backups

`BackupEngine` provides a managed, incremental backup system on top of
RocksDB. Unlike cloud checkpoints (which copy the entire database state each
time), `BackupEngine` deduplicates SST and blob files across backups so that
only new or changed files are transferred. It also maintains a catalog of
numbered backups with verification, retention policies, and point-in-time
restore.

| Feature                     | Cloud checkpoint          | BackupEngine                      |
|-----------------------------|---------------------------|-----------------------------------|
| Incremental / deduplicated  | No (full copy each time)  | Yes (shared SST files by default) |
| Backup catalog              | No                        | Yes (numbered, with metadata)     |
| Point-in-time restore       | Manual (one snapshot)     | Any retained backup ID            |
| Integrity verification      | No                        | `verify_backup()` with checksums  |
| Retention policy            | Manual deletion           | `purge_old_backups(n)`            |
| Destination                 | Cloud bucket              | Any `Env` (local, S3, etc.)       |

### Basic usage

A minimal backup to a local directory:

```rust
use rocksdb::{
    backup::{BackupEngine, BackupEngineOptions, RestoreOptions},
    Env, DB, Options,
};

fn main() -> Result<(), rocksdb::Error> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    let db = DB::open(&opts, "/tmp/my_db")?;

    db.put(b"key", b"value")?;

    // Open a backup engine targeting a local directory
    let env = Env::new()?;
    let backup_opts = BackupEngineOptions::new("/tmp/my_backups")?;
    let mut backup_engine = BackupEngine::open(&backup_opts, &env)?;

    // Create a backup (flush_before_backup = true captures memtable data)
    backup_engine.create_new_backup_flush(&db, true)?;

    // Verify the latest backup
    let backups = backup_engine.get_backup_info();
    for b in &backups {
        backup_engine.verify_backup(b.backup_id)?;
        println!(
            "Backup #{}: {} bytes, {} files, ts={}",
            b.backup_id, b.size, b.num_files, b.timestamp,
        );
    }

    // Keep only the 3 most recent backups
    backup_engine.purge_old_backups(3)?;

    Ok(())
}
```

Subsequent calls to `create_new_backup` or `create_new_backup_flush` are
incremental — SST files already present in the backup directory are not
copied again. This is the default behavior (`share_table_files` defaults to
`true` in the underlying C++ engine).

### Backing up to S3

`BackupEngine::open` accepts any `Env`, including a cloud-backed one created
from `CloudFileSystem`. This makes backup files flow directly to S3:

```rust
use rocksdb::{
    backup::{BackupEngine, BackupEngineOptions},
    CloudBucketOptions, CloudCredentials, CloudFileSystem,
    CloudFileSystemOptions, AwsAccessType,
    CloudDB, Options,
};

fn backup_to_s3(db: &CloudDB) -> Result<(), rocksdb::Error> {
    // Configure a cloud file system pointing to the backup bucket
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    let mut backup_bucket = CloudBucketOptions::default();
    backup_bucket.set_bucket_name("my-backup-bucket");
    backup_bucket.set_region("us-east-1");
    backup_bucket.set_object_path("backups/production");

    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&backup_bucket);
    cloud_opts.set_create_bucket_if_missing(true);

    // Enable S3 server-side encryption (SSE-KMS)
    cloud_opts.set_server_side_encryption(true);
    cloud_opts.set_encryption_key_id("arn:aws:kms:us-east-1:123456789:key/my-key");

    let backup_fs = CloudFileSystem::new(&cloud_opts)?;
    let backup_env = backup_fs.create_cloud_env()?;

    // Open the backup engine — files are written to S3 through backup_env
    let backup_opts = BackupEngineOptions::new("backups/production")?;
    let mut engine = BackupEngine::open(&backup_opts, &backup_env)?;

    // Incremental backup (only new SSTs are uploaded)
    engine.create_new_backup_flush(db, true)?;

    // Verify and retain
    let info = engine.get_backup_info();
    if let Some(latest) = info.last() {
        engine.verify_backup(latest.backup_id)?;
    }
    engine.purge_old_backups(5)?;

    Ok(())
}
```

The `BackupEngineOptions::new` path becomes a prefix within the cloud bucket.
The backup engine's incremental deduplication works across cloud-backed
backups the same way it does locally — shared SST files are stored once and
referenced by multiple backup IDs.

### Automated backup loop

`BackupEngine` is `Send`, so it can be driven from a background thread or a
`tokio::task::spawn_blocking` closure. Here is a minimal periodic backup
loop:

```rust
use std::sync::Arc;
use std::time::Duration;
use rocksdb::{
    backup::{BackupEngine, BackupEngineOptions},
    Env, DB,
};

fn spawn_backup_loop(
    db: Arc<DB>,
    backup_dir: String,
    interval: Duration,
    max_backups: usize,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);

        let env = match Env::new() {
            Ok(e) => e,
            Err(e) => { eprintln!("env error: {e}"); continue; }
        };
        let opts = match BackupEngineOptions::new(&backup_dir) {
            Ok(o) => o,
            Err(e) => { eprintln!("opts error: {e}"); continue; }
        };
        let mut engine = match BackupEngine::open(&opts, &env) {
            Ok(e) => e,
            Err(e) => { eprintln!("open error: {e}"); continue; }
        };

        if let Err(e) = engine.create_new_backup_flush(&*db, true) {
            eprintln!("backup failed: {e}");
            continue;
        }

        // Verify the new backup
        let info = engine.get_backup_info();
        if let Some(latest) = info.last() {
            if let Err(e) = engine.verify_backup(latest.backup_id) {
                eprintln!("verification failed for #{}: {e}", latest.backup_id);
            }
        }

        if let Err(e) = engine.purge_old_backups(max_backups) {
            eprintln!("purge failed: {e}");
        }
    })
}
```

To back up to S3 instead, replace the `Env::new()` call with a cloud env
created from a `CloudFileSystem` (see [Backing up to S3](#backing-up-to-s3)).

### Restoring from backup

Restore to a target directory (which must differ from the backup directory):

```rust
use rocksdb::{
    backup::{BackupEngine, BackupEngineOptions, RestoreOptions},
    Env, DB, Options,
};

fn restore_latest(backup_dir: &str, restore_dir: &str) -> Result<(), rocksdb::Error> {
    let env = Env::new()?;
    let opts = BackupEngineOptions::new(backup_dir)?;
    let mut engine = BackupEngine::open(&opts, &env)?;

    let restore_opts = RestoreOptions::default();
    engine.restore_from_latest_backup(restore_dir, restore_dir, &restore_opts)?;

    // Open the restored database
    let db = DB::open_default(restore_dir)?;
    let val = db.get(b"key")?;
    println!("restored value: {:?}", val);

    Ok(())
}

fn restore_specific(backup_dir: &str, restore_dir: &str) -> Result<(), rocksdb::Error> {
    let env = Env::new()?;
    let opts = BackupEngineOptions::new(backup_dir)?;
    let mut engine = BackupEngine::open(&opts, &env)?;

    // List available backups
    for b in engine.get_backup_info() {
        println!("Backup #{}: ts={}, size={}", b.backup_id, b.timestamp, b.size);
    }

    // Restore a specific backup by ID
    let restore_opts = RestoreOptions::default();
    engine.restore_from_backup(restore_dir, restore_dir, &restore_opts, 3)?;

    Ok(())
}
```

`RestoreOptions` supports `set_keep_log_files(true)` to preserve existing
WAL files in the restore directory, which can be combined with
`BackupEngineOptions::backup_log_files(false)` for in-memory database
persistence workflows.

### Combining backups with encryption

When the primary database uses client-side encryption (via the `encryption`
feature), `BackupEngine` reads decrypted data in process memory during the
copy. To keep backup files encrypted at rest, configure encryption on the
backup side as well:

```rust
use rocksdb::{
    backup::{BackupEngine, BackupEngineOptions},
    CloudFileSystemOptions, CloudFileSystem,
    Options, CloudDB,
};

fn encrypted_backup_to_s3(db: &CloudDB) -> Result<(), rocksdb::Error> {
    let mut cloud_opts = CloudFileSystemOptions::default();
    // ... configure backup bucket and credentials ...

    // Server-side encryption on the backup bucket (SSE-KMS)
    cloud_opts.set_server_side_encryption(true);
    cloud_opts.set_encryption_key_id("arn:aws:kms:us-east-1:123456789:key/backup-key");

    let backup_fs = CloudFileSystem::new(&cloud_opts)?;
    let backup_env = backup_fs.create_cloud_env()?;

    let opts = BackupEngineOptions::new("backups/encrypted")?;
    let mut engine = BackupEngine::open(&opts, &backup_env)?;
    engine.create_new_backup_flush(db, true)?;

    Ok(())
}
```

For defense-in-depth, you can also layer client-side encryption on the backup
env using `create_encrypted_env` (see [Encryption at rest](#encryption-at-rest)),
providing double encryption: AES-CTR before the data leaves the process,
plus SSE-KMS at the S3 storage layer.

---

## Zero-copy branching

Zero-copy branching allows creating lightweight database branches that share
SST files with a parent database without copying data.

### Fork points

A fork point captures the current position in the cloud manifest. It records
the epoch, next file number, and cloud manifest cookie — everything needed
to create a branch that reads the parent's SST files.

```rust
use rocksdb::{CloudDB, CloudFileSystem, CloudFileSystemOptions, Options};

let cloud_fs = CloudFileSystem::new(&CloudFileSystemOptions::default())?;
let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);

let db = CloudDB::open(&opts, &cloud_fs, "/tmp/parent_db")?;

// Write some data
db.put(b"key", b"value")?;

// Capture a fork point (metadata-only, very fast)
let fork_point = db.capture_fork_point()?;
println!("Epoch: {}", fork_point.epoch);
println!("File number: {}", fork_point.file_number);
println!("Cookie: {}", fork_point.cloud_manifest_cookie);
```

Fork points are available on all three cloud database types:
- `CloudDB::capture_fork_point()`
- `CloudTransactionDB::capture_fork_point()`
- `CloudOptimisticTransactionDB::capture_fork_point()`

The `ForkPoint` struct contains:

| Field                     | Type     | Description                          |
|---------------------------|----------|--------------------------------------|
| `epoch`                   | `String` | Current cloud manifest epoch         |
| `file_number`             | `u64`    | Next file number at the fork point   |
| `cloud_manifest_cookie`   | `String` | CLOUDMANIFEST cookie for consistency |

### Fallback buckets

Fallback buckets enable a child branch to read SST files from its parent's
storage location without copying them. When a file is not found in the
primary buckets, the cloud file system searches fallback buckets in order.

```rust
use rocksdb::{CloudBucketOptions, CloudFileSystemOptions};

let mut cloud_opts = CloudFileSystemOptions::default();

// Primary bucket for this branch
let mut dest = CloudBucketOptions::default();
dest.set_bucket_name("my-bucket");
dest.set_object_path("branches/child");
cloud_opts.set_dest_bucket(&dest);

// Add parent's bucket as a fallback
let mut parent_bucket = CloudBucketOptions::default();
parent_bucket.set_bucket_name("my-bucket");
parent_bucket.set_object_path("branches/parent");
cloud_opts.add_fallback_bucket(&parent_bucket);

// Add grandparent as another fallback
let mut grandparent_bucket = CloudBucketOptions::default();
grandparent_bucket.set_bucket_name("my-bucket");
grandparent_bucket.set_object_path("branches/grandparent");
cloud_opts.add_fallback_bucket(&grandparent_bucket);

// Check and manage fallbacks
assert_eq!(cloud_opts.num_fallback_buckets(), 2);
// cloud_opts.clear_fallback_buckets();  // remove all fallbacks
```

---

## Cross-region replication

Replication buckets replicate SST files, MANIFEST, and CLOUDMANIFEST to one or
more remote regions asynchronously. SST data is replicated in the background;
metadata files are only written to replication targets once the corresponding
SST uploads complete. This gives you cross-region durability without blocking
the write path.

```rust
use rocksdb::{
    CloudBucketOptions, CloudCredentials, CloudFileSystem,
    CloudFileSystemOptions, AwsAccessType, CloudDB, Options,
};

fn main() -> Result<(), rocksdb::Error> {
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    // Primary bucket (us-east-1)
    let mut primary = CloudBucketOptions::default();
    primary.set_bucket_name("my-db-bucket-us-east-1");
    primary.set_region("us-east-1");
    primary.set_object_path("db/production");

    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&primary);
    cloud_opts.set_create_bucket_if_missing(true);

    // Replicate to eu-west-1
    let mut eu_replica = CloudBucketOptions::default();
    eu_replica.set_bucket_name("my-db-bucket-eu-west-1");
    eu_replica.set_region("eu-west-1");
    eu_replica.set_object_path("db/production");
    cloud_opts.add_replication_bucket(&eu_replica);

    // Replicate to ap-southeast-1
    let mut ap_replica = CloudBucketOptions::default();
    ap_replica.set_bucket_name("my-db-bucket-ap-southeast-1");
    ap_replica.set_region("ap-southeast-1");
    ap_replica.set_object_path("db/production");
    cloud_opts.add_replication_bucket(&ap_replica);

    assert_eq!(cloud_opts.num_replication_buckets(), 2);

    let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_env(&cloud_fs.create_cloud_env()?);

    let db = CloudDB::open(&opts, &cloud_fs, "/tmp/replicated_db")?;

    db.put(b"key", b"value")?;
    db.close()?;

    Ok(())
}
```

Use `clear_replication_buckets()` to remove all configured replication targets
(for example, before re-configuring them on a subsequent open).

---

## Bandwidth throttling

Per-instance upload and download rate limiting prevents a single database
from saturating network bandwidth. Uses RocksDB's built-in `RateLimiter`.

```rust
use rocksdb::CloudFileSystemOptions;

let mut cloud_opts = CloudFileSystemOptions::default();

// Limit uploads to 50 MB/s
cloud_opts.set_cloud_upload_rate_limiter(
    50 * 1024 * 1024,  // rate_bytes_per_sec
    100_000,           // refill_period_us (100ms)
    10,                // fairness
);

// Limit downloads to 100 MB/s
cloud_opts.set_cloud_download_rate_limiter(
    100 * 1024 * 1024, // rate_bytes_per_sec
    100_000,           // refill_period_us (100ms)
    10,                // fairness
);

// Pass 0 or negative rate_bytes_per_sec to disable throttling
cloud_opts.set_cloud_upload_rate_limiter(0, 0, 0);
```

| Parameter          | Description                                                |
|--------------------|------------------------------------------------------------|
| `rate_bytes_per_sec` | Maximum bytes per second (0 or negative disables limiter) |
| `refill_period_us` | How often the token bucket is refilled (microseconds)      |
| `fairness`         | Priority fairness factor (higher = more fair)              |

---

## Cold start optimization

When running RocksDB in serverless or ephemeral environments, the time to
open a cloud-backed database can be a bottleneck. The following options reduce
cold start latency.

### Skip cloud listing during open

During `DB::Open`, RocksDB calls `GetChildren` multiple times. By default,
each call lists objects in all configured cloud buckets — an expensive
operation when there are many files or fallback buckets. When
`skip_cloud_listing_on_open` is enabled, cloud listing is suppressed during
the open phase only. After `DB::Open` returns, `GetChildren` resumes normal
behavior.

This is safe when `resync_on_open` is true, because the freshly-fetched
MANIFEST is authoritative for the set of live files.

```rust
use rocksdb::CloudFileSystemOptions;

let mut cloud_opts = CloudFileSystemOptions::default();
cloud_opts.set_resync_on_open(true);
cloud_opts.set_skip_cloud_listing_on_open(true);
```

### Connection pool pre-warming

TLS handshakes to S3/GCS can add 100-300ms per connection. By default, only
one connection is established during initialization (for the bucket existence
check). Setting `warm_connection_pool_size` to a positive value causes that
many lightweight HEAD requests to be issued in parallel during
initialization, pre-establishing TLS connections before the first real
download.

```rust
use rocksdb::CloudFileSystemOptions;

let mut cloud_opts = CloudFileSystemOptions::default();
cloud_opts.set_warm_connection_pool_size(4);
```

### Initial table load limit

When opening a database, RocksDB eagerly loads metadata (index, filter) for
a limited number of SST files. The rest are opened lazily on first access,
which can cause latency spikes on the first queries. The
`initial_table_load_limit` option on `Options` controls this (for positive
values the effective limit is `min(limit, table_cache_capacity / 4)`):

- Default (`16`): load at most 16 tables per column family during open
- `0`: load all tables (eliminates first-query latency at the cost of
  longer open time)
- `-1`: use `table_cache_capacity / 4` with no additional limit

Pair with `max_file_opening_threads` to parallelize the table opens.

```rust
use rocksdb::Options;

let mut opts = Options::default();
opts.set_initial_table_load_limit(0);
opts.set_max_file_opening_threads(32);
```

### Combined example

A serverless-optimized open combining all cold start options:

```rust
use rocksdb::{
    Options, CloudFileSystemOptions, CloudFileSystem,
    CloudDB, CloudBucketOptions, CloudCredentials, AwsAccessType,
};

fn open_serverless() -> Result<(), rocksdb::Error> {
    let mut creds = CloudCredentials::default();
    creds.set_type(AwsAccessType::Environment);

    let mut bucket = CloudBucketOptions::default();
    bucket.set_bucket_name("my-bucket");
    bucket.set_region("us-east-1");
    bucket.set_object_path("db/production");

    let mut cloud_opts = CloudFileSystemOptions::default();
    cloud_opts.set_credentials(&creds);
    cloud_opts.set_dest_bucket(&bucket);

    // Cold start optimizations
    cloud_opts.set_resync_on_open(true);
    cloud_opts.set_skip_cloud_listing_on_open(true);
    cloud_opts.set_warm_connection_pool_size(4);

    let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_env(&cloud_fs.create_cloud_env()?);
    opts.set_initial_table_load_limit(0);
    opts.set_max_file_opening_threads(32);

    let db = CloudDB::open(&opts, &cloud_fs, "/tmp/serverless_db")?;
    Ok(())
}
```

---

## Encryption at rest

The `encryption` feature provides transparent data-at-rest encryption using
OpenSSL AES-CTR. You implement the `KeyManager` trait to control which key
and IV is used for each file.

### Enabling encryption

Add the feature flag:

```toml
[dependencies]
surrealdb-rocksdb = { version = "0.24", features = ["aws", "encryption"] }
```

### Implementing a key manager

```rust
use rocksdb::encryption::{
    EncryptionMethod, FileEncryptionInfo, KeyManager,
};
use rocksdb::Error;

struct MyKeyManager {
    master_key: [u8; 32],
}

impl KeyManager for MyKeyManager {
    fn get_file(&self, _fname: &str) -> Result<FileEncryptionInfo, Error> {
        Ok(FileEncryptionInfo {
            method: EncryptionMethod::Aes256Ctr,
            key: self.master_key.to_vec(),
            iv: vec![0u8; 16], // in production, derive per-file IVs
        })
    }

    fn new_file(&self, _fname: &str) -> Result<FileEncryptionInfo, Error> {
        Ok(FileEncryptionInfo {
            method: EncryptionMethod::Aes256Ctr,
            key: self.master_key.to_vec(),
            iv: vec![0u8; 16],
        })
    }

    fn delete_file(&self, _fname: &str) -> Result<(), Error> {
        Ok(())
    }

    fn link_file(&self, _src: &str, _dst: &str) -> Result<(), Error> {
        Ok(())
    }
}
```

### Using encryption with a database

```rust
use rocksdb::{DB, Options};
use rocksdb::encryption::create_encrypted_env;

let env = create_encrypted_env(MyKeyManager {
    master_key: [0x42; 32],
})?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&env);

let db = DB::open(&opts, "/tmp/encrypted_db")?;
db.put(b"secret", b"data")?;
```

### Combining encryption with cloud storage

```rust
use rocksdb::{Options, CloudDB, CloudFileSystem, CloudFileSystemOptions};
use rocksdb::encryption::create_encrypted_env;

// Create encrypted env
let enc_env = create_encrypted_env(MyKeyManager {
    master_key: [0x42; 32],
})?;

// Create cloud file system
let mut cloud_opts = CloudFileSystemOptions::default();
// ... configure cloud options ...
cloud_opts.set_server_side_encryption(true); // also enable SSE on S3

let cloud_fs = CloudFileSystem::new(&cloud_opts)?;

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_env(&cloud_fs.create_cloud_env()?);
// The encrypted env and cloud env can be layered as needed

let db = CloudDB::open(&opts, &cloud_fs, "/tmp/encrypted_cloud_db")?;
```

### EncryptionMethod variants

| Variant      | Key size | IV size | Description          |
|--------------|----------|---------|----------------------|
| `Plaintext`  | —        | —       | No encryption        |
| `Aes128Ctr`  | 16 bytes | 16 bytes| AES-128 in CTR mode  |
| `Aes192Ctr`  | 24 bytes | 16 bytes| AES-192 in CTR mode  |
| `Aes256Ctr`  | 32 bytes | 16 bytes| AES-256 in CTR mode  |
| `Sm4Ctr`     | 16 bytes | 16 bytes| SM4 in CTR mode      |

### KeyManager trait

All methods must be thread-safe (`Send + Sync`).

| Method        | Called when                        | Returns                  |
|---------------|------------------------------------|--------------------------|
| `get_file`    | Opening an existing file           | `FileEncryptionInfo`     |
| `new_file`    | Creating a new file                | `FileEncryptionInfo`     |
| `delete_file` | A file has been deleted            | `()`                     |
| `link_file`   | A file has been hard-linked/copied | `()`                     |

---

## SST file manager

`SstFileManager` tracks SST files and controls their deletion rate. Useful
for preventing disk bandwidth saturation during compaction cleanup.

```rust
use rocksdb::{Env, Options, DB, SstFileManager};

let env = Env::new()?;
let sst_mgr = SstFileManager::new(&env)?;

// Limit total SST space to 100 GB
sst_mgr.set_max_allowed_space_usage(100 * 1024 * 1024 * 1024);

// Reserve 10 GB buffer for compaction output
sst_mgr.set_compaction_buffer_size(10 * 1024 * 1024 * 1024);

// Throttle deletion to 100 MB/s
sst_mgr.set_delete_rate_bytes_per_second(100 * 1024 * 1024);

// Limit trash files to 25% of total
sst_mgr.set_max_trash_db_ratio(0.25);

let mut opts = Options::default();
opts.create_if_missing(true);
opts.set_sst_file_manager(&sst_mgr);

let db = DB::open(&opts, "/tmp/managed_db")?;

// Query manager state
println!("Total SST size: {} bytes", sst_mgr.get_total_size());
println!("Total trash size: {} bytes", sst_mgr.get_total_trash_size());
println!("Space limit reached: {}", sst_mgr.is_max_allowed_space_reached());
```

---

## Resuming after errors

When RocksDB encounters certain errors (e.g. no disk space), it pauses
background work. After the underlying issue is resolved, call `resume()`:

```rust
use rocksdb::{DB, Options};

let db = DB::open_default("/tmp/my_db")?;

// ... error occurs, background work pauses ...
// ... free disk space or fix the issue ...

db.resume()?; // resume background compaction, flushes, etc.
```

`resume()` is available on all database types: `DB`, `TransactionDB`,
`OptimisticTransactionDB`, and all their cloud variants.

---

## Full option reference

### CloudFileSystemOptions

#### Core configuration

| Method                    | Type                | Description                                    |
|---------------------------|---------------------|------------------------------------------------|
| `set_credentials`         | `&CloudCredentials` | Authentication credentials for cloud access    |
| `set_src_bucket`          | `&CloudBucketOptions` | Source (read-only) bucket                    |
| `set_dest_bucket`         | `&CloudBucketOptions` | Destination (read-write) bucket              |
| `set_persistent_cache_path` | `impl Into<String>` | Local path for persistent read cache        |
| `set_persistent_cache_size_gb` | `u64`          | Maximum size of the persistent cache in GB    |

#### Boolean options

| Setter / Getter                              | Default | Description                                              |
|----------------------------------------------|---------|----------------------------------------------------------|
| `set_keep_local_sst_files` / `get_keep_local_sst_files` | varies | Keep SST files locally after upload          |
| `set_validate_filesize` / `get_validate_filesize` | varies | Validate local vs. cloud file sizes              |
| `set_server_side_encryption` / `get_server_side_encryption` | `false` | Enable SSE on the cloud provider         |
| `set_create_bucket_if_missing` / `get_create_bucket_if_missing` | `false` | Auto-create bucket on first use        |
| `set_run_purger` / `get_run_purger`          | varies  | Run background thread to purge obsolete cloud files      |
| `set_resync_on_open` / `get_resync_on_open`  | varies  | Re-sync cloud state when opening the database            |
| `set_skip_dbid_verification` / `get_skip_dbid_verification` | `false` | Skip DB identity verification on open      |
| `set_use_aws_transfer_manager` / `get_use_aws_transfer_manager` | varies | Use AWS Transfer Manager for uploads   |
| `set_skip_cloud_files_in_getchildren` / `get_skip_cloud_files_in_getchildren` | varies | Skip cloud listing in GetChildren |
| `set_use_direct_io_for_cloud_download` / `get_use_direct_io_for_cloud_download` | `false` | Use O_DIRECT for cloud downloads  |
| `set_roll_cloud_manifest_on_open` / `get_roll_cloud_manifest_on_open` | varies | Roll the cloud manifest epoch on open |
| `set_delete_cloud_invisible_files_on_open` / `get_delete_cloud_invisible_files_on_open` | varies | Delete locally invisible cloud files on open |
| `set_skip_cloud_listing_on_open` / `get_skip_cloud_listing_on_open` | `false` | Skip cloud object listing during DB open |

#### Numeric options

| Setter / Getter                                                  | Type  | Description                                                |
|------------------------------------------------------------------|-------|------------------------------------------------------------|
| `set_purger_periodicity_millis` / `get_purger_periodicity_millis` | `u64` | How often the purger runs (milliseconds)                  |
| `set_request_timeout_ms` / `get_request_timeout_ms`              | `u64` | Cloud API request timeout (milliseconds)                  |
| `set_number_objects_listed_in_one_iteration` / `get_number_objects_listed_in_one_iteration` | `i32` | Page size for cloud list operations |
| `set_constant_sst_file_size_in_sst_file_manager` / `get_constant_sst_file_size_in_sst_file_manager` | `i64` | Override SST file size reported to the file manager |
| `set_cloud_file_deletion_delay_secs` / `get_cloud_file_deletion_delay_secs` | `u64` | Delay before deleting cloud files (seconds) |
| `set_warm_connection_pool_size` / `get_warm_connection_pool_size` | `i32` | Number of TLS connections to pre-warm (0 = disabled) |

#### String options

| Setter / Getter                                      | Description                                       |
|------------------------------------------------------|---------------------------------------------------|
| `set_encryption_key_id` / `get_encryption_key_id`    | KMS key ID for server-side encryption             |
| `set_cookie_on_open` / `get_cookie_on_open`          | CLOUDMANIFEST cookie to use when opening           |
| `set_new_cookie_on_open` / `get_new_cookie_on_open`  | New cookie to roll to on open                      |
| `set_kafka_bootstrap_servers` / `get_kafka_bootstrap_servers` | Kafka bootstrap servers (e.g. `"broker1:9092,broker2:9092"`) |
| `set_kafka_topic_prefix` / `get_kafka_topic_prefix`  | Prefix for the Kafka topic name (full topic: `<prefix>.<dest_bucket>`) |

#### WAL sync options

| Setter / Getter                                                          | Type               | Default       | Description                                                          |
|--------------------------------------------------------------------------|--------------------|---------------|----------------------------------------------------------------------|
| `set_keep_local_log_files` / `get_keep_local_log_files`                  | `bool`             | `true`        | Keep WAL files on the local filesystem                               |
| `set_background_wal_sync_to_cloud` / `get_background_wal_sync_to_cloud` | `bool`             | `false`       | Periodically upload WAL files to cloud storage in the background     |
| `set_background_wal_sync_interval_ms` / `get_background_wal_sync_interval_ms` | `u64`        | `5000`        | Interval between background WAL uploads (milliseconds)               |
| `set_kafka_wal_sync_mode` / `get_kafka_wal_sync_mode`                    | `WalKafkaSyncMode` | `None`        | When to publish WAL records to Kafka (`None`, `PerAppend`, `PerSync`) |
| `set_use_wal_delta_upload` / `get_use_wal_delta_upload`                  | `bool`             | `false`       | Upload only new WAL bytes as delta objects instead of full re-upload  |

### WalKafkaSyncMode

| Variant     | Value | Description                                |
|-------------|-------|--------------------------------------------|
| `None`      | `0`   | No Kafka WAL sync (default)                |
| `PerAppend` | `1`   | Publish to Kafka on every `Append()`       |
| `PerSync`   | `2`   | Publish to Kafka on every `Sync()`/`fsync` |

#### Kafka WAL recovery on startup

When `kafka_wal_sync_mode` is set to `PerAppend` or `PerSync`, WAL records
are published to Kafka during normal operation. On startup (both read-write
and read-only opens), the cloud file system automatically consumes all
available records from the Kafka topic and writes them into the local DB
directory before RocksDB's recovery runs. This ensures that unflushed data
that was published to Kafka but not yet compacted into SST files is not
lost after a restart.

The Kafka topic name is derived as `<kafka_topic_prefix>.<dest_bucket_name>`.
The consumer reads from the earliest available offset on every startup.
RocksDB's `Recover()` deduplicates WAL entries by sequence number, so
replaying records that are already covered by flushed SSTs is safe.

**Important:** The Kafka topic retention must be configured to be long
enough to cover the interval between the last flush/compaction and a
potential crash. If Kafka purges messages before the data reaches SSTs,
those writes will be lost.

When `background_wal_sync_to_cloud` is also enabled, both S3 and Kafka
recovery run during startup. S3 recovery downloads complete WAL files
first, then Kafka recovery fills in any records that the periodic S3
upload had not yet captured.

#### Fallback buckets

| Method                  | Description                                                  |
|-------------------------|--------------------------------------------------------------|
| `add_fallback_bucket`   | Add a fallback bucket (searched in order when file not found)|
| `num_fallback_buckets`  | Number of configured fallback buckets                        |
| `clear_fallback_buckets`| Remove all fallback buckets                                  |

#### Replication buckets

| Method                      | Description                                                              |
|-----------------------------|--------------------------------------------------------------------------|
| `add_replication_bucket`    | Add a bucket for async cross-region SST/MANIFEST/CLOUDMANIFEST replication |
| `num_replication_buckets`   | Number of configured replication buckets                                 |
| `clear_replication_buckets` | Remove all replication buckets                                           |

#### Bandwidth throttling

| Method                              | Description                                   |
|-------------------------------------|-----------------------------------------------|
| `set_cloud_upload_rate_limiter`     | Throttle upload bandwidth (bytes/sec, refill period, fairness) |
| `set_cloud_download_rate_limiter`   | Throttle download bandwidth (bytes/sec, refill period, fairness) |

### CloudBucketOptions

| Method / Getter              | Type           | Description                      |
|------------------------------|----------------|----------------------------------|
| `set_bucket_name` / `get_bucket_name` | `String` | Cloud storage bucket name       |
| `set_region` / `get_region`  | `String`       | Cloud provider region            |
| `set_prefix` / `get_prefix`  | `String`       | Key prefix within the bucket     |
| `set_object_path` / `get_object_path` | `String` | Logical object path            |
| `read_from_env`              | `&str` (prefix)| Populate from environment variables |
| `is_valid`                   | `bool`         | Whether the configuration is valid |

### BackupEngineOptions

| Method / Getter                                        | Type   | Default | Description                                          |
|--------------------------------------------------------|--------|---------|------------------------------------------------------|
| `new(backup_dir)`                                      | `Path` | —       | Create options targeting the given backup directory   |
| `set_max_background_operations`                        | `i32`  | `1`     | Parallel file copy / checksum operations             |
| `set_sync` / `get_sync`                                | `bool` | `true`  | fsync after every file write for crash consistency   |

> **Note:** The underlying C++ `BackupEngineOptions` has additional fields
> (`share_table_files`, `destroy_old_data`, `backup_log_files`,
> `backup_rate_limit`, `restore_rate_limit`, etc.) that are available in the
> C API but not yet wrapped in the Rust `BackupEngineOptions` struct.
> `share_table_files` defaults to `true` in the C++ engine, so incremental
> deduplication works out of the box.

### BackupEngine

| Method                                                   | Description                                              |
|----------------------------------------------------------|----------------------------------------------------------|
| `open(opts, env)`                                        | Open or create a backup engine for the given `Env`       |
| `create_new_backup(db)`                                  | Capture a backup (no flush)                              |
| `create_new_backup_flush(db, flush)`                     | Capture a backup, optionally flushing the memtable       |
| `purge_old_backups(n)`                                   | Retain only the `n` most recent backups                  |
| `verify_backup(id)`                                      | Check file existence and sizes for a backup              |
| `get_backup_info()`                                      | List all backups with ID, timestamp, size, and file count|
| `restore_from_latest_backup(db_dir, wal_dir, opts)`      | Restore the most recent backup                           |
| `restore_from_backup(db_dir, wal_dir, opts, id)`         | Restore a specific backup by ID                          |

### RestoreOptions

| Method                   | Default | Description                                            |
|--------------------------|---------|--------------------------------------------------------|
| `set_keep_log_files`     | `false` | If true, don't overwrite existing WAL files on restore |

### CloudCheckpointOptions

| Method / Getter                              | Type   | Description                         |
|----------------------------------------------|--------|-------------------------------------|
| `set_thread_count` / `get_thread_count`      | `i32`  | Number of parallel upload threads   |
| `set_flush_memtable` / `get_flush_memtable`  | `bool` | Flush memtable before checkpointing |

### CloudCredentials

| Method                  | Description                                          |
|-------------------------|------------------------------------------------------|
| `initialize_simple`     | Set explicit access key ID and secret key            |
| `initialize_config`     | Read credentials from a config file                  |
| `set_type` / `get_type` | Set/get the `AwsAccessType`                          |
| `has_valid`             | Check whether credentials are valid                  |

### SstFileManager

| Method / Getter                                     | Type   | Description                                  |
|------------------------------------------------------|--------|----------------------------------------------|
| `new`                                                | `&Env` | Create a new manager                         |
| `set_max_allowed_space_usage`                        | `u64`  | Maximum total SST size in bytes              |
| `set_compaction_buffer_size`                          | `u64`  | Space reserved for compaction output         |
| `is_max_allowed_space_reached`                       | `bool` | Whether the space limit has been reached     |
| `is_max_allowed_space_reached_including_compactions` | `bool` | Including pending compaction output          |
| `get_total_size`                                     | `u64`  | Current total SST file size                  |
| `set_delete_rate_bytes_per_second` / `get_delete_rate_bytes_per_second` | `i64` | Deletion rate limit   |
| `set_max_trash_db_ratio` / `get_max_trash_db_ratio`  | `f64`  | Max ratio of trash to total size            |
| `get_total_trash_size`                               | `u64`  | Current trash file size                      |
