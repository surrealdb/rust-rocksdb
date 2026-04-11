mod util;

use rocksdb::{CompactOptions, Options, ReadOptions, WriteBatch, WriteOptions, DB};
use std::cmp::Ordering;
use std::iter::FromIterator;
use util::{U64Comparator, U64Timestamp};

/// This function is for ensuring test of backwards compatibility
pub fn rocks_old_compare(one: &[u8], two: &[u8]) -> Ordering {
    one.cmp(two)
}

type CompareFn = dyn Fn(&[u8], &[u8]) -> Ordering;

/// create database add some values, and iterate over these
pub fn write_to_db_with_comparator(compare_fn: Box<CompareFn>) -> Vec<String> {
    let mut result_vec = Vec::new();

    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage")
        .tempdir()
        .expect("Failed to create temporary path for the _path_for_rocksdb_storage");
    let path = tempdir.path();
    {
        let mut db_opts = Options::default();

        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);
        db_opts.set_comparator("cname", compare_fn);
        let db = DB::open(&db_opts, path).unwrap();
        db.put(b"a-key", b"a-value").unwrap();
        db.put(b"b-key", b"b-value").unwrap();
        let mut iter = db.raw_iterator();
        iter.seek_to_first();
        while iter.valid() {
            let key = iter.key().unwrap();
            // maybe not best way to copy?
            let key_str = key.iter().map(|b| *b as char).collect::<Vec<_>>();
            result_vec.push(String::from_iter(key_str));
            iter.next();
        }
    }
    let _ = DB::destroy(&Options::default(), path);
    result_vec
}

#[test]
/// First verify that using a function as a comparator works as expected
/// This should verify backwards compatibility
/// Then run a test with a clojure where an x-variable is passed
/// Keep in mind that this variable must be moved to the clojure
/// Then run a test with a reverse sorting clojure and make sure the order is reverted
fn test_comparator() {
    let local_compare = move |one: &[u8], two: &[u8]| one.cmp(two);
    let x = 0;
    let local_compare_reverse = move |one: &[u8], two: &[u8]| {
        println!("Use the x value from the closure scope to do something smart: {x:?}");
        match one.cmp(two) {
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => Ordering::Equal,
            Ordering::Greater => Ordering::Less,
        }
    };

    let old_res = write_to_db_with_comparator(Box::new(rocks_old_compare));
    println!("Keys in normal sort order, no closure: {old_res:?}");
    assert_eq!(vec!["a-key", "b-key"], old_res);
    let res_closure = write_to_db_with_comparator(Box::new(local_compare));
    println!("Keys in normal sort order, closure: {res_closure:?}");
    assert_eq!(res_closure, old_res);
    let res_closure_reverse = write_to_db_with_comparator(Box::new(local_compare_reverse));
    println!("Keys in reverse sort order, closure: {res_closure_reverse:?}");
    assert_eq!(vec!["b-key", "a-key"], res_closure_reverse);
}

#[test]
fn test_comparator_with_ts() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_with_ts")
        .tempdir()
        .expect("Failed to create temporary path for the _path_for_rocksdb_storage_with_ts.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);
        db_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );
        let db = DB::open(&db_opts, path).unwrap();

        let key = b"hello";
        let val1 = b"world0";
        let val2 = b"world1";

        let ts = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);
        let ts3 = U64Timestamp::new(3);

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts);

        // basic put and get
        db.put_with_ts(key, ts, val1).unwrap();
        let value = db.get_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);

        // update
        db.put_with_ts(key, ts2, val2).unwrap();
        opts.set_timestamp(ts2);
        let value = db.get_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);

        // delete
        db.delete_with_ts(key, ts3).unwrap();
        opts.set_timestamp(ts3);
        let value = db.get_opt(key, &opts).unwrap();
        assert!(value.is_none());

        // ts2 should read deleted data
        opts.set_timestamp(ts2);
        let value = db.get_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);

        // ts1 should read old data
        opts.set_timestamp(ts);
        let value = db.get_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);

        // test iterator with ts
        opts.set_timestamp(ts2);
        let mut iter = db.raw_iterator_opt(opts);
        iter.seek_to_first();
        let mut result_vec = Vec::new();
        while iter.valid() {
            let key = iter.key().unwrap();
            // maybe not best way to copy?
            let key_str = key.iter().map(|b| *b as char).collect::<Vec<_>>();
            result_vec.push(String::from_iter(key_str));
            iter.next();
        }
        assert_eq!(result_vec, ["hello"]);

        // test full_history_ts_low works
        let mut compact_opts = CompactOptions::default();
        compact_opts.set_full_history_ts_low(ts2);
        db.compact_range_opt(None::<&[u8]>, None::<&[u8]>, &compact_opts);
        db.flush().unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts3);
        let value = db.get_opt(key, &opts).unwrap();
        assert_eq!(value, None);
        // cannot read with timestamp older than full_history_ts_low
        opts.set_timestamp(ts);
        assert!(db.get_opt(key, &opts).is_err());
    }

    let _ = DB::destroy(&Options::default(), path);
}

