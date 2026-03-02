use std::{
    collections::BTreeMap,
    ffi::CString,
    fs, iter,
    marker::PhantomData,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, Mutex},
};

use libc::{c_char, c_int, c_void, size_t};

use crate::{
    cloud::CloudFileSystem,
    column_family::{ColumnFamilyTtl, UnboundColumnFamily},
    db::{convert_values, DBAccess},
    db_options::OptionsMustOutliveDB,
    ffi,
    ffi_util::to_cpath,
    AsColumnFamilyRef, BoundColumnFamily, ColumnFamily, ColumnFamilyDescriptor,
    DBIteratorWithThreadMode, DBPinnableSlice, DBRawIteratorWithThreadMode, Direction, Error,
    IteratorMode, MultiThreaded, Options, ReadOptions, SingleThreaded, SnapshotWithThreadMode,
    ThreadMode, Transaction, TransactionDBOptions, TransactionOptions, WriteBatchWithTransaction,
    WriteOptions, DEFAULT_COLUMN_FAMILY_NAME,
};
use ffi::rocksdb_transaction_t;

#[cfg(not(feature = "multi-threaded-cf"))]
type DefaultThreadMode = crate::SingleThreaded;
#[cfg(feature = "multi-threaded-cf")]
type DefaultThreadMode = crate::MultiThreaded;

/// RocksDB Cloud TransactionDB.
///
/// Provides pessimistic transactions over a cloud-backed database.
/// Follows the same API as [`TransactionDB`](crate::TransactionDB) with
/// cloud-specific open methods.
pub struct CloudTransactionDB<T: ThreadMode = DefaultThreadMode> {
    cloud_db: *mut ffi::rocksdb_cloud_txn_db_t,
    pub(crate) txn_db: *mut ffi::rocksdb_transactiondb_t,
    cfs: T,
    path: PathBuf,
    prepared: Mutex<Vec<*mut rocksdb_transaction_t>>,
    _cloud_fs: CloudFileSystem,
    _outlive: Vec<OptionsMustOutliveDB>,
}

unsafe impl<T: ThreadMode> Send for CloudTransactionDB<T> {}
unsafe impl<T: ThreadMode> Sync for CloudTransactionDB<T> {}

impl<T: ThreadMode> DBAccess for CloudTransactionDB<T> {
    unsafe fn create_snapshot(&self) -> *const ffi::rocksdb_snapshot_t {
        ffi::rocksdb_transactiondb_create_snapshot(self.txn_db)
    }

    unsafe fn release_snapshot(&self, snapshot: *const ffi::rocksdb_snapshot_t) {
        ffi::rocksdb_transactiondb_release_snapshot(self.txn_db, snapshot);
    }

    unsafe fn create_iterator(&self, readopts: &ReadOptions) -> *mut ffi::rocksdb_iterator_t {
        ffi::rocksdb_transactiondb_create_iterator(self.txn_db, readopts.inner)
    }

    unsafe fn create_iterator_cf(
        &self,
        cf_handle: *mut ffi::rocksdb_column_family_handle_t,
        readopts: &ReadOptions,
    ) -> *mut ffi::rocksdb_iterator_t {
        ffi::rocksdb_transactiondb_create_iterator_cf(self.txn_db, readopts.inner, cf_handle)
    }

    fn get_opt<K: AsRef<[u8]>>(
        &self,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.get_opt(key, readopts)
    }

    fn get_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.get_cf_opt(cf, key, readopts)
    }

    fn get_pinned_opt<K: AsRef<[u8]>>(
        &self,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        self.get_pinned_opt(key, readopts)
    }

    fn get_pinned_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        self.get_pinned_cf_opt(cf, key, readopts)
    }

    fn multi_get_opt<K, I>(
        &self,
        keys: I,
        readopts: &ReadOptions,
    ) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = K>,
    {
        self.multi_get_opt(keys, readopts)
    }

    fn multi_get_cf_opt<'b, K, I, W>(
        &self,
        keys_cf: I,
        readopts: &ReadOptions,
    ) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: AsColumnFamilyRef + 'b,
    {
        self.multi_get_cf_opt(keys_cf, readopts)
    }
}

impl<T: ThreadMode> CloudTransactionDB<T> {
    /// Opens a cloud transaction database.
    pub fn open<P: AsRef<Path>>(
        opts: &Options,
        txn_db_opts: &TransactionDBOptions,
        cloud_fs: &CloudFileSystem,
        path: P,
    ) -> Result<Self, Error> {
        Self::open_cf(opts, txn_db_opts, cloud_fs, path, None::<&str>)
    }

