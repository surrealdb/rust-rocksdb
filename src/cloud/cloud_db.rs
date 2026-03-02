use std::{collections::BTreeMap, ffi::CString, fs, iter, path::Path};

use libc::{c_char, c_int, size_t};

use crate::{
    cloud::{CloudBucketOptions, CloudCheckpointOptions, CloudFileSystem},
    column_family::ColumnFamilyTtl,
    db::{DBCommon, DBInner},
    ffi,
    ffi_util::to_cpath,
    ColumnFamilyDescriptor, Error, FlushOptions, Options, ThreadMode, DEFAULT_COLUMN_FAMILY_NAME,
};

/// A type alias to RocksDB Cloud DB.
///
/// See [`DBCommon`] for the full list of methods.
#[cfg(not(feature = "multi-threaded-cf"))]
pub type CloudDB<T = crate::SingleThreaded> = DBCommon<T, CloudDBInner>;
#[cfg(feature = "multi-threaded-cf")]
pub type CloudDB<T = crate::MultiThreaded> = DBCommon<T, CloudDBInner>;

pub struct CloudDBInner {
    base: *mut ffi::rocksdb_t,
    db: *mut ffi::rocksdb_cloud_db_t,
    _cloud_fs: CloudFileSystem,
}

impl DBInner for CloudDBInner {
    fn inner(&self) -> *mut ffi::rocksdb_t {
        self.base
    }
}

impl Drop for CloudDBInner {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_cloud_db_close(self.db);
        }
    }
}

impl CloudDBInner {
    fn flush(&self) -> Result<(), Error> {
        let opts = FlushOptions::default();
        unsafe {
            ffi_try!(ffi::rocksdb_flush(self.base, opts.inner));
        }
        Ok(())
    }
}

impl<T: ThreadMode> CloudDB<T> {
    /// Opens a cloud database.
    pub fn open<P: AsRef<Path>>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
    ) -> Result<Self, Error> {
        Self::open_cf(opts, cloud_fs, path, None::<&str>)
    }

    /// Opens a read-only cloud database.
    pub fn open_read_only<P: AsRef<Path>>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
    ) -> Result<Self, Error> {
        Self::open_cf_internal(opts, cloud_fs, path, Vec::new(), true)
    }

    /// Opens a cloud database with column families.
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
        Self::open_cf_internal(opts, cloud_fs, path, cfs.collect(), false)
    }

    /// Opens a cloud database with column family descriptors.
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
        Self::open_cf_internal(opts, cloud_fs, path, cfs.into_iter().collect(), false)
    }

    fn open_cf_internal<P: AsRef<Path>>(
        opts: &Options,
        cloud_fs: &CloudFileSystem,
        path: P,
        cfs: Vec<ColumnFamilyDescriptor>,
        read_only: bool,
    ) -> Result<Self, Error> {
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

        let db: *mut ffi::rocksdb_cloud_db_t;
        let mut cf_map = BTreeMap::new();

        if cfs.is_empty() {
            db = if read_only {
                unsafe {
                    ffi_try!(ffi::rocksdb_cloud_db_open_read_only(
                        opts.inner,
                        cpath.as_ptr(),
                        cache_path.as_ptr(),
                        cache_size,
                    ))
                }
            } else {
                unsafe {
                    ffi_try!(ffi::rocksdb_cloud_db_open(
                        opts.inner,
                        cpath.as_ptr(),
                        cache_path.as_ptr(),
                        cache_size,
                    ))
                }
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
            let mut cfhandles: Vec<_> = cfs_v.iter().map(|_| std::ptr::null_mut()).collect();
            let cfopts: Vec<_> = cfs_v
                .iter()
                .map(|cf| cf.options.inner.cast_const())
                .collect();

            db = if read_only {
                unsafe {
                    ffi_try!(ffi::rocksdb_cloud_db_open_column_families_read_only(
                        opts.inner,
                        cpath.as_ptr(),
                        cache_path.as_ptr(),
                        cache_size,
                        cfs_v.len() as c_int,
                        cfnames.as_ptr(),
                        cfopts.as_ptr(),
                        cfhandles.as_mut_ptr(),
                    ))
                }
            } else {
                unsafe {
                    ffi_try!(ffi::rocksdb_cloud_db_open_column_families(
                        opts.inner,
                        cpath.as_ptr(),
                        cache_path.as_ptr(),
                        cache_size,
                        cfs_v.len() as c_int,
                        cfnames.as_ptr(),
                        cfopts.as_ptr(),
                        cfhandles.as_mut_ptr(),
                    ))
                }
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

        let base = unsafe { ffi::rocksdb_cloud_db_get_base_db(db) };
        if base.is_null() {
            unsafe {
                ffi::rocksdb_cloud_db_close(db);
            }
            return Err(Error::new("Could not initialize database.".to_owned()));
        }

        let inner = CloudDBInner {
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

    /// Create a savepoint (copy local files to cloud).
    pub fn savepoint(&self) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_cloud_db_savepoint(self.inner.db));
        }
        Ok(())
    }

    /// Checkpoint the database to another cloud bucket.
    pub fn checkpoint_to_cloud(
        &self,
        destination: &CloudBucketOptions,
        options: &CloudCheckpointOptions,
    ) -> Result<(), Error> {
        unsafe {
            ffi_try!(ffi::rocksdb_cloud_db_checkpoint_to_cloud(
                self.inner.db,
                destination.inner,
                options.inner,
            ));
        }
        Ok(())
    }

    /// List column families for a cloud database at the given path.
    pub fn list_column_families(
        opts: &Options,
        name: impl AsRef<Path>,
    ) -> Result<Vec<String>, Error> {
        let cname = to_cpath(name)?;
        let mut lencf: size_t = 0;
        unsafe {
            let cfs_raw = ffi_try!(ffi::rocksdb_cloud_db_list_column_families(
                opts.inner,
                cname.as_ptr(),
                &mut lencf,
            ));
            let result = (0..lencf)
                .map(|i| {
                    let s = std::ffi::CStr::from_ptr(*cfs_raw.add(i))
                        .to_string_lossy()
                        .into_owned();
                    libc::free(*cfs_raw.add(i) as *mut libc::c_void);
                    s
                })
                .collect();
            libc::free(cfs_raw as *mut libc::c_void);
            Ok(result)
        }
    }
}
