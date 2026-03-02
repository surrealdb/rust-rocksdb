use std::{collections::BTreeMap, ffi::CString, fs, iter, marker::PhantomData, path::Path, ptr};

use libc::{c_char, c_int, size_t};

use crate::{
    cloud::CloudFileSystem,
    column_family::ColumnFamilyTtl,
    db::{DBCommon, DBInner},
    ffi,
    ffi_util::to_cpath,
    write_batch::WriteBatchWithTransaction,
    AsColumnFamilyRef, ColumnFamilyDescriptor, Error, OptimisticTransactionOptions, Options,
    ThreadMode, Transaction, WriteOptions, DEFAULT_COLUMN_FAMILY_NAME,
};

/// A type alias to RocksDB Cloud Optimistic Transaction DB.
///
/// See [`DBCommon`] for the full list of methods.
#[cfg(not(feature = "multi-threaded-cf"))]
pub type CloudOptimisticTransactionDB<T = crate::SingleThreaded> =
    DBCommon<T, CloudOptimisticTransactionDBInner>;
#[cfg(feature = "multi-threaded-cf")]
pub type CloudOptimisticTransactionDB<T = crate::MultiThreaded> =
    DBCommon<T, CloudOptimisticTransactionDBInner>;

pub struct CloudOptimisticTransactionDBInner {
    base: *mut ffi::rocksdb_t,
    db: *mut ffi::rocksdb_cloud_otxn_db_t,
    _cloud_fs: CloudFileSystem,
}

impl DBInner for CloudOptimisticTransactionDBInner {
    fn inner(&self) -> *mut ffi::rocksdb_t {
        self.base
    }
}

impl Drop for CloudOptimisticTransactionDBInner {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_optimistictransactiondb_close_base_db(self.base);
            ffi::rocksdb_cloud_otxn_db_close(self.db);
        }
    }
}

impl CloudOptimisticTransactionDBInner {
    fn flush(&self) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_flush(
                self.base,
                ffi::rocksdb_flushoptions_create()
            ));
        }
        Ok(())
    }
}

impl<T: ThreadMode> CloudOptimisticTransactionDB<T> {
    /// Opens a cloud optimistic transaction database.
    pub fn open<P: AsRef<Path>>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
    ) -> Result<Self, Error> {
        Self::open_cf(opts, cloud_fs, path, None::<&str>)
    }

    /// Opens with column family names.
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

    /// Opens with column family descriptors.
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

        let cache_path = cloud_fs.persistent_cache_path_c();
        let cache_size = cloud_fs.persistent_cache_size_gb();

        let db: *mut ffi::rocksdb_cloud_otxn_db_t;
        let mut cf_map = BTreeMap::new();

        if cfs.is_empty() {
            db = unsafe {
                ffi_try!(ffi::rocksdb_cloud_otxn_db_open(
                    opts.inner,
                    cpath.as_ptr(),
                    cache_path.as_ptr(),
                    cache_size,
                ))
            };
        } else {
            let mut cfs_v = cfs;
            if !cfs_v.iter().any(|cf| cf.name == DEFAULT_COLUMN_FAMILY_NAME) {
                cfs_v.push(ColumnFamilyDescriptor {
                    name: String::from(DEFAULT_COLUMN_FAMILY_NAME),
                    options: Options::default(),
                    ttl: ColumnFamilyTtl::SameAsDb,
                });
            }

            let c_cfs: Vec<CString> = cfs_v
                .iter()
                .map(|cf| CString::new(cf.name.as_bytes()).unwrap())
                .collect();
            let cfnames: Vec<_> = c_cfs.iter().map(|cf| cf.as_ptr()).collect();
            let mut cfhandles: Vec<_> = cfs_v.iter().map(|_| ptr::null_mut()).collect();
            let cfopts: Vec<_> = cfs_v
                .iter()
                .map(|cf| cf.options.inner.cast_const())
                .collect();

            db = unsafe {
                ffi_try!(ffi::rocksdb_cloud_otxn_db_open_column_families(
                    opts.inner,
                    cpath.as_ptr(),
                    cache_path.as_ptr(),
                    cache_size,
                    cfs_v.len() as c_int,
                    cfnames.as_ptr(),
                    cfopts.as_ptr(),
                    cfhandles.as_mut_ptr(),
                ))
            };

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

        let otxn_db = unsafe { ffi::rocksdb_cloud_otxn_db_get_txn_db(db) };
        if otxn_db.is_null() {
            unsafe { ffi::rocksdb_cloud_otxn_db_close(db) };
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let base = unsafe { ffi::rocksdb_optimistictransactiondb_get_base_db(otxn_db) };
        if base.is_null() {
            unsafe { ffi::rocksdb_cloud_otxn_db_close(db) };
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

    /// Flushes all memtables to ensure data is uploaded to cloud, then closes the DB.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.flush()
    }

    /// Creates a transaction with default options.
    pub fn transaction(&self) -> Transaction<Self> {
        self.transaction_opt(
            &WriteOptions::default(),
            &OptimisticTransactionOptions::default(),
        )
    }

    /// Creates a transaction with the given options.
    pub fn transaction_opt(
        &self,
        writeopts: &WriteOptions,
        otxn_opts: &OptimisticTransactionOptions,
    ) -> Transaction<Self> {
        let otxn_db = unsafe { ffi::rocksdb_cloud_otxn_db_get_txn_db(self.inner.db) };
        Transaction {
            inner: unsafe {
                ffi::rocksdb_optimistictransaction_begin(
                    otxn_db,
                    writeopts.inner,
                    otxn_opts.inner,
                    ptr::null_mut(),
                )
            },
            _marker: PhantomData,
        }
    }

    pub fn write_opt(
        &self,
        batch: WriteBatchWithTransaction<true>,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        let otxn_db = unsafe { ffi::rocksdb_cloud_otxn_db_get_txn_db(self.inner.db) };
        unsafe {
            ffi_try!(ffi::rocksdb_optimistictransactiondb_write(
                otxn_db,
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