// Create options with a comparator and use it for multiple DBs to test lifetimes.
#[test]
fn test_comparator_lifetime() {
    fn do_not_call_comparator(_a: &[u8], _b: &[u8]) -> Ordering {
        panic!("BUG: must not be called");
    }

    let options = {
        let mut options = Options::default();
        options.set_comparator(
            "test_do_not_call_comparator",
            Box::new(do_not_call_comparator),
        );
        options.create_if_missing(true);
        options
    };

    // create a database with the comparator
    let rocksdb1_dir = tempfile::tempdir().unwrap();
    let rocksdb1 = DB::open(&options, rocksdb1_dir.path()).unwrap();

    // a second rocksdb using the same comparator is created and dropped
    {
        let rocksdb2_dir = tempfile::tempdir().unwrap();
        let rocksdb2 = DB::open(&options, rocksdb2_dir.path()).unwrap();
        rocksdb2.put(b"k", b"v").unwrap();
    }

    // rocksdb1 still works after dropping rocksdb2
    rocksdb1.put(b"k", b"v").unwrap();
    rocksdb1.flush().unwrap();
    drop(rocksdb1);
}

#[test]
fn test_comparator_with_column_family_with_ts() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_with_column_family_with_ts")
        .tempdir()
        .expect("Failed to create temporary path for the _path_for_rocksdb_storage_with_column_family_with_ts.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![("cf", cf_opts)];

        let db = DB::open_cf_with_opts(&db_opts, path, cfs).unwrap();
        let cf = db.cf_handle("cf").unwrap();

        let key = b"hello";
        let val1 = b"world0";
        let val2 = b"world1";

        let ts = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);
        let ts3 = U64Timestamp::new(3);

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts);

        // basic put and get
        db.put_cf_with_ts(&cf, key, ts, val1).unwrap();
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);

        // update
        db.put_cf_with_ts(&cf, key, ts2, val2).unwrap();
        opts.set_timestamp(ts2);
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);

        // delete
        db.delete_cf_with_ts(&cf, key, ts3).unwrap();
        opts.set_timestamp(ts3);
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert!(value.is_none());

        // ts2 should read deleted data
        opts.set_timestamp(ts2);
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);

        // ts1 should read old data
        opts.set_timestamp(ts);
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);

        // test iterator with ts
        opts.set_timestamp(ts2);
        let mut iter = db.raw_iterator_cf_opt(&cf, opts);
        iter.seek_to_first();
        let mut result_vec = Vec::new();
        while iter.valid() {
            let key = iter.key().unwrap();
            // maybe not best way to copy?
            let key_str = key.iter().map(|b| *b as char).collect::<Vec<_>>();
            result_vec.push(String::from_iter(key_str));
            iter.next();
        }
        assert_eq!(result_vec, ["hello"]);

        // test full_history_ts_low works
        let mut compact_opts = CompactOptions::default();
        compact_opts.set_full_history_ts_low(ts2);
        db.compact_range_cf_opt(&cf, None::<&[u8]>, None::<&[u8]>, &compact_opts);
        db.flush().unwrap();

        // Attempt to read `full_history_ts_low`.
        // It should match the value we set earlier (`ts2`).
        let full_history_ts_low = db.get_full_history_ts_low(&cf).unwrap();
        assert_eq!(U64Timestamp::from(full_history_ts_low.as_slice()), ts2);

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts3);
        let value = db.get_cf_opt(&cf, key, &opts).unwrap();
        assert_eq!(value, None);
        // cannot read with timestamp older than full_history_ts_low
        opts.set_timestamp(ts);
        assert!(db.get_cf_opt(&cf, key, &opts).is_err());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_get_cf_with_ts_opt() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_get_cf_with_ts_opt")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![("cf", cf_opts)];
        let db = DB::open_cf_with_opts(&db_opts, path, cfs).unwrap();
        let cf = db.cf_handle("cf").unwrap();

        let key = b"hello";
        let val1 = b"world0";
        let val2 = b"world1";

        let ts1 = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);

        // Write two versions
        db.put_cf_with_ts(&cf, key, ts1, val1).unwrap();
        db.put_cf_with_ts(&cf, key, ts2, val2).unwrap();

        // Read at ts2 — should get val2 with matched timestamp ts2
        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts2);
        let (value, matched_ts) = db.get_cf_with_ts_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);
        assert_eq!(U64Timestamp::from(matched_ts.unwrap().as_slice()), ts2);

        // Read at ts1 — should get val1 with matched timestamp ts1
        opts.set_timestamp(ts1);
        let (value, matched_ts) = db.get_cf_with_ts_opt(&cf, key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);
        assert_eq!(U64Timestamp::from(matched_ts.unwrap().as_slice()), ts1);

        // Read a non-existent key — should get (None, None)
        opts.set_timestamp(ts2);
        let (value, matched_ts) = db.get_cf_with_ts_opt(&cf, b"missing", &opts).unwrap();
        assert!(value.is_none());
        assert!(matched_ts.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_get_with_ts_opt() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_get_with_ts_opt")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let db = DB::open(&db_opts, path).unwrap();

        let key = b"hello";
        let val1 = b"world0";
        let val2 = b"world1";

        let ts1 = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);

        db.put_with_ts(key, ts1, val1).unwrap();
        db.put_with_ts(key, ts2, val2).unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts2);
        let (value, matched_ts) = db.get_with_ts_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val2);
        assert_eq!(U64Timestamp::from(matched_ts.unwrap().as_slice()), ts2);

        opts.set_timestamp(ts1);
        let (value, matched_ts) = db.get_with_ts_opt(key, &opts).unwrap();
        assert_eq!(value.unwrap().as_slice(), val1);
        assert_eq!(U64Timestamp::from(matched_ts.unwrap().as_slice()), ts1);

        opts.set_timestamp(ts2);
        let (value, matched_ts) = db.get_with_ts_opt(b"missing", &opts).unwrap();
        assert!(value.is_none());
        assert!(matched_ts.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_multi_get_with_ts_opt() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_multi_get_with_ts_opt")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let db = DB::open(&db_opts, path).unwrap();

        let ts1 = U64Timestamp::new(1);
        db.put_with_ts(b"k1", ts1, b"v1").unwrap();
        db.put_with_ts(b"k2", ts1, b"v2").unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts1);
        let results =
            db.multi_get_with_ts_opt([b"k1".as_ref(), b"k2".as_ref(), b"k3".as_ref()], &opts);
        assert_eq!(results.len(), 3);

        let (val, ts) = results[0].as_ref().unwrap();
        assert_eq!(val.as_deref(), Some(b"v1".as_ref()));
        assert_eq!(U64Timestamp::from(ts.as_deref().unwrap()), ts1);

        let (val, ts) = results[1].as_ref().unwrap();
        assert_eq!(val.as_deref(), Some(b"v2".as_ref()));
        assert_eq!(U64Timestamp::from(ts.as_deref().unwrap()), ts1);

        let (val, ts) = results[2].as_ref().unwrap();
        assert!(val.is_none());
        assert!(ts.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_multi_get_cf_with_ts_opt() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_multi_get_cf_with_ts_opt")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![("cf", cf_opts)];
        let db = DB::open_cf_with_opts(&db_opts, path, cfs).unwrap();
        let cf = db.cf_handle("cf").unwrap();

        let ts1 = U64Timestamp::new(1);
        db.put_cf_with_ts(&cf, b"k1", ts1, b"v1").unwrap();
        db.put_cf_with_ts(&cf, b"k2", ts1, b"v2").unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts1);
        let results = db.multi_get_cf_with_ts_opt(
            [
                (&cf, b"k1".as_ref()),
                (&cf, b"k2".as_ref()),
                (&cf, b"k3".as_ref()),
            ],
            &opts,
        );
        assert_eq!(results.len(), 3);

        let (val, ts) = results[0].as_ref().unwrap();
        assert_eq!(val.as_deref(), Some(b"v1".as_ref()));
        assert_eq!(U64Timestamp::from(ts.as_deref().unwrap()), ts1);

        let (val, ts) = results[1].as_ref().unwrap();
        assert_eq!(val.as_deref(), Some(b"v2".as_ref()));
        assert_eq!(U64Timestamp::from(ts.as_deref().unwrap()), ts1);

        let (val, ts) = results[2].as_ref().unwrap();
        assert!(val.is_none());
        assert!(ts.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_singledelete_with_ts() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_singledelete_with_ts")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let db = DB::open(&db_opts, path).unwrap();

        let ts1 = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);
        let ts3 = U64Timestamp::new(3);

        db.put_with_ts(b"k1", ts1, b"v1").unwrap();
        db.singledelete_with_ts(b"k1", ts2).unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts3);
        let (value, _) = db.get_with_ts_opt(b"k1", &opts).unwrap();
        assert!(value.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_singledelete_cf_with_ts() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_singledelete_cf_with_ts")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![("cf", cf_opts)];
        let db = DB::open_cf_with_opts(&db_opts, path, cfs).unwrap();
        let cf = db.cf_handle("cf").unwrap();

        let ts1 = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);
        let ts3 = U64Timestamp::new(3);

        db.put_cf_with_ts(&cf, b"k1", ts1, b"v1").unwrap();
        db.singledelete_cf_with_ts(&cf, b"k1", ts2).unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts3);
        let (value, _) = db.get_cf_with_ts_opt(&cf, b"k1", &opts).unwrap();
        assert!(value.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}