    /// Opens with column family names.
    pub fn open_cf<P, I, N>(
        opts: &Options,
        txn_db_opts: &TransactionDBOptions,
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
        Self::open_cf_descriptors_internal(opts, txn_db_opts, cloud_fs, path, cfs)
    }

    /// Opens with column family descriptors.
    pub fn open_cf_descriptors<P, I>(
        opts: &Options,
        txn_db_opts: &TransactionDBOptions,
        cloud_fs: &CloudFileSystem,
        path: P,
        cfs: I,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = ColumnFamilyDescriptor>,
    {
        Self::open_cf_descriptors_internal(opts, txn_db_opts, cloud_fs, path, cfs)
    }

    fn open_cf_descriptors_internal<P, I>(
        opts: &Options,
        txn_db_opts: &TransactionDBOptions,
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

        let cloud_db: *mut ffi::rocksdb_cloud_txn_db_t;
        let mut cf_map = BTreeMap::new();

        if cfs.is_empty() {
            cloud_db = unsafe {
                ffi_try!(ffi::rocksdb_cloud_txn_db_open(
                    opts.inner,
                    txn_db_opts.inner,
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

            cloud_db = unsafe {
                ffi_try!(ffi::rocksdb_cloud_txn_db_open_column_families(
                    opts.inner,
                    txn_db_opts.inner,
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

        if cloud_db.is_null() {
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let txn_db = unsafe { ffi::rocksdb_cloud_txn_db_get_txn_db(cloud_db) };
        if txn_db.is_null() {
            unsafe { ffi::rocksdb_cloud_txn_db_close(cloud_db) };
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let prepared = unsafe {
            let mut cnt = 0;
            let ptr = ffi::rocksdb_transactiondb_get_prepared_transactions(txn_db, &mut cnt);
            let mut vec = vec![std::ptr::null_mut(); cnt];
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(ptr, vec.as_mut_ptr(), cnt);
                ffi::rocksdb_free(ptr as *mut c_void);
            }
            vec
        };

        Ok(CloudTransactionDB {
            cloud_db,
            txn_db,
            cfs: T::new_cf_map_internal(cf_map),
            path: path.as_ref().to_path_buf(),
            prepared: Mutex::new(prepared),
            _cloud_fs: cloud_fs.clone(),
            _outlive: outlive,
        })
    }

    /// Flushes all memtables to ensure data is uploaded to cloud.
    pub fn flush(&self) -> Result<(), Error> {
        let opts = crate::FlushOptions::default();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_flush(self.txn_db, opts.inner,));
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Creates a transaction with default options.
    pub fn transaction(&self) -> Transaction<Self> {
        self.transaction_opt(&WriteOptions::default(), &TransactionOptions::default())
    }

    /// Creates a transaction with options.
    pub fn transaction_opt<'a>(
        &'a self,
        write_opts: &WriteOptions,
        txn_opts: &TransactionOptions,
    ) -> Transaction<'a, Self> {
        Transaction {
            inner: unsafe {
                ffi::rocksdb_transaction_begin(
                    self.txn_db,
                    write_opts.inner,
                    txn_opts.inner,
                    std::ptr::null_mut(),
                )
            },
            _marker: PhantomData,
        }
    }

    /// Get all prepared transactions for recovery.
    pub fn prepared_transactions(&self) -> Vec<Transaction<Self>> {
        self.prepared
            .lock()
            .unwrap()
            .drain(0..)
            .map(|inner| Transaction {
                inner,
                _marker: PhantomData,
            })
            .collect()
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Error> {
        self.get_pinned(key).map(|x| x.map(|v| v.as_ref().to_vec()))
    }

    pub fn get_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.get_pinned_cf(cf, key)
            .map(|x| x.map(|v| v.as_ref().to_vec()))
    }

    pub fn get_opt<K: AsRef<[u8]>>(
        &self,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.get_pinned_opt(key, readopts)
            .map(|x| x.map(|v| v.as_ref().to_vec()))
    }

    pub fn get_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.get_pinned_cf_opt(cf, key, readopts)
            .map(|x| x.map(|v| v.as_ref().to_vec()))
    }

    pub fn get_pinned<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<DBPinnableSlice>, Error> {
        self.get_pinned_opt(key, &ReadOptions::default())
    }

    pub fn get_pinned_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        self.get_pinned_cf_opt(cf, key, &ReadOptions::default())
    }

    pub fn get_pinned_opt<K: AsRef<[u8]>>(
        &self,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        let key = key.as_ref();
        unsafe {
            let val = ffi_try!(ffi::rocksdb_transactiondb_get_pinned(
                self.txn_db,
                readopts.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            ));
            if val.is_null() {
                Ok(None)
            } else {
                Ok(Some(DBPinnableSlice::from_c(val)))
            }
        }
    }

    pub fn get_pinned_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        readopts: &ReadOptions,
    ) -> Result<Option<DBPinnableSlice>, Error> {
        let key = key.as_ref();
        unsafe {
            let val = ffi_try!(ffi::rocksdb_transactiondb_get_pinned_cf(
                self.txn_db,
                readopts.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            ));
            if val.is_null() {
                Ok(None)
            } else {
                Ok(Some(DBPinnableSlice::from_c(val)))
            }
        }
    }

    pub fn multi_get<K, I>(&self, keys: I) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = K>,
    {
        self.multi_get_opt(keys, &ReadOptions::default())
    }

    pub fn multi_get_opt<K, I>(
        &self,
        keys: I,
        readopts: &ReadOptions,
    ) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = K>,
    {
        let (keys, keys_sizes): (Vec<Box<[u8]>>, Vec<_>) = keys
            .into_iter()
            .map(|key| {
                let key = key.as_ref();
                (Box::from(key), key.len())
            })
            .unzip();
        let ptr_keys: Vec<_> = keys.iter().map(|k| k.as_ptr() as *const c_char).collect();

        let mut values = vec![ptr::null_mut(); keys.len()];
        let mut values_sizes = vec![0_usize; keys.len()];
        let mut errors = vec![ptr::null_mut(); keys.len()];
        unsafe {
            ffi::rocksdb_transactiondb_multi_get(
                self.txn_db,
                readopts.inner,
                ptr_keys.len(),
                ptr_keys.as_ptr(),
                keys_sizes.as_ptr(),
                values.as_mut_ptr(),
                values_sizes.as_mut_ptr(),
                errors.as_mut_ptr(),
            );
        }

        convert_values(values, values_sizes, errors)
    }

    pub fn multi_get_cf<'a, 'b: 'a, K, I, W>(
        &'a self,
        keys: I,
    ) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef,
    {
        self.multi_get_cf_opt(keys, &ReadOptions::default())
    }

    pub fn multi_get_cf_opt<'a, 'b: 'a, K, I, W>(
        &'a self,
        keys: I,
        readopts: &ReadOptions,
    ) -> Vec<Result<Option<Vec<u8>>, Error>>
    where
        K: AsRef<[u8]>,
        I: IntoIterator<Item = (&'b W, K)>,
        W: 'b + AsColumnFamilyRef,
    {
        let (cfs_and_keys, keys_sizes): (Vec<(_, Box<[u8]>)>, Vec<_>) = keys
            .into_iter()
            .map(|(cf, key)| {
                let key = key.as_ref();
                ((cf, Box::from(key)), key.len())
            })
            .unzip();
        let ptr_keys: Vec<_> = cfs_and_keys
            .iter()
            .map(|(_, k)| k.as_ptr() as *const c_char)
            .collect();
        let ptr_cfs: Vec<_> = cfs_and_keys
            .iter()
            .map(|(c, _)| c.inner().cast_const())
            .collect();

        let mut values = vec![ptr::null_mut(); ptr_keys.len()];
        let mut values_sizes = vec![0_usize; ptr_keys.len()];
        let mut errors = vec![ptr::null_mut(); ptr_keys.len()];
        unsafe {
            ffi::rocksdb_transactiondb_multi_get_cf(
                self.txn_db,
                readopts.inner,
                ptr_cfs.as_ptr(),
                ptr_keys.len(),
                ptr_keys.as_ptr(),
                keys_sizes.as_ptr(),
                values.as_mut_ptr(),
                values_sizes.as_mut_ptr(),
                errors.as_mut_ptr(),
            );
        }

        convert_values(values, values_sizes, errors)
    }

    pub fn put<K, V>(&self, key: K, value: V) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.put_opt(key, value, &WriteOptions::default())
    }

    pub fn put_cf<K, V>(&self, cf: &impl AsColumnFamilyRef, key: K, value: V) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.put_cf_opt(cf, key, value, &WriteOptions::default())
    }

    pub fn put_opt<K, V>(&self, key: K, value: V, writeopts: &WriteOptions) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_put(
                self.txn_db,
                writeopts.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t
            ));
        }
        Ok(())
    }

    pub fn put_cf_opt<K, V>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        value: V,
        writeopts: &WriteOptions,
    ) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_put_cf(
                self.txn_db,
                writeopts.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t
            ));
        }
        Ok(())
    }

    pub fn write(&self, batch: WriteBatchWithTransaction<true>) -> Result<(), Error> {
        self.write_opt(batch, &WriteOptions::default())
    }

    pub fn write_opt(
        &self,
        batch: WriteBatchWithTransaction<true>,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_write(
                self.txn_db,
                writeopts.inner,
                batch.inner
            ));
        }
        Ok(())
    }

    pub fn merge<K, V>(&self, key: K, value: V) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.merge_opt(key, value, &WriteOptions::default())
    }

    pub fn merge_cf<K, V>(&self, cf: &impl AsColumnFamilyRef, key: K, value: V) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        self.merge_cf_opt(cf, key, value, &WriteOptions::default())
    }

    pub fn merge_opt<K, V>(&self, key: K, value: V, writeopts: &WriteOptions) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_merge(
                self.txn_db,
                writeopts.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            ));
            Ok(())
        }
    }

    pub fn merge_cf_opt<K, V>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        value: V,
        writeopts: &WriteOptions,
    ) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_merge_cf(
                self.txn_db,
                writeopts.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            ));
            Ok(())
        }
    }

    pub fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Error> {
        self.delete_opt(key, &WriteOptions::default())
    }

    pub fn delete_cf<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
    ) -> Result<(), Error> {
        self.delete_cf_opt(cf, key, &WriteOptions::default())
    }

    pub fn delete_opt<K: AsRef<[u8]>>(
        &self,
        key: K,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        let key = key.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_delete(
                self.txn_db,
                writeopts.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            ));
        }
        Ok(())
    }

    pub fn delete_cf_opt<K: AsRef<[u8]>>(
        &self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        writeopts: &WriteOptions,
    ) -> Result<(), Error> {
        let key = key.as_ref();
        unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_delete_cf(
                self.txn_db,
                writeopts.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            ));
        }
        Ok(())
    }

    pub fn iterator<'a: 'b, 'b>(
        &'a self,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        let readopts = ReadOptions::default();
        self.iterator_opt(mode, readopts)
    }

    pub fn iterator_opt<'a: 'b, 'b>(
        &'a self,
        mode: IteratorMode,
        readopts: ReadOptions,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        DBIteratorWithThreadMode::new(self, readopts, mode)
    }

    pub fn iterator_cf_opt<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        DBIteratorWithThreadMode::new_cf(self, cf_handle.inner(), readopts, mode)
    }

    pub fn full_iterator<'a: 'b, 'b>(
        &'a self,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        let mut opts = ReadOptions::default();
        opts.set_total_order_seek(true);
        DBIteratorWithThreadMode::new(self, opts, mode)
    }

    pub fn prefix_iterator<'a: 'b, 'b, P: AsRef<[u8]>>(
        &'a self,
        prefix: P,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        let mut opts = ReadOptions::default();
        opts.set_prefix_same_as_start(true);
        DBIteratorWithThreadMode::new(
            self,
            opts,
            IteratorMode::From(prefix.as_ref(), Direction::Forward),
        )
    }

    pub fn iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        let opts = ReadOptions::default();
        DBIteratorWithThreadMode::new_cf(self, cf_handle.inner(), opts, mode)
    }

    pub fn full_iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        mode: IteratorMode,
    ) -> DBIteratorWithThreadMode<'b, Self> {
        let mut opts = ReadOptions::default();
        opts.set_total_order_seek(true);
        DBIteratorWithThreadMode::new_cf(self, cf_handle.inner(), opts, mode)
    }

    pub fn prefix_iterator_cf<'a, P: AsRef<[u8]>>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        prefix: P,
    ) -> DBIteratorWithThreadMode<'a, Self> {
        let mut opts = ReadOptions::default();
        opts.set_prefix_same_as_start(true);
        DBIteratorWithThreadMode::<'a, Self>::new_cf(
            self,
            cf_handle.inner(),
            opts,
            IteratorMode::From(prefix.as_ref(), Direction::Forward),
        )
    }

    pub fn raw_iterator<'a: 'b, 'b>(&'a self) -> DBRawIteratorWithThreadMode<'b, Self> {
        let opts = ReadOptions::default();
        DBRawIteratorWithThreadMode::new(self, opts)
    }

    pub fn raw_iterator_cf<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
    ) -> DBRawIteratorWithThreadMode<'b, Self> {
        let opts = ReadOptions::default();
        DBRawIteratorWithThreadMode::new_cf(self, cf_handle.inner(), opts)
    }

    pub fn raw_iterator_opt<'a: 'b, 'b>(
        &'a self,
        readopts: ReadOptions,
    ) -> DBRawIteratorWithThreadMode<'b, Self> {
        DBRawIteratorWithThreadMode::new(self, readopts)
    }

    pub fn raw_iterator_cf_opt<'a: 'b, 'b>(
        &'a self,
        cf_handle: &impl AsColumnFamilyRef,
        readopts: ReadOptions,
    ) -> DBRawIteratorWithThreadMode<'b, Self> {
        DBRawIteratorWithThreadMode::new_cf(self, cf_handle.inner(), readopts)
    }

    pub fn snapshot(&self) -> SnapshotWithThreadMode<Self> {
        SnapshotWithThreadMode::<Self>::new(self)
    }

    fn create_inner_cf_handle(
        &self,
        name: &str,
        opts: &Options,
    ) -> Result<*mut ffi::rocksdb_column_family_handle_t, Error> {
        let cf_name = CString::new(name.as_bytes()).map_err(|_| {
            Error::new("Failed to convert path to CString when creating cf".to_owned())
        })?;

        Ok(unsafe {
            ffi_try!(ffi::rocksdb_transactiondb_create_column_family(
                self.txn_db,
                opts.inner,
                cf_name.as_ptr(),
            ))
        })
    }

    fn drop_column_family<C>(
        &self,
        cf_inner: *mut ffi::rocksdb_column_family_handle_t,
        _cf: C,
    ) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_drop_column_family(
                self.txn_db as *mut ffi::rocksdb_t,
                cf_inner
            ));
        }
        Ok(())
    }
}

