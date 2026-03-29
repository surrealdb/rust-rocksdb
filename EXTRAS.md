# SurrealDB Extras for `rust-rocksdb`

This document describes the changes in the `0.24.0-surreal.3` branch compared
to upstream `main`. These additions provide Rust bindings for the SurrealDB
cloud extensions to RocksDB.

**Underlying RocksDB:** `surrealdb/rocksdb` branch `cloud/11.0.0`

---

## Submodule update to cloud RocksDB

Points the `librocksdb-sys/rocksdb` submodule at the `surrealdb/rocksdb`
fork's `cloud/11.0.0` branch and updates the lib source list. Also adjusts
statistics enums for new tickers and histograms introduced in RocksDB 11.

## SstFileManager bindings

Rust bindings for `SstFileManager`, which tracks SST file sizes and controls
deletion rate limiting. Includes `Options::set_sst_file_manager` to attach
the manager to a DB instance. Full test coverage.

## `DB::resume` binding

Adds `resume()` to `DBCommon` so all DB variants (`DB`, `TransactionDB`,
`OptimisticTransactionDB`) can resume operations after a background error
has been resolved.

## Cloud support: build system, configuration types, and CloudDB

The core cloud integration, gated behind `cloud` and `aws` Cargo features
(`aws` implies `cloud` and links the AWS SDK).

Rust types and bindings for:
- **`CloudCredentials`** — AWS access key, config file path, and access type
  (simple, config, instance-profile, environment, anonymous)
- **`CloudBucketOptions`** — bucket name, region, prefix, and object path
- **`CloudFileSystemOptions`** — full set of boolean, numeric, and string
  options controlling cloud behavior (keep local files, purge on close,
  SST read-ahead, ephemeral mode, etc.)
- **`CloudFileSystem`** — cloud file system creation and `Env` wrapping
- **`CloudCheckpointOptions`** — thread count and flush-memtable toggle
- **`CloudDB`** — open, open read-only, column family variants, savepoint,
  cloud checkpoint, and list column families

## CloudOptimisticTransactionDB bindings

Rust wrapper for `CloudOptimisticTransactionDB` — optimistic transactions
over a cloud-backed database. Supports the full optimistic transaction
lifecycle plus cloud operations (checkpoint, savepoint, close with flush).

## CloudTransactionDB bindings

Rust wrapper for `CloudTransactionDB` — pessimistic transactions over a
cloud-backed database. Supports the full transaction lifecycle (begin,
commit, rollback, set save point) plus cloud operations and all
`TransactionDB`-specific configuration.

## Flush options leak fix

Fixed a memory leak in the cloud DB close paths where `FlushOptions` created
via `rocksdb_flushoptions_create()` was never freed. Now uses
`FlushOptions::default()` which has a proper `Drop` implementation.

## Deprecated API removal

Removed Rust bindings for APIs deprecated in RocksDB 11.0.0 to keep the
binding surface clean and avoid compilation warnings.

## GCS (Google Cloud Storage) support

Added `gcs` Cargo feature that enables the Google Cloud Storage backend.
Build system integration links `google-cloud-cpp` libraries when the
feature is active.

## Encryption key management bindings

Rust bindings for the encryption key management module, gated behind an
`encryption` Cargo feature:
- **`KeyManager`** — create, rotate, delete, and retrieve encryption keys
- **`InMemoryKeyManager`** — in-memory implementation for testing
- **`EncryptedEnv`** — environment wrapper for transparent data-at-rest
  encryption

Build system links OpenSSL when the feature is active.

## User-defined timestamps in optimistic transactions

Adds `put_with_ts`, `merge_with_ts`, and `delete_with_ts` methods to the
`Transaction` type, enabling timestamp-aware reads and writes within
optimistic transactions.

## Clippy warning fixes

Fixes various Clippy warnings (needless borrows, redundant closures,
unnecessary casts) across the crate to keep CI clean.

## Cloud WAL sync option bindings

Bindings for WAL cloud sync configuration on `CloudFileSystemOptions`:
- `WalKafkaSyncMode` enum
- Getters/setters for `keep_local_log_files`, `kafka_wal_sync_mode`,
  `kafka_bootstrap_servers`, `kafka_topic_prefix`,
  `background_wal_sync_to_cloud`, and `background_wal_sync_interval_ms`

## Incremental WAL delta upload option

`set_use_wal_delta_upload` / `get_use_wal_delta_upload` on
`CloudFileSystemOptions`. When enabled alongside
`background_wal_sync_to_cloud`, only new bytes since the last upload are
written as separate delta objects instead of re-uploading the entire WAL
file. Recovery reassembles deltas in order.

## Fork point snapshot API bindings

`ForkPoint` struct and `capture_fork_point()` method on `CloudDB`,
`CloudTransactionDB`, and `CloudOptimisticTransactionDB`. Returns a
lightweight metadata-only snapshot (epoch, next file number, cookie) used
for zero-copy database branching.

## Cloud bandwidth throttling option bindings

Getters/setters for `cloud_upload_rate_limiter` and
`cloud_download_rate_limiter` on `CloudFileSystemOptions`, enabling
per-instance throttling of S3/GCS upload and download bandwidth.

## Serverless cold start optimization bindings

Rust bindings for three new cold start optimization options:
- `set_skip_cloud_listing_on_open` / `get_skip_cloud_listing_on_open` on
  `CloudFileSystemOptions` — skip cloud object listing during DB open
- `set_warm_connection_pool_size` / `get_warm_connection_pool_size` on
  `CloudFileSystemOptions` — pre-warm TLS connections at init time
- `set_initial_table_load_limit` / `get_initial_table_load_limit` on
  `Options` — control how many SST files are opened during DB::Open
