// Copyright 2024 SurrealDB Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

use std::{collections::BTreeMap, ffi::CString, fs, iter, marker::PhantomData, path::Path, ptr};

use libc::{c_char, c_int, size_t};

use crate::{
    cloud::CloudFileSystem,
    db::{DBCommon, DBInner},
    ffi,
    ffi_util::to_cpath,
    write_batch::WriteBatchWithTransaction,
    AsColumnFamilyRef, CStrLike, ColumnFamilyDescriptor, Error, OptimisticTransactionOptions,
    Options, ThreadMode, Transaction, WriteOptions, DEFAULT_COLUMN_FAMILY_NAME,
};

/// A type alias to RocksDB DBCloud Optimistic Transaction. In practice is works as a regular RocksDB Optimistic Transaction instance.
/// It only adds the CloudFileSystem to the DB instance.
///
/// Please read the official
/// [guide](https://github.com/facebook/rocksdb/wiki/Transactions#optimistictransactiondb)
/// to learn more about RocksDB OptimisticTransactionDB.
///
/// The default thread mode for [`OptimisticTransactionDB`] is [`SingleThreaded`]
/// if feature `multi-threaded-cf` is not enabled.
///
/// See [`DBCommon`] for full list of methods.
///
/// # Examples
///
/// ```
/// use rocksdb::{DB, Options, CloudOptimisticTransactionDB, SingleThreaded};
/// let path = "_path_for_optimistic_transaction_db";
/// {
///     let db: CloudOptimisticTransactionDB = CloudOptimisticTransactionDB::open_default(path).unwrap();
///     db.put(b"my key", b"my value").unwrap();
///
///     // create transaction
///     let txn = db.transaction();
///     txn.put(b"key2", b"value2");
///     txn.put(b"key3", b"value3");
///     txn.commit().unwrap();
/// }
/// let _ = DB::destroy(&Options::default(), path);
/// ```
///
/// [`SingleThreaded`]: crate::SingleThreaded
#[cfg(not(feature = "multi-threaded-cf"))]
pub type CloudOptimisticTransactionDB<T = crate::SingleThreaded> =
    DBCommon<T, CloudOptimisticTransactionDBInner>;
#[cfg(feature = "multi-threaded-cf")]
pub type CloudOptimisticTransactionDB<T = crate::MultiThreaded> =
    DBCommon<T, CloudOptimisticTransactionDBInner>;

pub struct CloudOptimisticTransactionDBInner {
    base: *mut ffi::rocksdb_t,
    db: *mut ffi::rocksdb_cloud_otxn_db_t,
    // Keep a reference to the CloudFileSystem to ensure it lives as long as the DB.
    _cloud_fs: CloudFileSystem,
}

impl DBInner for CloudOptimisticTransactionDBInner {
    fn inner(&self) -> *mut ffi::rocksdb_t {
        self.base
    }
}

/// Methods of `CloudOptimisticTransactionDBInner`.
impl CloudOptimisticTransactionDBInner {
    /// Flushes all memtables to storage.
    fn flush(&self) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_flush(
                self.base,
                ffi::rocksdb_flushoptions_create()
            ));
        }
        Ok(())
    }

    /// Closes the database.
    fn close(&self) {
        unsafe {
            ffi::rocksdb_optimistictransactiondb_close_base_db(self.base);
            ffi::rocksdb_cloud_otxn_close(self.db);
        }
    }
}