impl CloudTransactionDB<SingleThreaded> {
    pub fn create_cf<N: AsRef<str>>(&mut self, name: N, opts: &Options) -> Result<(), Error> {
        let inner = self.create_inner_cf_handle(name.as_ref(), opts)?;
        self.cfs
            .cfs
            .insert(name.as_ref().to_string(), ColumnFamily { inner });
        Ok(())
    }

    pub fn cf_handle(&self, name: &str) -> Option<&ColumnFamily> {
        self.cfs.cfs.get(name)
    }

    pub fn drop_cf(&mut self, name: &str) -> Result<(), Error> {
        if let Some(cf) = self.cfs.cfs.remove(name) {
            self.drop_column_family(cf.inner, cf)
        } else {
            Err(Error::new(format!("Invalid column family: {name}")))
        }
    }
}

impl CloudTransactionDB<MultiThreaded> {
    pub fn create_cf<N: AsRef<str>>(&self, name: N, opts: &Options) -> Result<(), Error> {
        let mut cfs = self.cfs.cfs.write().unwrap();
        let inner = self.create_inner_cf_handle(name.as_ref(), opts)?;
        cfs.insert(
            name.as_ref().to_string(),
            Arc::new(UnboundColumnFamily { inner }),
        );
        Ok(())
    }

    pub fn cf_handle(&self, name: &str) -> Option<Arc<BoundColumnFamily>> {
        self.cfs
            .cfs
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .map(UnboundColumnFamily::bound_column_family)
    }

    pub fn drop_cf(&self, name: &str) -> Result<(), Error> {
        if let Some(cf) = self.cfs.cfs.write().unwrap().remove(name) {
            self.drop_column_family(cf.inner, cf)
        } else {
            Err(Error::new(format!("Invalid column family: {name}")))
        }
    }
}

impl<T: ThreadMode> Drop for CloudTransactionDB<T> {
    fn drop(&mut self) {
        unsafe {
            self.prepared_transactions().clear();
            self.cfs.drop_all_cfs_internal();
            ffi::rocksdb_cloud_txn_db_close(self.cloud_db);
        }
    }
}