#[test]
fn test_writebatch_singledelete_cf_with_ts() {
    let tempdir = tempfile::Builder::new()
        .prefix("_path_for_rocksdb_storage_writebatch_singledelete_cf_with_ts")
        .tempdir()
        .expect("Failed to create temporary path.");
    let path = tempdir.path();
    let _ = DB::destroy(&Options::default(), path);

    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let mut cf_opts = Options::default();
        cf_opts.set_comparator_with_ts(
            U64Comparator::NAME,
            U64Timestamp::SIZE,
            Box::new(U64Comparator::compare),
            Box::new(U64Comparator::compare_ts),
            Box::new(U64Comparator::compare_without_ts),
        );

        let cfs = vec![("cf", cf_opts)];
        let db = DB::open_cf_with_opts(&db_opts, path, cfs).unwrap();
        let cf = db.cf_handle("cf").unwrap();

        let ts1 = U64Timestamp::new(1);
        let ts2 = U64Timestamp::new(2);
        let ts3 = U64Timestamp::new(3);

        db.put_cf_with_ts(&cf, b"k1", ts1, b"v1").unwrap();

        let mut batch = WriteBatch::default();
        batch.singledelete_cf_with_ts(&cf, b"k1", ts2);
        db.write_opt(batch, &WriteOptions::default()).unwrap();

        let mut opts = ReadOptions::default();
        opts.set_timestamp(ts3);
        let (value, _) = db.get_cf_with_ts_opt(&cf, b"k1", &opts).unwrap();
        assert!(value.is_none());
    }

    let _ = DB::destroy(&Options::default(), path);
}