/// Methods of `CloudOptimisticTransactionDB`.
impl<T: ThreadMode> CloudOptimisticTransactionDB<T> {
    /// Flushes the database so all data is uploaded to the cloud and then closes the database.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.flush()?;
        self.inner.close();
        Ok(())
    }

    /// Opens the database with the specified options.
    pub fn open<P: AsRef<Path>>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
    ) -> Result<Self, Error> {
        Self::open_cf(opts, cloud_fs, path, None::<&str>)
    }

    /// Opens a database with the given database options and column family names.
    ///
    /// Column families opened using this function will be created with default `Options`.
    /// *NOTE*: `default` column family will be opened with the `Options::default()`.
    /// If you want to open `default` column family with custom options, use `open_cf_descriptors` and
    /// provide a `ColumnFamilyDescriptor` with the desired options.
    pub fn open_cf<P, I, N>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
        cfs: I,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = N>,
        N: AsRef<str>,
    {
        let cfs = cfs
            .into_iter()
            .map(|name| ColumnFamilyDescriptor::new(name.as_ref(), Options::default()));

        Self::open_cf_descriptors_internal(opts, cloud_fs, path, cfs)
    }

    /// Opens a database with the given database options and column family descriptors.
    pub fn open_cf_descriptors<P, I>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
        cfs: I,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = ColumnFamilyDescriptor>,
    {
        Self::open_cf_descriptors_internal(opts, cloud_fs, path, cfs)
    }

    /// Internal implementation for opening RocksDB.
    fn open_cf_descriptors_internal<P, I>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
        cfs: I,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = ColumnFamilyDescriptor>,
    {
        let cfs: Vec<_> = cfs.into_iter().collect();
        let outlive = iter::once(opts.outlive.clone())
            .chain(cfs.iter().map(|cf| cf.options.outlive.clone()))
            .collect();

        let cpath = to_cpath(&path)?;
        if let Err(e) = fs::create_dir_all(&path) {
            return Err(Error::new(format!(
                "Failed to create RocksDB directory: `{e:?}`."
            )));
        }

        let db: *mut ffi::rocksdb_cloud_otxn_db_t;
        let mut cf_map = BTreeMap::new();

        if cfs.is_empty() {
            db = Self::open_raw(opts, cloud_fs, &cpath)?;
        } else {
            let mut cfs_v = cfs;
            // Always open the default column family.
            if !cfs_v.iter().any(|cf| cf.name == DEFAULT_COLUMN_FAMILY_NAME) {
                cfs_v.push(ColumnFamilyDescriptor {
                    name: String::from(DEFAULT_COLUMN_FAMILY_NAME),
                    options: Options::default(),
                });
            }
            // We need to store our CStrings in an intermediate vector
            // so that their pointers remain valid.
            let c_cfs: Vec<CString> = cfs_v
                .iter()
                .map(|cf| CString::new(cf.name.as_bytes()).unwrap())
                .collect();

            let cfnames: Vec<_> = c_cfs.iter().map(|cf| cf.as_ptr()).collect();

            // These handles will be populated by DB.
            let mut cfhandles: Vec<_> = cfs_v.iter().map(|_| ptr::null_mut()).collect();

            let cfopts: Vec<_> = cfs_v
                .iter()
                .map(|cf| cf.options.inner as *const _)
                .collect();

            db = Self::open_cf_raw(
                opts,
                cloud_fs,
                &cpath,
                &cfs_v,
                &cfnames,
                &cfopts,
                &mut cfhandles,
            )?;

            for handle in &cfhandles {
                if handle.is_null() {
                    return Err(Error::new(
                        "Received null column family handle from DB.".to_owned(),
                    ));
                }
            }

            for (cf_desc, inner) in cfs_v.iter().zip(cfhandles) {
                cf_map.insert(cf_desc.name.clone(), inner);
            }
        }

        if db.is_null() {
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let otxn_db = unsafe { ffi::rocksdb_cloud_otxn_get_txn_db(db) };
        if otxn_db.is_null() {
            unsafe {
                ffi::rocksdb_cloud_otxn_close(db);
            }
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let base = unsafe { ffi::rocksdb_optimistictransactiondb_get_base_db(otxn_db) };
        if base.is_null() {
            unsafe {
                ffi::rocksdb_cloud_otxn_close(db);
            }
            return Err(Error::new("Could not initialize database.".to_owned()));
        }
        let inner = CloudOptimisticTransactionDBInner {
            base,
            db,
            _cloud_fs: cloud_fs.clone(),
        };

        Ok(Self::new(
            inner,
            T::new_cf_map_internal(cf_map),
            path.as_ref().to_path_buf(),
            outlive,
        ))
    }

    fn open_raw(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        cpath: &CString,
    ) -> Result<*mut ffi::rocksdb_cloud_otxn_db_t, Error> {
        let persistent_cache_path = cloud_fs
            .opts()
            .persistent_cache_path
            .clone()
            .unwrap_or("".to_owned())
            .into_c_string()
            .unwrap();
        let persistent_cache_size_gb = cloud_fs
            .opts()
            .persistent_cache_size_gb
            .clone()
            .unwrap_or(0) as u64;

        unsafe {
            let db = ffi_try!(ffi::rocksdb_cloud_otxn_open(
                opts.inner,
                cpath.as_ptr(),
                persistent_cache_path.as_ptr(),
                persistent_cache_size_gb,
            ));
            Ok(db)
        }
    }

    fn open_cf_raw(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        cpath: &CString,
        cfs_v: &[ColumnFamilyDescriptor],
        cfnames: &[*const c_char],
        cfopts: &[*const ffi::rocksdb_options_t],
        cfhandles: &mut [*mut ffi::rocksdb_column_family_handle_t],
    ) -> Result<*mut ffi::rocksdb_cloud_otxn_db_t, Error> {
        let persistent_cache_path = cloud_fs
            .opts()
            .persistent_cache_path
            .clone()
            .unwrap_or("".to_owned())
            .into_c_string()
            .unwrap();
        let persistent_cache_size_gb = cloud_fs
            .opts()
            .persistent_cache_size_gb
            .clone()
            .unwrap_or(0) as u64;

        unsafe {
            let db = ffi_try!(ffi::rocksdb_cloud_otxn_open_column_families(
                opts.inner,
                cpath.as_ptr(),
                persistent_cache_path.as_ptr(),
                persistent_cache_size_gb,
                cfs_v.len() as c_int,
                cfnames.as_ptr(),
                cfopts.as_ptr(),
                cfhandles.as_mut_ptr(),
            ));
            Ok(db)
        }
    }

    /// Creates a transaction with default options.
    pub fn transaction(&self) -> Transaction<Self> {
        self.transaction_opt(
            &WriteOptions::default(),
            &OptimisticTransactionOptions::default(),
        )
    }

    /// Creates a transaction with default options.
    pub fn transaction_opt(
        &self,
        writeopts: &WriteOptions,
        otxn_opts: &OptimisticTransactionOptions,
    ) -> Transaction<Self> {
        Transaction {
            inner: unsafe {
                ffi::rocksdb_optimistictransaction_begin(
                    ffi::rocksdb_cloud_otxn_get_txn_db(self.inner.db),
                    writeopts.inner,
                    otxn_opts.inner,
                    std::ptr::null_mut(),
                )
            },
            _marker: PhantomData::default(),
        }
    }

    pub fn write_opt(
        &self,
        batch: WriteBatchWithTransaction<true>,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_optimistictransactiondb_write(
                ffi::rocksdb_cloud_otxn_get_txn_db(self.inner.db),
                writeopts.inner,
                batch.inner
            ));
        }
        Ok(())
    }

    pub fn write(&self, batch: WriteBatchWithTransaction<true>) -> Result<(), Error> {
        self.write_opt(batch, &WriteOptions::default())
    }

    pub fn write_without_wal(&self, batch: WriteBatchWithTransaction<true>) -> Result<(), Error> {
        let mut wo = WriteOptions::new();
        wo.disable_wal(true);
        self.write_opt(batch, &wo)
    }

    /// Removes the database entries in the range `["from", "to")` using given write options.
    pub fn delete_range_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        from: K,
        to: K,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        let from = from.as_ref();
        let to = to.as_ref();

        unsafe {
            ffi_try!(ffi::rocksdb_delete_range_cf(
                self.inner.inner(),
                writeopts.inner,
                cf.inner(),
                from.as_ptr() as *const c_char,
                from.len() as size_t,
                to.as_ptr() as *const c_char,
                to.len() as size_t,
            ));
            Ok(())
        }
    }

    /// Removes the database entries in the range `["from", "to")` using default write options.
    pub fn delete_range_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        from: K,
        to: K,
    ) -> Result<(), Error> {
        self.delete_range_cf_opt(cf, from, to, &WriteOptions::default())
    }
}
